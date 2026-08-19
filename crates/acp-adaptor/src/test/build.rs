    assert!(!p.contains("[Tool result]: [Tool result for file_read]"));
}

#[test]
fn structured_tool_entries_are_rendered_without_reencoding() {
    let mut s = Session::new("s".into(), "/tmp".into(), vec![], "m");
    s.messages.push(HistoryEntry::User { content: "run".into() });
    s.messages.push(HistoryEntry::Assistant { content: "I will run it.".into() });
    s.messages.push(HistoryEntry::ToolCall {
        id: "call-1".into(),
        name: "shell_exec".into(),
        arguments: serde_json::json!({"command": "cargo test"}),
    });
    s.messages.push(HistoryEntry::ToolResult {
        id: "call-1".into(),
        name: "shell_exec".into(),
        content: "all green".into(),
        is_ok: true,
    });

    let p = build_prompt(&s, None);
    assert!(p.contains("```tool_call\n"));
    assert!(p.contains("\"id\":\"call-1\""));
    assert!(p.contains("\"name\":\"shell_exec\""));
    assert!(p.contains("\"arguments\":{\"command\":\"cargo test\"}"));
    assert!(p.contains("[Tool result]:"));
    assert!(p.contains("\"tool\":\"shell_exec\""));
    assert!(p.contains("\"id\":\"call-1\""));
    assert!(p.contains("\"status\":\"ok\""));
    assert!(p.contains("\"content\":\"all green\""));
    assert!(!p.contains("[tool_call shell_exec id=call-1]"));
    assert!(!p.contains("[tool_result shell_exec id=call-1 status=ok] all green"));
}

#[test]
fn fenetre_glissante_12_max() {
    let mut s = session(&[]);
    for i in 0..40 {
        s.messages.push((Role::User, format!("Question {i}")));
    }
    let p = build_prompt(&s, None);
    assert!(p.contains("Question 39"));
    assert!(!p.contains("Question 0"));
    assert!(p.matches("[User]").count() <= 12);
}