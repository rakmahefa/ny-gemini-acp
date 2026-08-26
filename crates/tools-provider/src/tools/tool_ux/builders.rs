use std::path::Path;

use agent_client_protocol::schema::v1::{Diff, ToolCallContent, ToolCallLocation, ToolKind};
use serde_json::Value;

use super::display::{concise_args, truncate, ux_card};
use super::results::{display_path, read_old_text, resolve_path};
use super::types::{CardBodyKind, ToolInfo};

impl ToolInfo {
    pub fn build(name: &str, args: &Value, cwd: &Path, terminal_id: Option<&str>) -> Self {
        match name {
            "file_read" => file_read(args, cwd),
            "file_write" => file_write(args, cwd),
            "file_edit" | "replace_in_file" => file_edit(args, cwd),
            "glob" => glob(args, cwd),
            "list_directory" => list_directory(args, cwd),
            "search" => search(args, cwd),
            "search_and_read" => search_and_read(args, cwd),
            "shell_exec" => shell_exec(args, terminal_id),
            "AskUserQuestion" => ask_user_question(args),
            "FollowUp" => follow_up(args),
            _ => generic(name, args),
        }
    }
}

fn file_read(args: &Value, cwd: &Path) -> ToolInfo {
    let path = arg_str(args, "path").unwrap_or("File");
    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(1).max(1);
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(500).max(1);
    let input = format!("{}  ·  lignes {}-{}", display_path(path, cwd), offset, offset + limit - 1);
    ToolInfo {
        title: format!("Read {} ({}-{})", display_path(path, cwd), offset, offset + limit - 1),
        kind: ToolKind::Read,
        content: vec![ux_card("file_read", "⏳ pending", args, Some((&input, CardBodyKind::Input, false)), None)],
        locations: vec![ToolCallLocation::new(resolve_path(path, cwd)).line(offset as u32)],
    }
}

fn file_write(args: &Value, cwd: &Path) -> ToolInfo {
    let path = arg_str(args, "path").unwrap_or("File");
    let content = arg_str(args, "content").unwrap_or("");
    let resolved = resolve_path(path, cwd);
    let diff = Diff::new(resolved.clone(), content.to_owned()).old_text(read_old_text(&resolved));
    let input = format!("{}  ·  {} chars", display_path(path, cwd), content.chars().count());
    ToolInfo {
        title: format!("Write {}", display_path(path, cwd)),
        kind: ToolKind::Edit,
        content: vec![
            ux_card("file_write", "⏳ pending", args, Some((&input, CardBodyKind::Input, false)), None),
            ToolCallContent::Diff(diff),
        ],
        locations: vec![ToolCallLocation::new(resolved)],
    }
}

fn file_edit(args: &Value, cwd: &Path) -> ToolInfo {
    let path = arg_str(args, "path").unwrap_or("File");
    let old = arg_str(args, "old_string").unwrap_or("");
    let new = arg_str(args, "new_string").unwrap_or("");
    let resolved = resolve_path(path, cwd);
    let old_text = if old.is_empty() { read_old_text(&resolved) } else { Some(old.to_owned()) };
    let diff = Diff::new(resolved.clone(), new.to_owned()).old_text(old_text);
    let input = format!("{}  ·  replacement {} → {} chars", display_path(path, cwd), old.chars().count(), new.chars().count());
    ToolInfo {
        title: format!("Edit {}", display_path(path, cwd)),
        kind: ToolKind::Edit,
        content: vec![
            ux_card("file_edit", "⏳ pending", args, Some((&input, CardBodyKind::Input, false)), None),
            ToolCallContent::Diff(diff),
        ],
        locations: vec![ToolCallLocation::new(resolved)],
    }
}

fn glob(args: &Value, cwd: &Path) -> ToolInfo {
    let pattern = arg_str(args, "pattern").unwrap_or("");
    let path = arg_str(args, "path").unwrap_or(".");
    let max_results = args.get("max_results").and_then(Value::as_u64).unwrap_or(100);
    let input = format!("pattern `{}`  ·  path {}  ·  max {}", truncate(pattern, 72), display_path(path, cwd), max_results);
    ToolInfo {
        title: format!("Find paths `{}`", truncate(pattern, 72)),
        kind: ToolKind::Search,
        content: vec![ux_card("glob", "⏳ pending", args, Some((&input, CardBodyKind::Input, false)), None)],
        locations: vec![ToolCallLocation::new(resolve_path(path, cwd))],
    }
}

fn list_directory(args: &Value, cwd: &Path) -> ToolInfo {
    let path = arg_str(args, "path").unwrap_or(".");
    let input = format!("path {}", display_path(path, cwd));
    ToolInfo {
        title: format!("List {}", display_path(path, cwd)),
        kind: ToolKind::Read,
        content: vec![ux_card("list_directory", "⏳ pending", args, Some((&input, CardBodyKind::Input, false)), None)],
        locations: vec![ToolCallLocation::new(resolve_path(path, cwd))],
    }
}

