//! Semantic contract joining raw protocol detection and ACP presentation.
//!
//! A single owner feeds the raw Gemini response to its semantic parsers in a
//! fixed order and validates their shared invariants. Interaction envelopes are
//! removed before executable-tool detection and ACP presentation so their XML
//! syntax can never become visible assistant content or an executable tool.

use std::collections::HashSet;

use gemini_acp_runtime::tools::parse::ParsedToolCall;

use super::interaction::{InteractionGroup, InteractionStreamParser};
use super::protocol::PROTOCOL_MARKERS;
use super::{protocol_filter::ProtocolFilter, tool_stream::ToolStreamDetector};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContractViolation {
    ProtocolLeakedToAssistant,
    EmptyToolCallId,
    EmptyToolName,
}

impl std::fmt::Display for ContractViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProtocolLeakedToAssistant => {
                f.write_str("protocol syntax escaped the ACP presentation filter")
            }
            Self::EmptyToolCallId => f.write_str("tool call has an empty id"),
            Self::EmptyToolName => f.write_str("tool call has an empty name"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct StreamDelta {
    pub(crate) visible: String,
    pub(crate) tool_calls: Vec<ParsedToolCall>,
    pub(crate) interaction_groups: Vec<InteractionGroup>,
}

#[derive(Debug)]
pub(crate) struct SemanticStreamContract {
    interactions: InteractionStreamParser,
    detector: ToolStreamDetector,
    filter: ProtocolFilter,
    seen_tool_ids: HashSet<String>,
    next_rekey: u64,
}

impl Default for SemanticStreamContract {
    fn default() -> Self {
        Self {
            interactions: InteractionStreamParser::new(),
            detector: ToolStreamDetector::new(),
            filter: ProtocolFilter::new(),
            seen_tool_ids: HashSet::new(),
            next_rekey: 0,
        }
    }
}

impl SemanticStreamContract {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn feed(&mut self, raw: &str) -> Result<StreamDelta, ContractViolation> {
        let parsed = self.interactions.push(raw);
        self.feed_normalized(parsed.visible, parsed.groups, false)
    }

    pub(crate) fn finish(&mut self) -> Result<StreamDelta, ContractViolation> {
        let parsed = self.interactions.finish();
        self.feed_normalized(parsed.visible, parsed.groups, true)
    }

    fn feed_normalized(
        &mut self,
        normalized: String,
        interaction_groups: Vec<InteractionGroup>,
        final_chunk: bool,
    ) -> Result<StreamDelta, ContractViolation> {
        let tool_calls = if final_chunk {
            self.detector.finish()
        } else {
            self.detector.feed(&normalized)
        };
        let tool_calls = self.validate_and_rekey(tool_calls)?;
        let visible = if final_chunk {
            self.filter.finish()
        } else {
            self.filter.push(&normalized)
        };
        self.validate_visible(&visible)?;
        Ok(StreamDelta {
            visible,
            tool_calls,
            interaction_groups,
        })
    }

    fn validate_and_rekey(
        &mut self,
        calls: Vec<ParsedToolCall>,
    ) -> Result<Vec<ParsedToolCall>, ContractViolation> {
        let mut accepted = Vec::with_capacity(calls.len());
        for mut call in calls {
            if call.id.trim().is_empty() {
                return Err(ContractViolation::EmptyToolCallId);
            }
            if call.name.trim().is_empty() {
                return Err(ContractViolation::EmptyToolName);
            }

            if self.seen_tool_ids.insert(call.id.clone()) {
                accepted.push(call);
                continue;
            }

            let original = call.id.clone();
            let rekey = loop {
                let candidate = format!("gemini_stream_call_{}", self.next_rekey);
                self.next_rekey = self.next_rekey.saturating_add(1);
                if self.seen_tool_ids.insert(candidate.clone()) {
                    break candidate;
                }
            };
            tracing::warn!(
                original_id = %original,
                replacement_id = %rekey,
                tool = %call.name,
                "rekeying duplicate streamed tool call id"
            );
            call.id = rekey;
            accepted.push(call);
        }
        Ok(accepted)
    }

    fn validate_visible(&self, visible: &str) -> Result<(), ContractViolation> {
        if visible
            .lines()
            .map(str::trim_start)
            .any(|line| PROTOCOL_MARKERS.iter().any(|marker| line.starts_with(marker)))
        {
            tracing::error!("semantic stream contract violation: protocol leaked to assistant output");
            return Err(ContractViolation::ProtocolLeakedToAssistant);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::protocol::{
        ASSISTANT_MARKER, FUNCTION_CALL_FENCE, TOOL_CALL_FENCE,
        TOOL_CALL_SINGLE_QUOTE_FENCE, TOOL_RESULT_ENVELOPE, TOOL_RESULT_PREFIX, USER_MARKER,
    };

    fn collect(chunks: &[&str]) -> StreamDelta {
        let mut contract = SemanticStreamContract::new();
        let mut result = StreamDelta::default();
        for chunk in chunks {
            let delta = contract.feed(chunk).expect("valid contract stream");
            result.visible.push_str(&delta.visible);
            result.tool_calls.extend(delta.tool_calls);
            result.interaction_groups.extend(delta.interaction_groups);
        }
        let tail = contract.finish().expect("valid contract finish");
        result.visible.push_str(&tail.visible);
        result.tool_calls.extend(tail.tool_calls);
        result.interaction_groups.extend(tail.interaction_groups);
        result
    }

    #[test]
    fn canonical_stream_keeps_semantics_and_hides_protocol() {
        let result = collect(&[
            "[Assistant]:",
            " thinking\n",
            "```tool_call\n{\"id\":\"c1\",\"name\":\"shell_exec\",\"arguments\":{}}\n```\n",
            "[Tool result]: {\"content\":\"```tool_call\"}\n",
            "[Assistant]: Suite",
        ]);

        assert_eq!(result.visible, "thinking\nSuite");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].id, "c1");
        assert!(result.interaction_groups.is_empty());
    }

    #[test]
    fn elicitation_group_becomes_semantic_data_and_never_visible_text() {
        let result = collect(&[
            "Avant\n<ElicitationsGroup message=\"Choisir\"><Elicitation label=\"Tests\" query=\"cargo test\"/><Elicitation label=\"MCP\" query=\"inspect MCP\"/></ElicitationsGroup>\nAprès",
        ]);

        assert_eq!(result.visible, "Avant\n\nAprès");
        assert_eq!(result.interaction_groups.len(), 1);
        assert_eq!(result.interaction_groups[0].actions.len(), 2);
        assert_eq!(result.interaction_groups[0].actions[0].query, "cargo test");
        assert!(result.tool_calls.is_empty());
    }

    #[test]
    fn interaction_content_cannot_trigger_tool_detection() {
        let result = collect(&[
            "<ElicitationsGroup message=\"Choix\"><Elicitation label=\"x\" query=\"```tool_call {\"name\":\"shell_exec\"}\"/></ElicitationsGroup>",
        ]);
        assert_eq!(result.interaction_groups.len(), 1);
        assert!(result.tool_calls.is_empty());
        assert!(result.visible.is_empty());
    }

    #[test]
    fn arbitrary_chunk_boundaries_do_not_change_result() {
        let full = "[Assistant]: Debut\n<ElicitationsGroup message=\"Choix\"><Elicitation label=\"A\" query=\"Q\"/><Elicitation label=\"B\" query=\"Q2\"/></ElicitationsGroup>\n```function_call\n{\"name\":\"shell_exec\",\"args\":{}}\n```\n[Assistant]: Fin";
        let reference = collect(&[full]);
        for split in full
            .char_indices()
            .map(|(index, _)| index)
            .filter(|index| *index > 0)
        {
            let (left, right) = full.split_at(split);
            let actual = collect(&[left, right]);
            assert_eq!(actual.visible, reference.visible, "split at {split}");
            assert_eq!(actual.tool_calls, reference.tool_calls, "split at {split}");
            assert_eq!(actual.interaction_groups, reference.interaction_groups, "split at {split}");
        }
    }

    #[test]
    fn duplicate_tool_ids_are_rekeyed_not_executed_twice_under_one_id() {
        let result = collect(&[
            "```tool_call\n{\"id\":\"dup\",\"name\":\"shell_exec\",\"arguments\":{}}\n```\n",
            "```tool_call\n{\"id\":\"dup\",\"name\":\"file_read\",\"arguments\":{}}\n```\n",
        ]);
        assert_eq!(result.tool_calls.len(), 2);
        assert_eq!(result.tool_calls[0].id, "dup");
        assert_eq!(result.tool_calls[1].id, "gemini_stream_call_0");
    }

    #[test]
    fn tool_result_payload_is_not_reinterpreted() {
        let result = collect(&[
            "[Tool result]: {\"content\":\"line\\n```tool_call\\n{\\\"name\\\":\\\"shell_exec\\\"}\\n```\"}\n",
            "[Assistant]: Réponse",
        ]);
        assert!(result.tool_calls.is_empty());
        assert_eq!(result.visible, "Réponse");
    }

    #[test]
    fn ordinary_markdown_is_preserved() {
        let input = "Voici un exemple :\n```rust\nfn main() {}\n```";
        let result = collect(&[input]);
        assert_eq!(result.visible, input);
        assert!(result.tool_calls.is_empty());
        assert!(result.interaction_groups.is_empty());
    }

    #[test]
    fn filter_contract_has_no_duplicate_marker_definitions() {
        let expected = [
            TOOL_RESULT_PREFIX,
            TOOL_RESULT_ENVELOPE,
            ASSISTANT_MARKER,
            USER_MARKER,
            TOOL_CALL_FENCE,
            TOOL_CALL_SINGLE_QUOTE_FENCE,
            FUNCTION_CALL_FENCE,
        ];
        assert_eq!(expected.as_slice(), PROTOCOL_MARKERS);
    }
}
