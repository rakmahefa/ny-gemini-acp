use std::collections::BTreeMap;

/// Parses XML-like `key=value` attributes while accepting arbitrary ordering,
/// optional whitespace around `=`, and both single and double quoted values.
///
/// This helper is intentionally protocol-neutral: it does not know about any
/// provider, tool name, or ACP type.
pub fn parse_tag_attributes(input: &str) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] == b'/' {
            break;
        }

        let key_start = index;
        while index < bytes.len() && !bytes[index].is_ascii_whitespace() && bytes[index] != b'=' {
            index += 1;
        }
        if key_start == index {
            index += 1;
            continue;
        }

        let key = &input[key_start..index];
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
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            input[value_start..index].to_owned()
        };

        attrs.insert(key.to_ascii_lowercase(), value);
    }

    attrs
}

/// Décode les entités XML de base. `&amp;` est traité en dernier pour ne pas
/// altérer les entités déjà décodées.
///
/// Implémentation unique du workspace (C-27) : consommée par le parsing de
/// flux du runtime LLM (`llm-provider::semantic_stream`) et par le parsing
/// des tool calls (`tools_provider::tools::parse`).
pub fn decode_xml_entities(input: &str) -> String {
    input
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Préfixe canonique des balises FollowUp émises par le modèle.
pub const FOLLOW_UP_TAG_PREFIX: &str = "<FollowUp";

/// Retourne l'index du `>` fermant une balise, en ignorant les `>` situés
/// entre guillemets (simples ou doubles).
pub fn find_tag_end(input: &str) -> Option<usize> {
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

/// Parse un tag `<FollowUp label="…" query="…" />` en `(label, query)`.
/// Implémentation unique du workspace (C-27).
pub fn parse_follow_up_tag(tag: &str) -> Option<(String, String)> {
    let inner = tag
        .strip_prefix(FOLLOW_UP_TAG_PREFIX)?
        .strip_suffix('>')?
        .trim();
    let inner = inner.strip_suffix('/').unwrap_or(inner).trim();
    let attrs = parse_tag_attributes(inner);
    let label = attrs.get("label")?.trim();
    let query = attrs.get("query")?.trim();
    if label.is_empty() || query.is_empty() {
        return None;
    }
    Some((decode_xml_entities(label), decode_xml_entities(query)))
}

/// Troncature sûre sur frontière de caractère avec ellipse finale.
/// Implémentation unique du workspace (C-26).
pub fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    format!("{}…", value.chars().take(max_chars).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::{
        decode_xml_entities, find_tag_end, parse_follow_up_tag, parse_tag_attributes,
        truncate_chars,
    };

    #[test]
    fn accepts_reordered_double_quoted_attributes() {
        let attrs = parse_tag_attributes(r#"query="cargo test" label="Run tests""#);
        assert_eq!(attrs.get("label"), Some(&"Run tests".to_owned()));
        assert_eq!(attrs.get("query"), Some(&"cargo test".to_owned()));
    }

    #[test]
    fn accepts_single_quoted_attributes_and_whitespace() {
        let attrs = parse_tag_attributes("label = 'Run tests' query = 'cargo test'");
        assert_eq!(attrs.get("label"), Some(&"Run tests".to_owned()));
        assert_eq!(attrs.get("query"), Some(&"cargo test".to_owned()));
    }

    #[test]
    fn decodes_xml_entities_with_amp_last() {
        assert_eq!(decode_xml_entities("&amp;quot;"), "&quot;");
        assert_eq!(
            decode_xml_entities("a&lt;b&gt;&apos;&quot;c&quot;"),
            concat!("a<b>'", '"', "c", '"')
        );
    }

    #[test]
    fn finds_tag_end_ignoring_gt_inside_quotes() {
        assert_eq!(
            find_tag_end(" label=\"A > B\" query=\"x\" /> tail"),
            Some(26)
        );
        assert_eq!(find_tag_end("without close"), None);
    }

    #[test]
    fn parses_follow_up_tag_variants() {
        let (label, query) =
            parse_follow_up_tag("<FollowUp label=\"Run tests\" query=\"cargo test\" />").unwrap();
        assert_eq!(
            (label.as_str(), query.as_str()),
            ("Run tests", "cargo test")
        );
        let (label, query) = parse_follow_up_tag("<FollowUp label=\"L\" query=\"Q\">").unwrap();
        assert_eq!((label.as_str(), query.as_str()), ("L", "Q"));
        assert!(parse_follow_up_tag("<FollowUp label=\"\" query=\"Q\" />").is_none());
    }

    #[test]
    fn truncates_on_char_boundary_with_ellipsis() {
        assert_eq!(truncate_chars("abcdef", 3), "abc…");
        assert_eq!(truncate_chars("abc", 5), "abc");
        assert_eq!(truncate_chars("héhé", 2), "hé…");
    }
}
