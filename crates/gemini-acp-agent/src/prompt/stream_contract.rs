//! Semantic contract joining raw protocol detection and ACP presentation.
//!
//! A single owner feeds the raw Gemini response to both state machines in a
//! fixed order and validates their shared invariants. This prevents the stream
//! consumer from accidentally evolving two independent interpretations of the
//! same bytes.

use std::collections::HashSet;

use gemini_acp_runtime::tools::parse::ParsedToolCall;

use super::protocol::PROTOCOL_MARKERS;
use super::{protocol_filter::ProtocolFilter, tool_stream::ToolStreamDetector};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContractViolation {
    ProtocolLeakedToAssistant,
    DuplicateToolCallId(String),
    EmptyToolCallId,
    EmptyToolName,
}

impl std::fmt::Display for ContractViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProtocolLeakedToAssistant => {
                f.write_str("protocol syntax escaped the ACP presentation filter")
            }
            Self::DuplicateToolCallId(id) => {
                write!(f, "duplicate tool call id: {id}")
            }
            Self::EmptyToolCallId => f.write_str("tool call has an empty id"),
            Self::EmptyToolName => f.write_str("tool call has an empty name"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct StreamDelta {
    pub(crate) visible: String,
    pub(crate) tool_calls: Vec<ParsedToolCall>,
}

/// Owns the semantic contract for one raw Gemini response stream.
///
/// The raw response is always consumed by the tool detector before it is
/// presented through the protocol filter. Tool call ids are made unique within
/// the stream, and any protocol envelope that escapes the presentation barrier
/// becomes a hard contract violation instead of silently reaching ACP clients.
#[derive(Debug)]
pub(crate) struct SemanticStreamContract {
    detector: ToolStreamDetector,
    filter: ProtocolFilter,
    seen_tool_ids: HashSet<String>,
    next_rekey: u64,
}

impl Default for SemanticStreamContract {
    fn default() -> Self {
        Self {
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
        let tool_calls = self.detector.feed(raw);
        let tool_calls = self.validate_and_rekey(tool_calls)?;
        let visible = self.filter.push(raw);
        self.validate_visible(&visible)?;
        Ok(StreamDelta { visible, tool_calls })
    }

    pub(crate) fn finish(&mut self) -> Result<StreamDelta, ContractViolation> {
        let tool_calls = self.detector.finish();
        let tool_calls = self.validate_and_rekey(tool_calls)?;
        let visible = self.filter.finish();
        self.validate_visible(&visible)?;
        Ok(StreamDelta { visible, tool_calls })
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
        }
        let tail = contract.finish().expect("valid contract finish");
        result.visible.push_str(&tail.visible);
        result.tool_calls.extend(tail.tool_calls);
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
    }

    #[test]
    fn arbitrary_chunk_boundaries_do_not_change_result() {
        let full = "[Assistant]: Début\n```function_call\n{\"name\":\"shell_exec\",\"args\":{}}\n```\n[Tool result]: {\"content\":\"x\"}\n[Assistant]: Fin";
        let reference = collect(&[full]);
        for split in 1..full.len() {
            let (left, right) = full.split_at(split);
            let actual = collect(&[left, right]);
            assert_eq!(actual.visible, reference.visible, "split at {split}");
            assert_eq!(actual.tool_calls, reference.tool_calls, "split at {split}");
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
        let input = "Voici du Markdown :\n```rust\nfn main() {}\n```";
        let result = collect(&[input]);
        assert_eq!(result.visible, input);
        assert!(result.tool_calls.is_empty());
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
