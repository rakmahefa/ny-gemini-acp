use super::super::lifecycle::{ToolLifecycle, ToolLifecycleState};
use super::super::registry::ToolResult as RegistryToolResult;
use super::ToolResult;
use agent_client_protocol::schema::v1::StopReason;
use serde_json::{json, Map, Value};

pub(super) fn registry_result(result: RegistryToolResult) -> ToolResult {
    match result {
        RegistryToolResult::Ok(content) => ToolResult {
            content,
            is_ok: true,
            executed: true,
        },
        RegistryToolResult::Err(content) => ToolResult {
            content,
            is_ok: false,
            executed: true,
        },
    }
}
pub(super) fn lifecycle_meta(
    tool_name: &str,
    lifecycle: &ToolLifecycle,
    non_execution_kind: Option<&str>,
    terminal_meta: Option<Map<String, Value>>,
) -> Map<String, Value> {
    let mut meta = terminal_meta.unwrap_or_default();
    meta.insert("geminiAcp".into(),json!({"lifecycle":{"state":lifecycle_state_label(lifecycle.state()),"sequence":lifecycle.sequence()}}));
    let claude = meta.entry("claudeCode").or_insert_with(|| json!({}));
    if let Some(object) = claude.as_object_mut() {
        if !tool_name.is_empty() {
            object.insert("toolName".into(), Value::String(tool_name.to_owned()));
        }
        if let Some(reason) = non_execution_kind {
            object.insert("nonExecutionKind".into(), Value::String(reason.to_owned()));
        }
    }
    meta
}
fn lifecycle_state_label(state: ToolLifecycleState) -> &'static str {
    match state {
        ToolLifecycleState::Pending => "pending",
        ToolLifecycleState::Permission => "permission",
        ToolLifecycleState::Executing => "executing",
        ToolLifecycleState::Completed => "completed",
        ToolLifecycleState::Failed => "failed",
        ToolLifecycleState::Cancelled => "cancelled",
    }
}
#[allow(dead_code)]
pub fn map_stop_reason(gemini_finish: Option<&str>) -> StopReason {
    match gemini_finish {
        Some("length") | Some("max_tokens") => StopReason::MaxTokens,
        Some("content_filter") | Some("safety") | Some("block_reason") => StopReason::Refusal,
        _ => StopReason::EndTurn,
    }
}
