use agent_runtime::{ToolUiKind, ToolUiModel};
use serde_json::{json, Value};

fn text_output(content: &str) -> Value {
    json!({ "text": content })
}

fn input_path(args: &Value) -> Option<&str> {
    args.get("path").and_then(Value::as_str).filter(|v| !v.trim().is_empty())
}

fn model(
    kind: ToolUiKind,
    title: impl Into<String>,
    summary: impl Into<String>,
    input: Value,
    content: &str,
    ok: bool,
) -> ToolUiModel {
    ToolUiModel::pending(kind, title, summary, input).completed(ok, Some(text_output(content)))
}

/// One host-neutral UI model per builtin tool.
///
/// This intentionally contains no ACP schema types, no markdown rendering, and
/// no client-specific widget assumptions. A host can map the semantic category
/// and structured facts to the visual component it supports.
pub fn completed(name: &str, args: &Value, content: &str, ok: bool) -> ToolUiModel {
    match name {
        "file_read" => model(
            ToolUiKind::FileRead,
            "Read file",
            input_path(args).map(|p| p.to_owned()).unwrap_or_else(|| "Read a file".into()),
            json!({ "path": input_path(args), "offset": args.get("offset").cloned().unwrap_or(json!(1)), "limit": args.get("limit").cloned().unwrap_or(json!(500)) }),
            content,
            ok,
        ),
        "file_write" => model(ToolUiKind::FileWrite, "Write file", input_path(args).map(|p| p.to_owned()).unwrap_or_else(|| "Write a file".into()), json!({ "path": input_path(args), "content_chars": args.get("content").and_then(Value::as_str).map(|s| s.chars().count()) }), content, ok),
        "file_edit" => model(ToolUiKind::FileEdit, "Edit file", input_path(args).map(|p| p.to_owned()).unwrap_or_else(|| "Edit a file".into()), json!({ "path": input_path(args), "replace_all": args.get("replace_all").cloned().unwrap_or(json!(false)) }), content, ok),
        "replace_in_file" => model(ToolUiKind::ReplaceInFile, "Replace in file", input_path(args).map(|p| p.to_owned()).unwrap_or_else(|| "Replace text".into()), json!({ "path": input_path(args), "replace_all": args.get("replace_all").cloned().unwrap_or(json!(false)) }), content, ok),
        "glob" => model(ToolUiKind::Glob, "Find files", args.get("pattern").and_then(Value::as_str).unwrap_or("Find matching files"), json!({ "pattern": args.get("pattern"), "path": input_path(args), "max_results": args.get("max_results").cloned().unwrap_or(json!(100)) }), content, ok),
        "list_directory" => model(ToolUiKind::DirectoryList, "List directory", input_path(args).map(|p| p.to_owned()).unwrap_or_else(|| "Current directory".into()), json!({ "path": input_path(args) }), content, ok),
        "search" => model(ToolUiKind::Search, "Search", args.get("pattern").and_then(Value::as_str).unwrap_or("Search files"), json!({ "pattern": args.get("pattern"), "path": input_path(args), "glob": args.get("glob"), "max_results": args.get("max_results").cloned().unwrap_or(json!(50)) }), content, ok),
        "search_and_read" => model(ToolUiKind::SearchAndRead, "Inspect matches", args.get("pattern").and_then(Value::as_str).unwrap_or("Search and inspect"), json!({ "pattern": args.get("pattern"), "path": input_path(args), "glob": args.get("glob"), "context": args.get("context").cloned().unwrap_or(json!(2)), "max_matches": args.get("max_matches").cloned().unwrap_or(json!(20)) }), content, ok),
        "shell_exec" => model(ToolUiKind::Shell, "Run command", args.get("command").and_then(Value::as_str).unwrap_or("Run shell command"), json!({ "command": args.get("command"), "timeout": args.get("timeout").cloned().unwrap_or(json!(30)) }), content, ok),
        "AskUserQuestion" => model(ToolUiKind::AskUserQuestion, "Question", "Waiting for your answer", json!({ "interactive": true }), content, ok),
        _ => model(ToolUiKind::Generic, name.replace('_', " "), format!("Run {name}"), args.clone(), content, ok),
    }
}

pub fn pending(name: &str, args: &Value) -> ToolUiModel {
    let mut ui = completed(name, args, "", true);
    ui.status = agent_runtime::ToolUiStatus::Pending;
    ui.output = None;
    ui
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_runtime::{ToolUiKind, ToolUiStatus};

    #[test]
    fn each_builtin_has_a_distinct_semantic_kind() {
        let cases = [
            ("file_read", ToolUiKind::FileRead),
            ("file_write", ToolUiKind::FileWrite),
            ("file_edit", ToolUiKind::FileEdit),
            ("glob", ToolUiKind::Glob),
            ("list_directory", ToolUiKind::DirectoryList),
            ("search", ToolUiKind::Search),
            ("search_and_read", ToolUiKind::SearchAndRead),
            ("shell_exec", ToolUiKind::Shell),
            ("replace_in_file", ToolUiKind::ReplaceInFile),
            ("AskUserQuestion", ToolUiKind::AskUserQuestion),
        ];
        for (name, kind) in cases {
            let ui = pending(name, &json!({ "path": "src/main.rs" }));
            assert_eq!(ui.kind, kind);
            assert_eq!(ui.status, ToolUiStatus::Pending);
        }
    }

    #[test]
    fn shell_summary_is_the_command_not_a_runtime_error() {
        let ui = completed("shell_exec", &json!({ "command": "cargo test", "timeout": 30 }), "[exit code 0]", true);
        assert_eq!(ui.title, "Run command");
        assert_eq!(ui.summary, "cargo test");
        assert_eq!(ui.status, ToolUiStatus::Succeeded);
    }

    #[test]
    fn file_edit_input_is_small_and_machine_safe() {
        let ui = completed("file_edit", &json!({ "path": "src/main.rs", "old_string": "huge secret", "new_string": "replacement" }), "Fichier modifié: src/main.rs (1 occurrence)", true);
        assert_eq!(ui.input["path"], "src/main.rs");
        assert!(ui.input.get("old_string").is_none());
    }
}
