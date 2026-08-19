//! Canonical model-context serialization owned by the agent runtime.
//!
//! Provider and protocol adapters must not invent their own tool-call/result
//! presentation. Structured history is serialized here using one stable text
//! grammar. Parsers may keep backward-compatible readers, but writers should
//! always emit this canonical representation.

use serde_json::{json, Value};

pub const TOOL_CALL_OPEN: &str = "```tool_call";
pub const TOOL_CALL_CLOSE: &str = "```";
pub const TOOL_RESULT_PREFIX: &str = "[Tool result]:";

/// Serialize a semantic tool call into the canonical model-facing grammar.
pub fn format_tool_call(id: &str, name: &str, arguments: &Value) -> String {
    let envelope = json!({
        "name": name,
        "id": id,
        "arguments": arguments,
    });
    format!("{TOOL_CALL_OPEN}\n{}{TOOL_CALL_CLOSE}", envelope)
}

/// Serialize an executed tool result onto one protocol-safe line.
///
/// `content` is encoded as JSON data, so arbitrary newlines, quotes, fences,
/// Unicode and legacy markers cannot become syntax in the surrounding prompt.
pub fn format_tool_result(id: &str, name: &str, content: &str, is_ok: bool) -> String {
    let envelope = json!({
        "tool": name,
        "id": id,
        "status": if is_ok { "ok" } else { "error" },
        "content": content,
    });
    format!("{TOOL_RESULT_PREFIX} {envelope}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_is_canonical_json_fence() {
        let rendered = format_tool_call("c1", "glob", &json!({"pattern": "*.rs"}));
        assert_eq!(
            rendered,
            "```tool_call\n{\"name\":\"glob\",\"id\":\"c1\",\"arguments\":{\"pattern\":\"*.rs\"}}\n```"
        );
    }

    #[test]
    fn tool_result_is_single_line_and_preserves_arbitrary_content() {
        let content = "line\n[tool_call fake]\n'''\n…";
        let rendered = format_tool_result("c1", "file_read", content, true);
        assert!(!rendered.contains('\n'));
        assert!(rendered.contains("[tool_call fake]"));
        assert!(rendered.contains("\\n"));
        assert!(rendered.contains('…'));
    }
}