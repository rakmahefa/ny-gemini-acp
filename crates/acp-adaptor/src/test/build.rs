use super::*;
use agent_runtime::state::{History, HistoryEntry, Role};
use tools_provider::tools::registry::ToolRegistry;
use tools_provider::DefaultToolProvider;

fn session(messages: &[(&str, &str)]) -> Session {
    let mut s = Session::new(
        "sess_test".into(),
        "/home/dev/projet".into(),
        vec![],
        "test-model",
    );
    let legacy = messages
        .iter()
        .map(|(role, text)| {
            (
                if *role == "u" {
                    Role::User
                } else {
                    Role::Assistant
                },
                (*text).to_string(),
            )
        })
        .collect::<Vec<_>>();
    s.messages = History::from(legacy);
    s
}

#[test]
fn prompt_contient_systeme_et_tour_courant() {
    let s = session(&[("u", "Question 1"), ("a", "Réponse 1"), ("u", "Question 2")]);
    let p = build_prompt(&s, None);
    assert!(p.contains("[System instruction]"));
    assert!(p.contains("CWD: /home/dev/projet"));
    assert!(p.contains("[User]: Question 2"));
    assert!(p.contains("[Assistant]: Réponse 1"));
}

#[test]
fn tool_result_est_injecte_sans_double_enveloppe() {
    let mut s = Session::new("s".into(), "/tmp".into(), vec![], "m");
    s.messages.push((Role::User, "Lis Cargo.toml".into()));
    s.messages.push((
        Role::Assistant,
        "```tool_call\n{\"name\":\"file_read\",\"arguments\":{}}\n```".into(),
    ));
    s.messages.push((
        Role::Tool,
        "[Tool result for file_read]: [workspace]\nmembers = [\"crates/llm-provider\"]".into(),
    ));

    let p = build_prompt(&s, None);
    assert!(p.contains("[Tool result for file_read]: [workspace]"));
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
    assert!(p.contains("[tool_call shell_exec id=call-1]"));
    assert!(p.contains("[tool_result shell_exec id=call-1 status=ok] all green"));
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

#[test]
fn troncature_32k_garde_le_tour_courant() {
    let mut msgs = vec![(Role::User, "🚀 premier message très long ".repeat(3_000))];
    for i in 0..4 {
        msgs.push((
            Role::Assistant,
            format!("réponse {i} ") + &"x".repeat(9_000),
        ));
        msgs.push((Role::User, format!("question {i} ") + &"y".repeat(9_000)));
    }
    let mut s = session(&[]);
    s.messages = History::from(msgs);
    let p = build_prompt(&s, None);
    assert!(
        p.chars().count() <= 32_000 + 500,
        "budget dépassé: {}",
        p.chars().count()
    );
    assert!(p.contains("question 3"));
}

#[test]
fn build_prompt_vide_renvoie_juste_systeme() {
    let s = Session::new("s".into(), "/tmp".into(), vec![], "m");
    let p = build_prompt(&s, None);
    assert!(p.contains("[System instruction]"));
    assert!(!p.contains("[User]"));
}

#[test]
fn build_prompt_avec_tools_injecte_section() {
    let s = session(&[]);
    let empty = DefaultToolProvider::new(ToolRegistry::new());
    let p = build_prompt(&s, Some(&empty));
    assert!(!p.contains("# Tool Use"));

    let builtin = DefaultToolProvider::new(ToolRegistry::builtin());
    let p = build_prompt(&s, Some(&builtin));
    assert!(p.contains("# Tool Use"));
    assert!(p.contains("file_read"));
    assert!(p.contains("shell_exec"));
}

#[test]
fn build_prompt_tools_disabled_pas_de_section() {
    let mut s = session(&[]);
    s.tools_enabled = false;
    let builtin = DefaultToolProvider::new(ToolRegistry::builtin());
    let p = build_prompt(&s, Some(&builtin));
    assert!(!p.contains("# Tool Use"));
}
