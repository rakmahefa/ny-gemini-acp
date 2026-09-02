//! Canonical model-context serialization owned by the agent runtime.
//!
//! Provider and protocol adapters must not invent their own tool-call/result
//! presentation. Structured history is serialized here using one stable text
//! grammar. Parsers may keep backward-compatible readers, but writers should
//! always emit this canonical representation.

use serde::Serialize;
use serde_json::{json, Value};

pub const TOOL_CALL_OPEN: &str = "```tool_call";
pub const TOOL_CALL_CLOSE: &str = "```";
pub const TOOL_RESULT_PREFIX: &str = "[Tool result]:";

#[derive(Serialize)]
struct PromptToolResultRecord<'a> {
    content: &'a str,
    id: &'a str,
    status: &'static str,
    tool: &'a str,
}

/// Serialize a semantic tool call into the canonical model-facing grammar.
pub fn format_tool_call(id: &str, name: &str, arguments: &Value) -> String {
    let envelope = json!({
        "name": name,
        "id": id,
        "arguments": arguments,
    });
    format!("{TOOL_CALL_OPEN}\n{}\n{TOOL_CALL_CLOSE}", envelope)
}

/// Serialize an executed tool result onto one protocol-safe line.
///
/// `content` is encoded as JSON data, so arbitrary newlines, quotes, fences,
/// Unicode and legacy markers cannot become syntax in the surrounding prompt.
/// The envelope uses a struct to make JSON field order deterministic, which is
/// part of this text protocol's canonical representation.
pub fn format_tool_result(id: &str, name: &str, content: &str, is_ok: bool) -> String {
    let envelope = PromptToolResultRecord {
        content,
        id,
        status: if is_ok { "ok" } else { "error" },
        tool: name,
    };
    let encoded =
        serde_json::to_string(&envelope).expect("tool result envelope serialization cannot fail");
    format!("{TOOL_RESULT_PREFIX} {encoded}")
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

    #[test]
    fn tool_result_is_canonical_field_order() {
        assert_eq!(
            format_tool_result("", "file_read", "contenu du fichier", true),
            "[Tool result]: {\"content\":\"contenu du fichier\",\"id\":\"\",\"status\":\"ok\",\"tool\":\"file_read\"}"
        );
    }
}
