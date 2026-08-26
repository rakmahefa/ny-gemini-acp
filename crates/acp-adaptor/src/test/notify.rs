use super::*;
use agent_client_protocol::schema::v1::{MessageId, SessionId};
use agent_runtime::{ToolUiKind, ToolUiModel, ToolUiStatus};

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
    assert_eq!(
        tool_status(ToolUiStatus::Running),
        ToolCallStatus::InProgress
    );
    assert_eq!(
        tool_status(ToolUiStatus::Succeeded),
        ToolCallStatus::Completed
    );
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
        serde_json::json!({"path":"src/main.rs"}),
    )
    .completed(true, Some(serde_json::json!({"text":"fn main() {}"})));

    let notification = tool_call_notification(&SessionId::from("sess-1"), "call-7", &ui);
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
        serde_json::json!({"command":"pwd"}),
    )
    .completed(true, Some(serde_json::json!({"text":"/tmp"})));

    let notification = tool_call_update_notification(&SessionId::from("sess-1"), "call-8", &ui);
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
        serde_json::json!({"path":"src/main.rs","offset":10,"limit":20}),
    )
    .completed(true, Some(serde_json::json!({"text":"fn main() {}"})));

    let call = tool_call_from_ui("turn_1/tool_0", &ui);
    assert_eq!(call.tool_call_id.0, "turn_1/tool_0".into());
    assert_eq!(call.kind, ToolKind::Read);
    assert_eq!(call.status, ToolCallStatus::Completed);
    assert_eq!(
        call.raw_input,
        Some(serde_json::json!({"path":"src/main.rs","offset":10,"limit":20}))
    );
    assert_eq!(
        call.raw_output,
        Some(serde_json::json!({"text":"fn main() {}"}))
    );
}

#[test]
fn tool_call_projection_preserves_rich_content_and_locations() {
    let ui = ToolUiModel::pending(
        ToolUiKind::FileEdit,
        "Edit file",
        "test.txt",
        serde_json::json!({"path":"test.txt"}),
    )
    .with_content(vec![serde_json::json!({
        "type": "diff",
        "path": "test.txt",
        "oldText": "before",
        "newText": "after"
    })])
    .with_locations(vec![serde_json::json!({"path":"/tmp/test.txt","line":2})]);

    let call = tool_call_from_ui("turn_1/tool_1", &ui);
    assert_eq!(call.content.len(), 1);
    assert_eq!(call.locations.len(), 1);
    assert!(format!("{:?}", call.content[0]).contains("Diff"));
    assert_eq!(
        call.locations[0].path,
        std::path::PathBuf::from("/tmp/test.txt")
    );
}
