use std::collections::HashSet;
use std::sync::Arc;

use crate::events::{
    consume_model_stream, ModelProjectionError, ModelRound, PendingToolCall, TurnEventSink,
};
use crate::state::{Role, Session};
use crate::{
    Cancellation, GenerationOptions, LlmError, LlmProvider, ModelRequest, ToolCallRequest,
    ToolCallResult, ToolPermissionDecision, ToolPermissionHandler, ToolPermissionRequest,
    ToolProvider,
};

const DEFAULT_MAX_ROUNDS: usize = 20;
const DEFAULT_MAX_TOOL_CALLS_PER_ROUND: usize = 32;
const CONTEXT_WINDOW_CHARS: usize = 1_000_000;
const COMPACTION_THRESHOLD_CHARS: usize = CONTEXT_WINDOW_CHARS * 9 / 10;
const EMERGENCY_COMPACTION_CHARS: usize = CONTEXT_WINDOW_CHARS * 7 / 10;
const PRESERVE_TURNS: usize = 10;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum AgentActionError {
    #[error("invalid agent action input: {0}")]
    InvalidInput(String),
    #[error("agent action cancelled")]
    Cancelled,
    #[error("agent action rejected: {0}")]
    Rejected(String),
    #[error("agent action failed: {0}")]
    Failed(String),
}

