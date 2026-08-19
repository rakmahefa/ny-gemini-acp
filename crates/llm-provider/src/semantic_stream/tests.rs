use agent_runtime::ModelEvent;
use serde_json::json;

use super::GeminiSemanticStream;

fn collect(chunks: &[&str]) -> Vec<ModelEvent> {
    let mut stream = GeminiSemanticStream::new(true);
    let mut out = Vec::new();
    for chunk in chunks {
        out.extend(stream.feed(chunk));
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
            "call\n{\"id\":\"c1\",\"name\":\"shell_exec\",\"arguments\":{}}\n```\n",
        ]),
        vec![ModelEvent::ToolCall {
            id: "c1".into(),
            name: "shell_exec".into(),
            arguments: json!({}),
        }]
    );
}

#[test]
fn detects_function_call_fence() {
    assert_eq!(
        collect(&[
            "```function_",
            "call\n{\"name\":\"search\",\"args\":{\"q\":\"rust\"}}\n```\n",
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
            "call\n{\"id\":\"c1\",\"name\":\"shell_exec\",\"arguments\":{}}\n'''\n",
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
    assert_eq!(
        collect(&[
            "[Tool result for shell_exec]: ```tool_call\n{\"name\":\"shell_exec\",\"arguments\":{}}\n```\nanswer\n",
        ]),
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
    let mut events = stream.feed("<thinking>hidden</thinking>answer");
    events.extend(stream.finish());
    assert_eq!(
        events,
        vec![ModelEvent::TextDelta("<thinking>hidden</thinking>answer".into())]
    );
}

#[test]
fn finish_is_idempotent() {
    let mut stream = GeminiSemanticStream::new(true);
    assert_eq!(stream.feed("answer"), vec![ModelEvent::TextDelta("answer".into())]);
    assert!(stream.finish().is_empty());
    assert!(stream.finish().is_empty());
    assert!(stream.feed("late").is_empty());
}
