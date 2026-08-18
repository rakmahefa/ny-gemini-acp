//! Streaming parser for the Gemini `<ElicitationsGroup>` interaction envelope.
//!
//! This parser is deliberately independent from executable tool detection and
//! from the final ACP presentation filter. It turns the XML-like wire format
//! observed in Zed captures into typed semantic interaction data while removing
//! the envelope from visible assistant text.

const GROUP_OPEN: &str = "<ElicitationsGroup";
const GROUP_CLOSE: &str = "</ElicitationsGroup>";
const ACTION_OPEN: &str = "<Elicitation";
const MAX_GROUP_BYTES: usize = 128 * 1024;
const MAX_LABEL_CHARS: usize = 160;
const MAX_QUERY_CHARS: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ElicitationAction {
    pub(crate) label: String,
    pub(crate) query: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InteractionGroup {
    pub(crate) message: Option<String>,
    pub(crate) actions: Vec<ElicitationAction>,
}

#[derive(Debug, Default)]
pub(crate) struct InteractionStreamParser {
    pending: String,
}

impl InteractionStreamParser {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Removes complete interaction groups from visible text and returns the
    /// semantic groups that were decoded from them.
    pub(crate) fn push(&mut self, chunk: &str) -> InteractionParseResult {
        self.pending.push_str(chunk);
        self.drain(false)
    }

    pub(crate) fn finish(&mut self) -> InteractionParseResult {
        self.drain(true)
    }

    fn drain(&mut self, final_flush: bool) -> InteractionParseResult {
        let mut out = String::new();
        let mut groups = Vec::new();

        loop {
            let Some(start) = self.pending.find(GROUP_OPEN) else {
                if final_flush {
                    out.push_str(&self.pending);
                    self.pending.clear();
                    return InteractionParseResult { visible: out, groups };
                }

                let keep = partial_suffix_len(&self.pending, GROUP_OPEN);
                let emit_len = self.pending.len().saturating_sub(keep);
                if emit_len > 0 {
                    out.push_str(&self.pending[..emit_len]);
                    self.pending = self.pending[emit_len..].to_owned();
                }
                return InteractionParseResult { visible: out, groups };
            };

            if start > 0 {
                out.push_str(&self.pending[..start]);
                self.pending = self.pending[start..].to_owned();
            }

            if self.pending.len() > MAX_GROUP_BYTES {
                tracing::warn!(
                    "dropping oversized ElicitationsGroup envelope from assistant presentation"
                );
                self.pending.clear();
                return InteractionParseResult { visible: out, groups };
            }

            let Some(end_rel) = self.pending.find(GROUP_CLOSE) else {
                if final_flush {
                    tracing::warn!(
                        "dropping incomplete ElicitationsGroup envelope at stream end"
                    );
                    self.pending.clear();
                }
                return InteractionParseResult { visible: out, groups };
            };

            let end = end_rel + GROUP_CLOSE.len();
            let group_text = self.pending[..end].to_owned();
            self.pending = self.pending[end..].to_owned();

            match parse_group(&group_text) {
                Some(group) if !group.actions.is_empty() => groups.push(group),
                Some(_) => tracing::warn!(
                    "dropping ElicitationsGroup without valid Elicitation actions"
                ),
                None => tracing::warn!(
                    "dropping malformed ElicitationsGroup envelope"
                ),
            }
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct InteractionParseResult {
    pub(crate) visible: String,
    pub(crate) groups: Vec<InteractionGroup>,
}

fn parse_group(text: &str) -> Option<InteractionGroup> {
    let open_end = find_tag_end(text.strip_prefix(GROUP_OPEN)?)?;
    let open_tag = &text[..GROUP_OPEN.len() + open_end + 1];
    let message = parse_attributes(&open_tag[1..])
        .get("message")
        .cloned()
        .filter(|value| !value.trim().is_empty())
        .map(|value| decode_entities(value.trim()));

    let inner_start = GROUP_OPEN.len() + open_end + 1;
    let inner_end = text.len().checked_sub(GROUP_CLOSE.len())?;
    if inner_end < inner_start {
        return None;
    }

    let inner = &text[inner_start..inner_end];
    let mut actions = Vec::new();
    let mut cursor = 0;
    while let Some(relative_start) = inner[cursor..].find(ACTION_OPEN) {
        let start = cursor + relative_start;
        let rest = &inner[start..];
        let tag_end = find_tag_end(rest)?;
        let tag = &rest[..tag_end + 1];
        let attrs = parse_attributes(&tag[1..]);
        let label = attrs
            .get("label")
            .map(|value| decode_entities(value.trim()))
            .filter(|value| !value.is_empty())?;
        let query = attrs
            .get("query")
            .map(|value| decode_entities(value.trim()))
            .filter(|value| !value.is_empty())?;

        if label.chars().count() > MAX_LABEL_CHARS || query.chars().count() > MAX_QUERY_CHARS {
            return None;
        }

        actions.push(ElicitationAction { label, query });
        cursor = start + tag_end + 1;
    }

    Some(InteractionGroup { message, actions })
}

fn parse_attributes(input: &str) -> std::collections::BTreeMap<String, String> {
    let mut attrs = std::collections::BTreeMap::new();
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] == b'/' || bytes[index] == b'>' {
            break;
        }

        let key_start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && bytes[index] != b'='
            && bytes[index] != b'>'
        {
            index += 1;
        }
        if key_start == index {
            index += 1;
            continue;
        }

        let key = input[key_start..index].to_ascii_lowercase();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            break;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }

        let value = if bytes[index] == b'\'' || bytes[index] == b'"' {
            let quote = bytes[index];
            index += 1;
            let value_start = index;
            while index < bytes.len() && bytes[index] != quote {
                index += 1;
            }
            let value = input[value_start..index].to_owned();
            if index < bytes.len() {
                index += 1;
            }
            value
        } else {
            let value_start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'>' {
                index += 1;
            }
            input[value_start..index].to_owned()
        };

        attrs.insert(key, value);
    }

    attrs
}

fn find_tag_end(input: &str) -> Option<usize> {
    let mut quote = None;
    for (index, byte) in input.as_bytes().iter().copied().enumerate() {
        match quote {
            Some(current) if byte == current => quote = None,
            Some(_) => {}
            None if byte == b'\'' || byte == b'"' => quote = Some(byte),
            None if byte == b'>' => return Some(index),
            None => {}
        }
    }
    None
}

fn partial_suffix_len(text: &str, marker: &str) -> usize {
    let max = text.len().min(marker.len().saturating_sub(1));
    for len in (1..=max).rev() {
        if text.ends_with(&marker[..len]) {
            return len;
        }
    }
    0
}

fn decode_entities(input: &str) -> String {
    input
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(chunks: &[&str]) -> InteractionParseResult {
        let mut parser = InteractionStreamParser::new();
        let mut result = InteractionParseResult::default();
        for chunk in chunks {
            let delta = parser.push(chunk);
            result.visible.push_str(&delta.visible);
            result.groups.extend(delta.groups);
        }
        let tail = parser.finish();
        result.visible.push_str(&tail.visible);
        result.groups.extend(tail.groups);
        result
    }

    #[test]
    fn parses_real_zed_group_shape() {
        let result = parse(&[
            "Avant\n<ElicitationsGroup message=\"Choisissez l'action suivante à exécuter :\">\n",
            "  <Elicitation label=\"Lancer les tests\" query=\"Lancer les tests\"/>\n",
            "  <Elicitation label=\"Lancer investigation\" query=\"Lancer investigation\"/>\n",
            "  <Elicitation label=\"Inspecter l'architecture MCP\" query=\"Inspecter l'architecture MCP\"/>\n",
            "</ElicitationsGroup>\nAprès",
        ]);

        assert_eq!(result.visible, "Avant\n\nAprès");
        assert_eq!(result.groups.len(), 1);
        assert_eq!(
            result.groups[0].message.as_deref(),
            Some("Choisissez l'action suivante à exécuter :")
        );
        assert_eq!(result.groups[0].actions.len(), 3);
        assert_eq!(result.groups[0].actions[0].label, "Lancer les tests");
        assert_eq!(result.groups[0].actions[2].query, "Inspecter l'architecture MCP");
    }

    #[test]
    fn arbitrary_chunk_boundaries_do_not_change_group_semantics() {
        let input = "Avant <ElicitationsGroup message=\"Choix\"><Elicitation label=\"A\" query=\"Q\"/><Elicitation label=\"B\" query=\"Q2\"/></ElicitationsGroup> Après";
        let reference = parse(&[input]);
        for split in input
            .char_indices()
            .map(|(index, _)| index)
            .filter(|index| *index > 0)
        {
            let (left, right) = input.split_at(split);
            let actual = parse(&[left, right]);
            assert_eq!(actual.visible, reference.visible, "split at {split}");
            assert_eq!(actual.groups, reference.groups, "split at {split}");
        }
    }

    #[test]
    fn quoted_gt_is_not_a_tag_boundary() {
        let result = parse(&[
            "<ElicitationsGroup message=\"A > B\"><Elicitation label=\"C > D\" query=\"x > y\"/></ElicitationsGroup>fin",
        ]);
        assert_eq!(result.visible, "fin");
        assert_eq!(result.groups[0].actions[0].label, "C > D");
        assert_eq!(result.groups[0].actions[0].query, "x > y");
    }

    #[test]
    fn entities_are_decoded() {
        let result = parse(&[
            "<ElicitationsGroup message=\"Choisir &amp; continuer\"><Elicitation label=\"A &quot;test&quot;\" query=\"x &amp; y\"/></ElicitationsGroup>",
        ]);
        assert_eq!(result.groups[0].message.as_deref(), Some("Choisir & continuer"));
        assert_eq!(result.groups[0].actions[0].label, "A \"test\"");
        assert_eq!(result.groups[0].actions[0].query, "x & y");
    }

    #[test]
    fn ordinary_less_than_text_is_preserved() {
        let result = parse(&["2 < 3 et x < y"]);
        assert_eq!(result.visible, "2 < 3 et x < y");
        assert!(result.groups.is_empty());
    }

    #[test]
    fn incomplete_group_is_not_leaked_at_eof() {
        let result = parse(&["Avant <ElicitationsGroup message=\"Choix\"><Elicitation label=\"A\" query=\"Q\"/>"]);
        assert_eq!(result.visible, "Avant ");
        assert!(result.groups.is_empty());
    }

    #[test]
    fn malformed_group_is_consumed_without_leaking_markup() {
        let result = parse(&[
            "Avant <ElicitationsGroup message=\"Choix\"><Elicitation label=\"A\"/></ElicitationsGroup> Après",
        ]);
        assert_eq!(result.visible, "Avant  Après");
        assert!(result.groups.is_empty());
    }
}
