use std::path::Path;

use serde_json::json;

use super::{bounded_raw_input, result_update, ToolInfo};

fn text_of(value: &serde_json::Value) -> String {
    value
        .get("text")
        .and_then(|text| text.as_str())
        .unwrap_or_default()
        .to_owned()
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
    assert_eq!(update.status, "completed");
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
        assert_eq!(info.content.first().and_then(|v| v.get("type")).and_then(|v| v.as_str()), Some("text"), "missing card for {name}");
    }
}

#[test]
fn rich_file_edit_content_is_semantic_not_acp_typed() {
    let info = ToolInfo::build(
        "file_edit",
        &json!({"path":"test.txt","old_string":"before","new_string":"after"}),
        Path::new("/tmp"),
        None,
    );
    let diff = info
        .content
        .iter()
        .find(|value| value.get("type").and_then(|v| v.as_str()) == Some("diff"))
        .expect("missing semantic diff");
    assert_eq!(diff.get("path").and_then(|v| v.as_str()), Some("/tmp/test.txt"));
    assert_eq!(diff.get("newText").and_then(|v| v.as_str()), Some("after"));
}

#[test]
fn bounded_raw_input_keeps_small_content_unchanged() {
    let args = json!({"content":"hello"});
    assert_eq!(bounded_raw_input(&args), args);
}

#[test]
fn bounded_raw_input_truncates_large_content_without_acp_wording() {
    let content = "x".repeat(8193);
    let args = json!({"content": content});
    let bounded = bounded_raw_input(&args);
    let rendered = bounded.get("content").and_then(|v| v.as_str()).unwrap();
    assert!(rendered.contains("chars omitted from semantic display"));
    assert!(!rendered.contains("ACP display"));
}
