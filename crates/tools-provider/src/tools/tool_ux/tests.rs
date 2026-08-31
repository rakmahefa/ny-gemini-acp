use std::path::Path;

use agent_runtime::ToolUiKind;
use serde_json::{json, Value};

use super::{bounded_raw_input, result_update, ToolInfo};

fn text_of(content: &Value) -> &str {
    content.get("text").and_then(Value::as_str).unwrap()
}

#[test]
fn follow_up_has_a_dedicated_card() {
    let args = json!({"label":"Initialiser un nouveau projet","query":"Initialisons un nouveau projet dans cet espace de travail."});
    let info = ToolInfo::build("FollowUp", &args, Path::new("/tmp"), None);
    let rendered = text_of(&info.content[0]);
    assert!(rendered.contains("Follow-up"));
    assert!(rendered.contains("Initialiser un nouveau projet"));
    assert!(rendered.contains("Input") || rendered.contains("Content"));
}

#[test]
fn follow_up_completion_keeps_label_and_query_in_card() {
    let args = json!({"label":"Initialiser","query":"Initialisons le projet."});
    let update = result_update(
        "FollowUp",
        &args,
        r#"{"label":"Initialiser","query":"Initialisons le projet."}"#,
        true,
        Path::new("/tmp"),
        None,
    );
    assert_eq!(update.content.len(), 1);
    let rendered = text_of(&update.content[0]);
    assert!(rendered.contains("completed"));
    assert!(rendered.contains("Initialiser"));
    assert!(rendered.contains("Initialisons le projet"));
    assert_eq!(update.status, agent_runtime::ToolUiStatus::Succeeded);
}

#[test]
fn core_tools_keep_one_text_card() {
    let cwd = Path::new("/tmp/project");
    for (name, args) in [
        ("file_read", json!({"path":"src/lib.rs"})),
        ("glob", json!({"pattern":"**/*.rs"})),
        ("list_directory", json!({"path":"src"})),
        ("search", json!({"pattern":"foo"})),
        ("shell_exec", json!({"command":"cargo test"})),
        ("AskUserQuestion", json!({"questions":[{"question":"Continue?","options":[{"label":"Yes"}]}]})),
    ] {
        let info = ToolInfo::build(name, &args, cwd, None);
        assert_eq!(info.content.first().and_then(|v| v.get("type")).and_then(Value::as_str), Some("content"), "missing card for {name}");
    }
}

#[test]
fn rich_presentation_remains_structured_and_host_neutral() {
    let cwd = Path::new("/tmp/project");
    let edit = ToolInfo::build(
        "file_edit",
        &json!({"path":"src/lib.rs","old_string":"before","new_string":"after"}),
        cwd,
        None,
    );
    assert_eq!(edit.kind, ToolUiKind::FileEdit);
    assert!(edit.content.iter().any(|v| v.get("type").and_then(Value::as_str) == Some("diff")));
    assert_eq!(edit.locations.len(), 1);
    assert_eq!(edit.locations[0].get("path").and_then(Value::as_str), Some("/tmp/project/src/lib.rs"));

    let shell = ToolInfo::build("shell_exec", &json!({"command":"pwd"}), cwd, Some("term-7"));
    assert_eq!(shell.kind, ToolUiKind::Shell);
    assert!(shell.content.iter().any(|v| {
        v.get("type").and_then(Value::as_str) == Some("terminal")
            && v.get("id").and_then(Value::as_str) == Some("term-7")
    }));

    let read = ToolInfo::build("file_read", &json!({"path":"src/lib.rs","offset":7}), cwd, None);
    assert_eq!(read.kind, ToolUiKind::FileRead);
    assert_eq!(read.locations[0].get("line").and_then(Value::as_u64), Some(7));
}

#[test]
fn bounded_raw_input_keeps_small_content_unchanged() {
    let args = json!({"content":"hello"});
    assert_eq!(bounded_raw_input(&args), args);
}

#[test]
fn bounded_raw_input_truncates_large_content() {
    let content = "x".repeat(8193);
    let args = json!({"content": content});
    let bounded = bounded_raw_input(&args);
    let rendered = bounded.get("content").and_then(|v| v.as_str()).unwrap();
    assert!(rendered.contains("chars omitted from display"));
}
