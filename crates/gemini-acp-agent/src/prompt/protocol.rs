//! Shared protocol markers used by both semantic detection and ACP presentation.
//!
//! Keeping the protocol vocabulary in one module prevents the detector and the
//! presentation filter from silently drifting apart as new Gemini envelopes are
//! added.

pub(crate) const TOOL_RESULT_PREFIX: &str = "[Tool result for ";
pub(crate) const TOOL_RESULT_ENVELOPE: &str = "[Tool result]:";
pub(crate) const ASSISTANT_MARKER: &str = "[Assistant]:";
pub(crate) const USER_MARKER: &str = "[User]:";
pub(crate) const TOOL_CALL_FENCE: &str = "```tool_call";
pub(crate) const TOOL_CALL_SINGLE_QUOTE_FENCE: &str = "'''tool_call";
pub(crate) const FUNCTION_CALL_FENCE: &str = "```function_call";
pub(crate) const FOLLOW_UP_PREFIX: &str = "<FollowUp";

pub(crate) const PROTOCOL_MARKERS: &[&str] = &[
    TOOL_RESULT_PREFIX,
    TOOL_RESULT_ENVELOPE,
    ASSISTANT_MARKER,
    USER_MARKER,
    TOOL_CALL_FENCE,
    TOOL_CALL_SINGLE_QUOTE_FENCE,
    FUNCTION_CALL_FENCE,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_vocabulary_is_unique() {
        for (index, marker) in PROTOCOL_MARKERS.iter().enumerate() {
            assert!(
                !PROTOCOL_MARKERS[index + 1..].contains(marker),
                "duplicate protocol marker: {marker}"
            );
        }
    }

    #[test]
    fn function_call_is_part_of_shared_protocol_vocabulary() {
        assert!(PROTOCOL_MARKERS.contains(&FUNCTION_CALL_FENCE));
    }
}
