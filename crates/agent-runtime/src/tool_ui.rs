use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Host-neutral semantic category for tool presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolUiKind {
    FileRead,
    FileWrite,
    FileEdit,
    Search,
    Glob,
    DirectoryList,
    Shell,
    SearchAndRead,
    ReplaceInFile,
    AskUserQuestion,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolUiStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolUiModel {
    /// Semantic category; a host may map this to an icon or visual component.
    pub kind: ToolUiKind,
    /// Human-readable title suitable for a compact tool card.
    pub title: String,
    /// Short, user-facing summary. Keep this stable and non-technical.
    pub summary: String,
    /// Lifecycle status of the represented action.
    pub status: ToolUiStatus,
    /// Small structured input facts used by a host to render the invocation.
    pub input: Value,
    /// Optional structured output facts. Raw tool content remains separate.
    pub output: Option<Value>,
    /// Whether verbose details should normally be collapsed by the host.
    pub expandable: bool,
}

impl ToolUiModel {
    pub fn pending(
        kind: ToolUiKind,
        title: impl Into<String>,
        summary: impl Into<String>,
        input: Value,
    ) -> Self {
        Self {
            kind,
            title: title.into(),
            summary: summary.into(),
            status: ToolUiStatus::Pending,
            input,
            output: None,
            expandable: true,
        }
    }

    pub fn completed(mut self, ok: bool, output: Option<Value>) -> Self {
        self.status = if ok {
            ToolUiStatus::Succeeded
        } else {
            ToolUiStatus::Failed
        };
        self.output = output;
        self
    }

    pub fn cancelled(mut self, output: Option<Value>) -> Self {
        self.status = ToolUiStatus::Cancelled;
        self.output = output;
        self
    }

    pub fn running(mut self) -> Self {
        self.status = ToolUiStatus::Running;
        self
    }

    pub fn generic(name: &str, input: Value) -> Self {
        Self::pending(
            ToolUiKind::Generic,
            name.replace('_', " "),
            format!("Run {name}"),
            input,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_is_explicit_and_terminal() {
        let pending = ToolUiModel::generic("shell_exec", serde_json::json!({"command": "cargo test"}));
        assert_eq!(pending.status, ToolUiStatus::Pending);

        let running = pending.clone().running();
        assert_eq!(running.status, ToolUiStatus::Running);
        assert_eq!(running.input["command"], "cargo test");

        let succeeded = running
            .clone()
            .completed(true, Some(serde_json::json!({"text": "ok"})));
        assert_eq!(succeeded.status, ToolUiStatus::Succeeded);
        assert_eq!(succeeded.output.as_ref().unwrap()["text"], "ok");

        let failed = running
            .clone()
            .completed(false, Some(serde_json::json!({"text": "failed"})));
        assert_eq!(failed.status, ToolUiStatus::Failed);

        let cancelled = running.cancelled(Some(serde_json::json!({"text": "cancelled"})));
        assert_eq!(cancelled.status, ToolUiStatus::Cancelled);
    }

    #[test]
    fn primary_surface_stays_separate_from_raw_output() {
        let ui = ToolUiModel::pending(
            ToolUiKind::FileEdit,
            "Edit file",
            "src/main.rs",
            serde_json::json!({"path": "src/main.rs", "replace_all": false}),
        );
        assert_eq!(ui.title, "Edit file");
        assert_eq!(ui.summary, "src/main.rs");
        assert!(ui.output.is_none());
        assert!(ui.expandable);
    }

    #[test]
    fn generic_tool_identity_is_human_readable_without_parsing_runtime_text() {
        let ui = ToolUiModel::generic("list_directory", serde_json::json!({}));
        assert_eq!(ui.kind, ToolUiKind::Generic);
        assert_eq!(ui.title, "list directory");
        assert_eq!(ui.summary, "Run list_directory");
    }
}