fn search(args: &Value, cwd: &Path) -> ToolInfo {
    let pattern = arg_str(args, "pattern").unwrap_or("");
    let path = arg_str(args, "path").unwrap_or(".");
    let input = if path == "." {
        format!("pattern `{}`", truncate(pattern, 72))
    } else {
        format!("pattern `{}`  ·  path {}", truncate(pattern, 56), display_path(path, cwd))
    };
    ToolInfo {
        title: if path == "." { format!("Find `{}`", truncate(pattern, 72)) } else { format!("Find `{}` in {}", truncate(pattern, 56), display_path(path, cwd)) },
        kind: ToolKind::Search,
        content: vec![ux_card("search", "⏳ pending", args, Some((&input, CardBodyKind::Input, false)), None)],
        locations: vec![ToolCallLocation::new(resolve_path(path, cwd))],
    }
}

fn search_and_read(args: &Value, cwd: &Path) -> ToolInfo {
    let pattern = arg_str(args, "pattern").unwrap_or("");
    let path = arg_str(args, "path").unwrap_or(".");
    let context = args.get("context").and_then(Value::as_u64).unwrap_or(0);
    let input = if path == "." {
        format!("pattern `{}`  ·  context ±{}", truncate(pattern, 56), context)
    } else {
        format!("pattern `{}`  ·  path {}  ·  context ±{}", truncate(pattern, 40), display_path(path, cwd), context)
    };
    ToolInfo {
        title: if path == "." { format!("Find excerpts for `{}`", truncate(pattern, 56)) } else { format!("Find excerpts for `{}` in {}", truncate(pattern, 40), display_path(path, cwd)) },
        kind: ToolKind::Search,
        content: vec![ux_card("search_and_read", "⏳ pending", args, Some((&input, CardBodyKind::Input, false)), None)],
        locations: vec![ToolCallLocation::new(resolve_path(path, cwd))],
    }
}

fn shell_exec(args: &Value, terminal_id: Option<&str>) -> ToolInfo {
    let command = arg_str(args, "command").unwrap_or("");
    let mut content = vec![ux_card("shell_exec", "⏳ pending", args, Some((command, CardBodyKind::Input, false)), terminal_id)];
    if let Some(id) = terminal_id {
        content.push(ToolCallContent::Terminal(agent_client_protocol::schema::v1::Terminal::new(id.to_owned())));
    }
    ToolInfo {
        title: if command.is_empty() { "Terminal".into() } else { truncate(command, 96) },
        kind: ToolKind::Execute,
        content,
        locations: vec![],
    }
}

fn ask_user_question(args: &Value) -> ToolInfo {
    let title = ask_user_title(args);
    let body = render_ask_user_input(args);
    ToolInfo {
        title,
        kind: ToolKind::Other,
        content: vec![ux_card("AskUserQuestion", "⏳ waiting for user", args, Some((&body, CardBodyKind::Content, false)), None)],
        locations: vec![],
    }
}

fn follow_up(args: &Value) -> ToolInfo {
    let label = arg_str(args, "label").unwrap_or("Suggested next step");
    let query = arg_str(args, "query").unwrap_or("");
    let input = format!("{label}\n→ {query}");
    ToolInfo {
        title: format!("Follow-up · {}", truncate(label, 72)),
        kind: ToolKind::Other,
        content: vec![ux_card("FollowUp", "⏳ pending", args, Some((&input, CardBodyKind::Content, false)), None)],
        locations: vec![],
    }
}

fn generic(name: &str, args: &Value) -> ToolInfo {
    let body = if args.as_object().is_none_or(|obj| obj.is_empty()) { "No input payload.".to_owned() } else { concise_args(args) };
    ToolInfo {
        title: name.to_owned(),
        kind: ToolKind::Other,
        content: vec![ux_card(name, "⏳ pending", args, Some((&body, CardBodyKind::Input, false)), None)],
        locations: vec![],
    }
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn render_ask_user_input(args: &Value) -> String {
    let Some(questions) = args.get("questions").and_then(Value::as_array) else { return "Question indisponible.".into(); };
    let mut output = String::new();
    for (index, question) in questions.iter().enumerate() {
        if index > 0 { output.push_str("\n\n"); }
        let header = question.get("header").and_then(Value::as_str).unwrap_or("Question");
        let prompt = question.get("question").and_then(Value::as_str).unwrap_or("Question indisponible.");
        output.push_str(&format!("{header}\n{prompt}"));
        if let Some(options) = question.get("options").and_then(Value::as_array) {
            for option in options {
                if let Some(label) = option.get("label").and_then(Value::as_str) { output.push_str(&format!("\n- {label}")); }
            }
        }
    }
    truncate(&output, super::types::MAX_QUESTION_PREVIEW_CHARS)
}

fn ask_user_title(args: &Value) -> String {
    let question = args.get("questions").and_then(Value::as_array).and_then(|questions| questions.first()).and_then(|question| question.get("question")).and_then(Value::as_str).unwrap_or("User input");
    format!("Ask user · {}", truncate(question, 72))
}
