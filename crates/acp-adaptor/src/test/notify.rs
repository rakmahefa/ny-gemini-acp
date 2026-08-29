use super::*;
use agent_client_protocol::schema::v1::{MessageId, SessionId};
use agent_runtime::{ToolUiKind, ToolUiModel, ToolUiStatus};
use serde_json::json;

#[test]
fn usage_estime_tokens_en_contexte() {
    let u = usage_update("question 🚀", "réponse");
    assert_eq!(u.used, (10 + 8) / 4);
    assert_eq!(u.size, CONTEXT_TOKENS);
    assert!(u.cost.is_none());
}

#[test]
fn maps_ui_kind_to_native_acp_tool_kind() {
    assert_eq!(tool_kind(ToolUiKind::FileRead), ToolKind::Read);
    assert_eq!(tool_kind(ToolUiKind::FileEdit), ToolKind::Edit);
    assert_eq!(tool_kind(ToolUiKind::Glob), ToolKind::Search);
    assert_eq!(tool_kind(ToolUiKind::Shell), ToolKind::Execute);
    assert_eq!(tool_kind(ToolUiKind::AskUserQuestion), ToolKind::Other);
}

#[test]
fn maps_ui_status_to_native_acp_status() {
    assert_eq!(tool_status(ToolUiStatus::Pending), ToolCallStatus::Pending);
    assert_eq!(tool_status(ToolUiStatus::Running), ToolCallStatus::InProgress);
    assert_eq!(tool_status(ToolUiStatus::Succeeded), ToolCallStatus::Completed);
    assert_eq!(tool_status(ToolUiStatus::Failed), ToolCallStatus::Failed);
    assert_eq!(tool_status(ToolUiStatus::Cancelled), ToolCallStatus::Failed);
}

#[test]
fn text_notification_contains_session_message_id_and_text() {
    let notification = text_notification(
        &SessionId::from("sess-1"),
        &MessageId::from("msg-1"),
        "hello".into(),
    );
    let json = serde_json::to_string(&notification).unwrap();
    assert!(json.contains("sess-1"));
    assert!(json.contains("msg-1"));
    assert!(json.contains("hello"));
    assert!(json.contains("agent_message_chunk"));
}

#[test]
fn reasoning_notification_uses_agent_thought_chunk() {
    let notification = reasoning_notification(
        &SessionId::from("sess-1"),
        &MessageId::from("msg-1"),
        "thinking".into(),
    );
    let json = serde_json::to_string(&notification).unwrap();
    assert!(json.contains("agent_thought_chunk"));
    assert!(json.contains("thinking"));
}

#[test]
fn tool_call_notification_preserves_id_input_output_and_status() {
    let ui = ToolUiModel::pending(
        ToolUiKind::FileRead,
        "Read file",
        "src/main.rs",
        json!({"path":"src/main.rs"}),
    )
    .completed(true, Some(json!({"text":"fn main() {}"})));

    let notification = tool_call_notification(&SessionId::from("sess-1"), "call-7", &ui).unwrap();
    let json = serde_json::to_string(&notification).unwrap();
    assert!(json.contains("call-7"));
    assert!(json.contains("src/main.rs"));
    assert!(json.contains("fn main() {}"));
    assert!(json.contains("completed"));
    assert!(json.contains("tool_call"));
}

#[test]
fn tool_call_update_notification_preserves_terminal_tool_status() {
    let ui = ToolUiModel::pending(
        ToolUiKind::Shell,
        "Run command",
        "pwd",
        json!({"command":"pwd"}),
    )
    .completed(true, Some(json!({"text":"/tmp"})));

    let notification = tool_call_update_notification(&SessionId::from("sess-1"), "call-8", &ui).unwrap();
    let json = serde_json::to_string(&notification).unwrap();
    assert!(json.contains("call-8"));
    assert!(json.contains("/tmp"));
    assert!(json.contains("completed"));
    assert!(json.contains("tool_call_update"));
}

