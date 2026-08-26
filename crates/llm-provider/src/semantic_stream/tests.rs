use agent_runtime::ModelEvent;
use serde_json::json;

use super::GeminiSemanticStream;
use crate::core::frames::GeminiFrameEvent;

fn collect(chunks: &[&str]) -> Vec<ModelEvent> {
    let mut stream = GeminiSemanticStream::new(true);
    let mut out = Vec::new();
    for chunk in chunks {
        out.extend(stream.feed_text(chunk));
    }
    out.extend(stream.finish());
    out
}

#[test]
fn reasoning_envelope_becomes_semantic_events() {
    assert_eq!(
        collect(&["<thinking>raisonnement", " utile</thinking>", "réponse"]),
        vec![
            ModelEvent::ReasoningDelta("raisonnement".into()),
            ModelEvent::ReasoningDelta(" utile".into()),
            ModelEvent::TextDelta("réponse".into()),
        ]
    );
}

#[test]
fn reasoning_marker_split_across_chunks_is_atomic() {
    assert_eq!(
        collect(&["<thi", "nking>pensée</thinking>réponse"]),
        vec![
            ModelEvent::ReasoningDelta("pensée".into()),
            ModelEvent::TextDelta("réponse".into()),
        ]
    );
}

#[test]
fn detects_tool_call_incrementally() {
    assert_eq!(
        collect(&[
            "```tool_",
            "call\n{\"id\":\"c1\",\"name\":\"shell_exec\",\"arguments\":{}}\n```\n"
        ]),
        vec![ModelEvent::ToolCall {
            id: "c1".into(),
            name: "shell_exec".into(),
            arguments: json!({}),
        }]
    );
}

#[test]
fn exact_multi_chunk_tool_call_never_becomes_assistant_text() {
    let events = collect(&[
        "Je recherche le terme `modifié` dans le fichier `test_tool.txt`.\n\n",
        "```tool_call\n{\"name\":\"search\",\"id\":\"test_5_search\",\"arguments\":{\"path\":\"test_tool.txt\",\"pattern\":\"modifié\"}}\n```\n\n",
        "La recherche avec `search` a bien fonctionné.\n",
    ]);

    assert_eq!(
        events,
        vec![
            ModelEvent::TextDelta(
                "Je recherche le terme `modifié` dans le fichier `test_tool.txt`.\n\n".into()
            ),
            ModelEvent::ToolCall {
                id: "test_5_search".into(),
                name: "search".into(),
                arguments: json!({"path": "test_tool.txt", "pattern": "modifié"}),
            },
            ModelEvent::TextDelta("La recherche avec `search` a bien fonctionné.\n".into()),
        ]
    );

    assert!(events.iter().all(|event| match event {
        ModelEvent::TextDelta(text) =>
            !text.contains("```tool_call") && !text.contains("test_5_search"),
        ModelEvent::ReasoningDelta(text) =>
            !text.contains("```tool_call") && !text.contains("test_5_search"),
        _ => true,
    }));
}

#[test]
fn detects_inline_tool_call_incrementally() {
    assert_eq!(
        collect(&[
            "avant\n[tool_call shell_exec id=gemini_call_0] {\"command\":\"pwd\"}",
            "\n",
        ]),
        vec![
            ModelEvent::TextDelta("avant\n".into()),
            ModelEvent::ToolCall {
                id: "gemini_call_0".into(),
                name: "shell_exec".into(),
                arguments: json!({"command": "pwd"})
            },
        ]
    );
}

#[test]
fn inline_tool_call_marker_is_not_emitted_as_assistant_text() {
    assert_eq!(
        collect(&["[tool_call file_write id=c1] {\"path\":\"a.txt\",\"content\":\"x\"}\nanswer"]),
        vec![
            ModelEvent::ToolCall {
                id: "c1".into(),
                name: "file_write".into(),
                arguments: json!({"path":"a.txt","content":"x"})
            },
            ModelEvent::TextDelta("answer".into()),
        ]
    );
}

#[test]
fn detects_function_call_fence() {
    assert_eq!(
        collect(&[
            "```function_",
            "call\n{\"name\":\"search\",\"args\":{\"q\":\"rust\"}}\n```\n"
        ]),
        vec![ModelEvent::ToolCall {
            id: "gemini_call_0".into(),
            name: "search".into(),
            arguments: json!({"q": "rust"}),
        }]
    );
}