#[async_trait::async_trait]
pub trait AgentActionHandler: Send + Sync {
    fn supports(&self, name: &str) -> bool;
    async fn handle(
        &self,
        session_id: &str,
        call_id: &str,
        name: &str,
        arguments: serde_json::Value,
        cancellation: Cancellation,
    ) -> Result<Option<String>, AgentActionError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentLoopConfig {
    pub max_rounds: usize,
    pub max_tool_calls_per_round: usize,
}
impl Default for AgentLoopConfig {
    fn default() -> Self {
        Self {
            max_rounds: DEFAULT_MAX_ROUNDS,
            max_tool_calls_per_round: DEFAULT_MAX_TOOL_CALLS_PER_ROUND,
        }
    }
}
impl AgentLoopConfig {
    fn validate(self) -> Result<Self, AgentLoopError> {
        if self.max_rounds == 0 {
            return Err(AgentLoopError::InvalidConfig(
                "max_rounds must be greater than zero".into(),
            ));
        }
        if self.max_tool_calls_per_round == 0 {
            return Err(AgentLoopError::InvalidConfig(
                "max_tool_calls_per_round must be greater than zero".into(),
            ));
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLoopOutcome {
    pub output: String,
    pub rounds: usize,
    pub tool_calls: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentLoopError {
    #[error("invalid agent loop configuration: {0}")]
    InvalidConfig(String),
    #[error("agent loop cancelled")]
    Cancelled,
    #[error("invalid session: {0}")]
    InvalidSession(String),
    #[error("LLM provider failed: {0}")]
    Llm(#[source] LlmError),
    #[error("LLM stream produced no observable events")]
    EmptyStream,
    #[error("LLM stream produced no final text or tool call")]
    NoProgress,
    #[error("agent loop exceeded the maximum of {0} model rounds")]
    MaxRounds(usize),
    #[error("model emitted too many tool calls in one round: {actual} > {limit}")]
    ToolCallLimit { actual: usize, limit: usize },
    #[error("invalid tool call: {0}")]
    InvalidToolCall(String),
    #[error("invalid model event sequence: {0}")]
    InvalidModelSequence(String),
    #[error("semantic event emission was rejected")]
    SemanticEventRejected,
    #[error("agent action failed: {0}")]
    Action(#[source] AgentActionError),
}

/// Provider-neutral model/tool orchestration. Host-specific permission policy is injected separately.
pub struct AgentLoop {
    llm: Arc<dyn LlmProvider>,
    tools: Arc<dyn ToolProvider>,
    config: AgentLoopConfig,
    action_handler: Option<Arc<dyn AgentActionHandler>>,
    permission_handler: Option<Arc<dyn ToolPermissionHandler>>,
}

impl AgentLoop {
    pub fn new(
        llm: Arc<dyn LlmProvider>,
        tools: Arc<dyn ToolProvider>,
        config: AgentLoopConfig,
    ) -> Result<Self, AgentLoopError> {
        Ok(Self {
            llm,
            tools,
            config: config.validate()?,
            action_handler: None,
            permission_handler: None,
        })
    }
    pub fn with_action_handler(mut self, handler: Arc<dyn AgentActionHandler>) -> Self {
        self.action_handler = Some(handler);
        self
    }
    pub fn with_permission_handler(mut self, handler: Arc<dyn ToolPermissionHandler>) -> Self {
        self.permission_handler = Some(handler);
        self
    }
    pub fn config(&self) -> AgentLoopConfig {
        self.config
    }

    pub async fn run<F>(
        &self,
        session: &mut Session,
        references: &[String],
        cancellation: Cancellation,
        sink: &mut dyn TurnEventSink,
        build_prompt: F,
    ) -> Result<AgentLoopOutcome, AgentLoopError>
    where
        F: Fn(&Session, &dyn ToolProvider) -> String,
    {
        validate_session(session)?;
        if cancellation.is_cancelled() {
            let _ = sink.turn_cancelled();
            return Err(AgentLoopError::Cancelled);
        }
        if !sink.turn_started() {
            return Err(AgentLoopError::SemanticEventRejected);
        }
        let tools = self.tools.for_session(&session.id).await;
        let mut seen_tool_ids = HashSet::new();
        let mut total_tool_calls = 0usize;
        let mut overflow_retry = false;

        'rounds: for round in 0..self.config.max_rounds {
            ensure_not_cancelled(&cancellation, sink)?;
            compact_messages(&mut session.messages, COMPACTION_THRESHOLD_CHARS);
            let request = ModelRequest {
                prompt: build_prompt(session, &*tools),
                model: session.model.clone(),
                generation: GenerationOptions {
                    reasoning_budget: session.think,
                },
                references: references.to_vec(),
            };
            let stream = match self
                .llm
                .stream_with_cancellation(request, cancellation.subscribe())
                .await
            {
                Ok(stream) => stream,
                Err(error) => {
                    if matches!(error, LlmError::Cancelled) || cancellation.is_cancelled() {
                        let _ = sink.turn_cancelled();
                        return Err(AgentLoopError::Cancelled);
                    }
                    if is_context_error(&error) && !overflow_retry {
                        compact_messages(&mut session.messages, EMERGENCY_COMPACTION_CHARS);
                        overflow_retry = true;
                        continue 'rounds;
                    }
                    let _ = sink.turn_failed();
                    return Err(AgentLoopError::Llm(error));
                }
            };
            overflow_retry = false;

            let round_result = consume_stream(stream, &cancellation, sink).await?;
            if round_result.event_count == 0 {
                let _ = sink.turn_failed();
                return Err(AgentLoopError::EmptyStream);
            }
            if round_result.tool_calls.len() > self.config.max_tool_calls_per_round {
                let _ = sink.turn_failed();
                return Err(AgentLoopError::ToolCallLimit {
                    actual: round_result.tool_calls.len(),
                    limit: self.config.max_tool_calls_per_round,
                });
            }
            let tool_calls =
                canonicalize_tool_calls(round_result.tool_calls, round, &mut seen_tool_ids)?;

            let mut executable = Vec::new();
            for call in tool_calls {
                if let Some(handler) = &self.action_handler {
                    if handler.supports(&call.name) {
                        ensure_not_cancelled(&cancellation, sink)?;
                        match handler
                            .handle(
                                &session.id,
                                &call.id,
                                &call.name,
                                call.arguments,
                                cancellation.clone(),
                            )
                            .await
                        {
                            Ok(Some(user_text)) => {
                                if !user_text.trim().is_empty() {
                                    session.messages.push_user(user_text);
                                }
                                continue 'rounds;
                            }
                            Ok(None) => continue,
                            Err(AgentActionError::Cancelled) if cancellation.is_cancelled() => {
                                let _ = sink.turn_cancelled();
                                return Err(AgentLoopError::Cancelled);
                            }
                            Err(AgentActionError::Cancelled) => {
                                let _ = sink.turn_cancelled();
                                return Err(AgentLoopError::Cancelled);
                            }
                            Err(error) => {
                                let _ = sink.turn_failed();
                                return Err(AgentLoopError::Action(error));
                            }
                        }
                    }
                }
                executable.push(call);
            }

            if executable.is_empty() {
                if round_result.text.trim().is_empty() {
                    let _ = sink.turn_failed();
                    return Err(AgentLoopError::NoProgress);
                }
                ensure_not_cancelled(&cancellation, sink)?;
                session.messages.push_assistant(round_result.text.clone());
                if !sink.turn_completed() {
                    let _ = sink.turn_failed();
                    return Err(AgentLoopError::SemanticEventRejected);
                }
                return Ok(AgentLoopOutcome {
                    output: round_result.text,
                    rounds: round + 1,
                    tool_calls: total_tool_calls,
                });
            }

            total_tool_calls = total_tool_calls.checked_add(executable.len()).ok_or(
                AgentLoopError::ToolCallLimit {
                    actual: usize::MAX,
                    limit: usize::MAX,
                },
            )?;
            ensure_not_cancelled(&cancellation, sink)?;

            if !round_result.text.trim().is_empty() {
                session.messages.push_assistant(round_result.text.clone());
            }
            for call in &executable {
                session.messages.push_tool_call(
                    call.id.clone(),
                    call.name.clone(),
                    call.arguments.clone(),
                );
            }

            for call in executable {
                ensure_not_cancelled(&cancellation, sink)?;
                let ui = tools.ui_model(&call.id, &call.name, &call.arguments);
                if !sink.tool_call_requested(call.id.clone(), call.name.clone(), ui.clone()) {
                    let _ = sink.turn_failed();
                    return Err(AgentLoopError::SemanticEventRejected);
                }
                let permission_request = ToolPermissionRequest {
                    session_id: session.id.clone(),
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                    cwd: session.cwd.clone(),
                    additional_dirs: session.additional_directories.clone(),
                };
                if let Some(permission_handler) = &self.permission_handler {
                    if permission_handler.needs_permission(session, &permission_request) {
                        if !sink.permission_requested(call.id.clone()) {
                            let _ = sink.turn_failed();
                            return Err(AgentLoopError::SemanticEventRejected);
                        }
                        match permission_handler
                            .request_permission(session, &permission_request, cancellation.clone())
                            .await
                        {
                            ToolPermissionDecision::Allow => {}
                            ToolPermissionDecision::Reject(message) => {
                                let mut result = ToolCallResult::error(message.clone());
                                result.ui = ui.map(|model| {
                                    model.completed(
                                        false,
                                        Some(serde_json::json!({"text": message})),
                                    )
                                });
                                ensure_not_cancelled(&cancellation, sink)?;
                                if !sink.tool_result_received(
                                    call.id.clone(),
                                    result.content.clone(),
                                    result.ui.clone(),
                                ) {
                                    let _ = sink.turn_failed();
                                    return Err(AgentLoopError::SemanticEventRejected);
                                }
                                session.messages.push_tool_result(
                                    call.id.clone(),
                                    call.name.clone(),
                                    result.content.clone(),
                                    result.is_ok,
                                );
                                continue;
                            }
                            ToolPermissionDecision::Cancelled => {
                                let _ = sink.turn_cancelled();
                                return Err(AgentLoopError::Cancelled);
                            }
                        }
                    }
                }
                let running_ui = ui.clone().map(|model| model.running());
                if !sink.tool_execution_started(call.id.clone(), running_ui) {
                    let _ = sink.turn_failed();
                    return Err(AgentLoopError::SemanticEventRejected);
                }
                let result = if session.tools_enabled && tools.has_tools() {
                    tools
                        .call(ToolCallRequest {
                            call_id: call.id.clone(),
                            session_id: session.id.clone(),
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                            cwd: session.cwd.clone(),
                            additional_dirs: session.additional_directories.clone(),
                            cancellation: cancellation.subscribe(),
                        })
                        .await
                } else {
                    let mut result = ToolCallResult::error(format!(
                        "tool execution disabled for session: {}",
                        call.name
                    ));
                    result.ui = ui.map(|model| {
                        model.completed(
                            false,
                            Some(serde_json::json!({"text": result.content.clone()})),
                        )
                    });
                    result
                };
                ensure_not_cancelled(&cancellation, sink)?;
                if !sink.tool_result_received(
                    call.id.clone(),
                    result.content.clone(),
                    result.ui.clone(),
                ) {
                    let _ = sink.turn_failed();
                    return Err(AgentLoopError::SemanticEventRejected);
                }
                session.messages.push_tool_result(
                    call.id.clone(),
                    call.name.clone(),
                    result.content.clone(),
                    result.is_ok,
                );
            }
        }
        let _ = sink.turn_failed();
        Err(AgentLoopError::MaxRounds(self.config.max_rounds))
    }
}

fn validate_session(session: &Session) -> Result<(), AgentLoopError> {
    if session.id.trim().is_empty() {
        return Err(AgentLoopError::InvalidSession(
            "session id must not be empty".into(),
        ));
    }
    if session.model.trim().is_empty() {
        return Err(AgentLoopError::InvalidSession(
            "model must not be empty".into(),
        ));
    }
    Ok(())
}

fn canonicalize_tool_calls(
    mut calls: Vec<PendingToolCall>,
    round: usize,
    seen: &mut HashSet<String>,
) -> Result<Vec<PendingToolCall>, AgentLoopError> {
    let mut round_seen = HashSet::new();
    for call in &mut calls {
        let upstream_id = call.id.trim().to_owned();
        if upstream_id.is_empty() {
            return Err(AgentLoopError::InvalidToolCall(
                "tool call id must not be empty".into(),
            ));
        }
        if call.name.trim().is_empty() {
            return Err(AgentLoopError::InvalidToolCall(
                "tool name must not be empty".into(),
            ));
        }
        if !round_seen.insert(upstream_id.clone()) {
            return Err(AgentLoopError::InvalidToolCall(format!(
                "duplicate tool call id in round {round}: {upstream_id}"
            )));
        }

        let mut semantic_id = upstream_id.clone();
        if seen.contains(&semantic_id) {
            semantic_id = format!("round-{round}-{upstream_id}");
            let mut suffix = 1usize;
            while seen.contains(&semantic_id) {
                semantic_id = format!("round-{round}-{suffix}-{upstream_id}");
                suffix = suffix.saturating_add(1);
            }
        }
        seen.insert(semantic_id.clone());
        call.id = semantic_id;
    }
    Ok(calls)
}

fn ensure_not_cancelled(
    cancellation: &Cancellation,
    sink: &mut dyn TurnEventSink,
) -> Result<(), AgentLoopError> {
    if cancellation.is_cancelled() {
        let _ = sink.turn_cancelled();
        return Err(AgentLoopError::Cancelled);
    }
    Ok(())
}
fn is_context_error(error: &LlmError) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    text.contains("context") || text.contains("too long") || text.contains("tokens")
}
fn compact_messages(messages: &mut Vec<(Role, String)>, target_chars: usize) {
    if messages.len() <= 1 {
        return;
    }
    let mut turns: Vec<Vec<(Role, String)>> = Vec::new();
    let mut current = Vec::new();
    for message in messages.iter() {
        if message.0 == Role::User && !current.is_empty() {
            turns.push(std::mem::take(&mut current));
        }
        current.push(message.clone());
    }
    if !current.is_empty() {
        turns.push(current);
    }
    if turns.len() <= PRESERVE_TURNS {
        return;
    }
    let current_chars: usize = messages.iter().map(|(_, text)| text.len()).sum();
    if current_chars <= target_chars {
        return;
    }
    let tail_end = turns.len().saturating_sub(PRESERVE_TURNS);
    let mut candidates: Vec<(usize, usize)> = (0..tail_end)
        .map(|i| (i, turns[i].iter().map(|(_, t)| t.len()).sum()))
        .collect();
    candidates.sort_by_key(|(_, chars)| std::cmp::Reverse(*chars));
    let mut remaining = current_chars;
    let mut evict = HashSet::new();
    for (i, chars) in candidates {
        if remaining <= target_chars {
            break;
        }
        evict.insert(i);
        remaining -= chars;
    }
    let mut compacted = Vec::new();
    for (i, turn) in turns.into_iter().enumerate() {
        if i >= tail_end || !evict.contains(&i) {
            compacted.extend(turn);
        }
    }
    *messages = compacted;
}
async fn consume_stream(
    stream: crate::LlmStream,
    cancellation: &Cancellation,
    sink: &mut dyn TurnEventSink,
) -> Result<ModelRound, AgentLoopError> {
    match consume_model_stream(stream, cancellation, sink).await {
        Ok(round) => Ok(round),
        Err(ModelProjectionError::Cancelled) => {
            let _ = sink.turn_cancelled();
            Err(AgentLoopError::Cancelled)
        }
        Err(ModelProjectionError::Llm(error)) => {
            let _ = sink.turn_failed();
            Err(AgentLoopError::Llm(error))
        }
        Err(ModelProjectionError::InvalidSequence(message)) => {
            let _ = sink.turn_failed();
            Err(AgentLoopError::InvalidModelSequence(message))
        }
        Err(ModelProjectionError::SemanticEventRejected) => {
            let _ = sink.turn_failed();
            Err(AgentLoopError::SemanticEventRejected)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModelEvent;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tokio::sync::mpsc;

    #[derive(Default)]
    struct FakeLlm {
        rounds: Mutex<VecDeque<Vec<Result<ModelEvent, LlmError>>>>,
    }

    #[async_trait::async_trait]
    impl LlmProvider for FakeLlm {
        async fn stream(&self, _: ModelRequest) -> Result<crate::LlmStream, LlmError> {
            let items = self.rounds.lock().unwrap().pop_front().unwrap_or_default();
            let (tx, rx) = mpsc::channel(items.len().max(1));
            for item in items {
                tx.send(item)
                    .await
                    .map_err(|_| LlmError::Provider("fake channel closed".into()))?;
            }
            drop(tx);
            Ok(rx)
        }
        async fn upload_image(&self, _: &str, _: &str) -> Result<String, LlmError> {
            Err(LlmError::Unavailable("not supported".into()))
        }
        fn model_info(&self, _: &str) -> crate::LlmModelInfo {
            crate::LlmModelInfo {
                supports_reasoning: true,
            }
        }
    }

    fn session() -> Session {
        Session::new(
            "sess_0123456789abcdef0123456789abcdef".into(),
            std::env::temp_dir(),
            vec![],
            "fake-model",
        )
    }
    fn emitter() -> crate::TurnEventEmitter {
        crate::TurnEventEmitter::new(
            crate::EventBus::new(),
            "sess_0123456789abcdef0123456789abcdef",
            "turn_test",
        )
    }
    #[tokio::test]
    async fn completes_text_round() {
        let llm = Arc::new(FakeLlm::default());
        llm.rounds
            .lock()
            .unwrap()
            .push_back(vec![Ok(ModelEvent::TextDelta("hello".into()))]);
        let agent = AgentLoop::new(
            llm,
            Arc::new(crate::NullToolProvider),
            AgentLoopConfig::default(),
        )
        .unwrap();
        let mut session = session();
        let mut emitter = emitter();
        let outcome = agent
            .run(
                &mut session,
                &[],
                Cancellation::new(),
                &mut emitter,
                |_, _| "prompt".into(),
            )
            .await
            .unwrap();
        assert_eq!(outcome.output, "hello");
        assert_eq!(outcome.rounds, 1);
    }
    #[tokio::test]
    async fn canonical_tool_ids_survive_history_roundtrip() {
        let llm = Arc::new(FakeLlm::default());
        llm.rounds
            .lock()
            .unwrap()
            .push_back(vec![Ok(ModelEvent::ToolCall {
                id: "call-42".into(),
                name: "search".into(),
                arguments: serde_json::json!({"q": "rust"}),
            })]);
        llm.rounds
            .lock()
            .unwrap()
            .push_back(vec![Ok(ModelEvent::TextDelta("done".into()))]);
        let agent = AgentLoop::new(
            llm,
            Arc::new(crate::NullToolProvider),
            AgentLoopConfig::default(),
        )
        .unwrap();
        let mut session = session();
        let mut emitter = emitter();
        let _ = agent
            .run(
                &mut session,
                &[],
                Cancellation::new(),
                &mut emitter,
                |_, _| "prompt".into(),
            )
            .await;
        let raw = serde_json::to_string(&session.messages).unwrap();
        let restored: crate::state::History = serde_json::from_str(&raw).unwrap();
        assert!(restored.entries().iter().any(|entry| matches!(entry, crate::state::HistoryEntry::ToolCall { id, name, .. } if id == "call-42" && name == "search")));
    }
    #[test]
    fn provider_tool_ids_can_repeat_across_rounds() {
        let mut seen = HashSet::new();
        let first = canonicalize_tool_calls(
            vec![PendingToolCall {
                id: "gemini_call_0".into(),
                name: "shell_exec".into(),
                arguments: serde_json::json!({}),
            }],
            0,
            &mut seen,
        )
        .unwrap();
        let second = canonicalize_tool_calls(
            vec![PendingToolCall {
                id: "gemini_call_0".into(),
                name: "shell_exec".into(),
                arguments: serde_json::json!({}),
            }],
            1,
            &mut seen,
        )
        .unwrap();
        assert_eq!(first[0].id, "gemini_call_0");
        assert_eq!(second[0].id, "round-1-gemini_call_0");
    }
    #[test]
    fn duplicate_provider_tool_ids_in_one_round_are_rejected() {
        let mut seen = HashSet::new();
        let error = canonicalize_tool_calls(
            vec![
                PendingToolCall {
                    id: "gemini_call_0".into(),
                    name: "shell_exec".into(),
                    arguments: serde_json::json!({}),
                },
                PendingToolCall {
                    id: "gemini_call_0".into(),
                    name: "search".into(),
                    arguments: serde_json::json!({}),
                },
            ],
            0,
            &mut seen,
        )
        .unwrap_err();
        assert!(
            matches!(error, AgentLoopError::InvalidToolCall(message) if message.contains("duplicate tool call id in round"))
        );
    }
    #[test]
    fn rejects_zero_limits() {
        assert!(AgentLoop::new(
            Arc::new(crate::NullLlmProvider),
            Arc::new(crate::NullToolProvider),
            AgentLoopConfig {
                max_rounds: 0,
                max_tool_calls_per_round: 1
            }
        )
        .is_err());
    }
}