#[test]
fn usage_notification_preserves_usage_contract() {
    let notification = usage_notification(&SessionId::from("sess-1"), "hello", "world");
    let json = serde_json::to_string(&notification).unwrap();
    assert!(json.contains("usage_update"));
    assert!(json.contains("size"));
    assert!(json.contains("1000000"));
}

#[test]
fn tool_call_projection_keeps_structured_input_and_output() {
    let ui = ToolUiModel::pending(
        ToolUiKind::FileRead,
        "Read file",
        "src/main.rs",
        json!({"path":"src/main.rs","offset":10,"limit":20}),
    )
    .completed(true, Some(json!({"text":"fn main() {}"})));

    let call = tool_call_from_ui("turn_1/tool_0", &ui).unwrap();
    assert_eq!(call.tool_call_id.0, "turn_1/tool_0".into());
    assert_eq!(call.kind, ToolKind::Read);
    assert_eq!(call.status, ToolCallStatus::Completed);
    assert_eq!(call.raw_input, Some(json!({"path":"src/main.rs","offset":10,"limit":20})));
    assert_eq!(call.raw_output, Some(json!({"text":"fn main() {}"})));
}

#[test]
fn file_edit_projects_to_acp_edit_diff_and_location() {
    let ui = ToolUiModel::pending(
        ToolUiKind::FileEdit,
        "Edit file",
        "test.txt",
        json!({"path":"test.txt"}),
    )
    .with_content(vec![json!({
        "type": "content",
        "text": "**✏️ File Edit**"
    }), json!({
        "type": "diff",
        "path": "test.txt",
        "oldText": "before",
        "newText": "after"
    })])
    .with_locations(vec![json!({"path":"/tmp/test.txt","line":2})]);

    let call = tool_call_from_ui("turn_1/tool_1", &ui).unwrap();
    assert_eq!(call.kind, ToolKind::Edit);
    assert_eq!(call.content.len(), 2);
    assert!(format!("{:?}", call.content[1]).contains("Diff"));
    assert_eq!(call.locations.len(), 1);
    assert_eq!(call.locations[0].path, std::path::PathBuf::from("/tmp/test.txt"));
    assert_eq!(call.locations[0].line, Some(2));
}

#[test]
fn shell_projects_to_acp_execute_and_terminal() {
    let ui = ToolUiModel::pending(
        ToolUiKind::Shell,
        "Run command",
        "pwd",
        json!({"command":"pwd"}),
    )
    .with_content(vec![json!({"type":"content","text":"**▣ Shell**"}), json!({"type":"terminal","id":"term-7"})]);

    let call = tool_call_from_ui("turn_1/tool_2", &ui).unwrap();
    assert_eq!(call.kind, ToolKind::Execute);
    assert!(call.content.iter().any(|content| format!("{content:?}").contains("Terminal")));
}

#[test]
fn file_read_projects_to_acp_read_and_location() {
    let ui = ToolUiModel::pending(
        ToolUiKind::FileRead,
        "Read file",
        "src/main.rs",
        json!({"path":"src/main.rs"}),
    )
    .with_locations(vec![json!({"path":"/tmp/src/main.rs","line":10})]);

    let call = tool_call_from_ui("turn_1/tool_3", &ui).unwrap();
    assert_eq!(call.kind, ToolKind::Read);
    assert_eq!(call.locations.len(), 1);
    assert_eq!(call.locations[0].path, std::path::PathBuf::from("/tmp/src/main.rs"));
    assert_eq!(call.locations[0].line, Some(10));
}

#[test]
fn malformed_rich_content_is_rejected_instead_of_dropped() {
    let ui = ToolUiModel::pending(
        ToolUiKind::FileEdit,
        "Edit file",
        "test.txt",
        json!({"path":"test.txt"}),
    )
    .with_content(vec![json!({"type":"diff","path":"test.txt"})]);

    assert!(tool_call_from_ui("turn_1/tool_4", &ui).is_err());
}