#[test]
fn detects_single_quote_tool_call_fence() {
    assert_eq!(
        collect(&[
            "'''tool_",
            "call\n{\"id\":\"c1\",\"name\":\"shell_exec\",\"arguments\":{}}\n'''\n"
        ]),
        vec![ModelEvent::ToolCall {
            id: "c1".into(),
            name: "shell_exec".into(),
            arguments: json!({}),
        }]
    );
}

#[test]
fn detects_follow_up_incrementally() {
    assert_eq!(
        collect(&["<FollowUp label=\"Run\" ", "query=\"cargo test\" />"]),
        vec![ModelEvent::ToolCall {
            id: "gemini_call_0".into(),
            name: "FollowUp".into(),
            arguments: json!({"label": "Run", "query": "cargo test"}),
        }]
    );
}

#[test]
fn ignores_tool_result_payload() {
    assert_eq!(collect(&["[Tool result for shell_exec]: ```tool_call\n{\"name\":\"shell_exec\",\"arguments\":{}}\n```\nanswer\n"]), vec![ModelEvent::TextDelta("answer\n".into())]);
}

#[test]
fn ignores_inline_tool_result_payload() {
    assert_eq!(
        collect(&["[tool_result file_write status=ok] [tool_call shell_exec id=bad] {}\nanswer\n"]),
        vec![ModelEvent::TextDelta("answer\n".into())]
    );
}

#[test]
fn ignores_tool_result_payload_when_split_across_chunks() {
    assert_eq!(
        collect(&[
            "[Tool result for shell_exec]: ```tool_",
            "call\n{\"name\":\"shell_exec\",\"arguments\":{}}\n",
            "```\nSuite\n",
        ]),
        vec![ModelEvent::TextDelta("Suite\n".into())]
    );
}

#[test]
fn assistant_marker_is_normalized_without_corrupting_leading_text() {
    assert_eq!(
        collect(&["[Assistant]: réponse"]),
        vec![ModelEvent::TextDelta("réponse".into())]
    );
    assert_eq!(
        collect(&["préfixe [Assistant]: réponse"]),
        vec![ModelEvent::TextDelta("préfixe [Assistant]: réponse".into())]
    );
}

#[test]
fn non_reasoning_models_pass_through_reasoning_markers() {
    let mut stream = GeminiSemanticStream::new(false);
    let mut events = stream.feed_text("<thinking>hidden</thinking>answer");
    events.extend(stream.finish());
    assert_eq!(
        events,
        vec![ModelEvent::TextDelta(
            "<thinking>hidden</thinking>answer".into()
        )]
    );
}

#[test]
fn structured_duplicate_tool_call_is_suppressed_at_semantic_boundary() {
    let mut stream = GeminiSemanticStream::new(true);
    let frame = || GeminiFrameEvent::ToolCall {
        id: "c1".into(),
        name: "glob".into(),
        arguments: json!({"pattern": "*"}),
    };

    assert_eq!(
        stream.feed(frame()),
        vec![ModelEvent::ToolCall {
            id: "c1".into(),
            name: "glob".into(),
            arguments: json!({"pattern": "*"}),
        }]
    );
    assert!(stream.feed(frame()).is_empty());
}

#[test]
fn malformed_semantic_tool_call_is_rejected() {
    let mut stream = GeminiSemanticStream::new(true);
    assert!(stream
        .feed(GeminiFrameEvent::ToolCall {
            id: "   ".into(),
            name: "glob".into(),
            arguments: json!({}),
        })
        .is_empty());
    assert!(stream
        .feed(GeminiFrameEvent::ToolCall {
            id: "c2".into(),
            name: "glob".into(),
            arguments: json!("not-an-object"),
        })
        .is_empty());
}

#[test]
fn finish_is_idempotent() {
    let mut stream = GeminiSemanticStream::new(true);
    assert_eq!(
        stream.feed_text("answer"),
        vec![ModelEvent::TextDelta("answer".into())]
    );
    assert!(stream.finish().is_empty());
    assert!(stream.finish().is_empty());
    assert!(stream.feed_text("late").is_empty());
}
