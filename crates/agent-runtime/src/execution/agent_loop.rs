use std::collections::HashSet;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::events::TurnEventEmitter;
use crate::state::{Role, Session};
use crate::{
    Cancellation, GenerationOptions, LlmError, LlmProvider, ModelEvent, ModelRequest,
    ToolCallRequest, ToolCallResult, ToolProvider,
};

const DEFAULT_MAX_ROUNDS: usize = 20;
const DEFAULT_MAX_TOOL_CALLS_PER_ROUND: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentLoopConfig {
    /// Maximum number of model streams in one logical turn.
    pub max_rounds: usize,
    /// Maximum number of tool calls accepted from one model stream.
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
    #[error("semantic event emission was rejected")]
    SemanticEventRejected,
}

/// Provider-neutral model/tool orchestration for one logical agent turn.
///
/// Hardening invariants enforced here:
/// - session identity/model must be usable before a provider request is made;
/// - every tool call in a round is validated before any tool side effect;
/// - tool-call ids must be unique within a model turn;
/// - reasoning cannot resume after assistant text has started;
/// - active assistant/thinking scopes are closed on stream failure or cancellation;
/// - cancellation is checked before each externally visible state mutation;
/// - model rounds and tool-call counts remain bounded by configuration.
pub struct AgentLoop {
    llm: Arc<dyn LlmProvider>,
    tools: Arc<dyn ToolProvider>,
    config: AgentLoopConfig,
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
        })
    }

    pub fn config(&self) -> AgentLoopConfig {
        self.config
    }

    pub async fn run<F>(
        &self,
        session: &mut Session,
        references: &[String],
        cancellation: Cancellation,
        emitter: &mut TurnEventEmitter,
        build_prompt: F,
    ) -> Result<AgentLoopOutcome, AgentLoopError>
    where
        F: Fn(&Session, &dyn ToolProvider) -> String,
    {
        validate_session(session)?;

        if cancellation.is_cancelled() {
            let _ = emitter.turn_cancelled();
            return Err(AgentLoopError::Cancelled);
        }
        if !emitter.turn_started() {
            return Err(AgentLoopError::SemanticEventRejected);
        }

        let tools = self.tools.for_session(&session.id).await;
        let mut total_tool_calls = 0usize;
        let mut seen_tool_ids = HashSet::new();

        for round in 0..self.config.max_rounds {
            ensure_not_cancelled(&cancellation, emitter)?;

            let request = ModelRequest {
                prompt: build_prompt(session, &*tools),
                model: session.model.clone(),
                generation: GenerationOptions {
                    reasoning_budget: session.think,
                },
                references: references.to_vec(),
            };

            let stream = match self.llm.stream(request).await {
                Ok(stream) => stream,
                Err(error) => {
                    let _ = emitter.turn_failed();
                    return Err(AgentLoopError::Llm(error));
                }
            };

            let round_result = consume_stream(stream, &cancellation, emitter).await?;

            if round_result.event_count == 0 {
                let _ = emitter.turn_failed();
                return Err(AgentLoopError::EmptyStream);
            }

            if round_result.tool_calls.len() > self.config.max_tool_calls_per_round {
                let _ = emitter.turn_failed();
                return Err(AgentLoopError::ToolCallLimit {
                    actual: round_result.tool_calls.len(),
                    limit: self.config.max_tool_calls_per_round,
                });
            }

            validate_tool_calls(&round_result.tool_calls, &mut seen_tool_ids)?;

            if round_result.tool_calls.is_empty() {
                if round_result.text.trim().is_empty() {
                    let _ = emitter.turn_failed();
                    return Err(AgentLoopError::NoProgress);
                }

                ensure_not_cancelled(&cancellation, emitter)?;
                session
                    .messages
                    .push((Role::Assistant, round_result.text.clone()));

                if !emitter.turn_completed() {
                    let _ = emitter.turn_failed();
                    return Err(AgentLoopError::SemanticEventRejected);
                }

                return Ok(AgentLoopOutcome {
                    output: round_result.text,
                    rounds: round + 1,
                    tool_calls: total_tool_calls,
                });
            }

            total_tool_calls = total_tool_calls
                .checked_add(round_result.tool_calls.len())
                .ok_or_else(|| AgentLoopError::ToolCallLimit {
                    actual: usize::MAX,
                    limit: usize::MAX,
                })?;

            // Persist intent only after every call in this model round has passed
            // validation. This prevents malformed later calls from leaving a
            // partially committed assistant tool-intent message in history.
            ensure_not_cancelled(&cancellation, emitter)?;
            let history = format_tool_calls(&round_result.text, &round_result.tool_calls);
            if !history.is_empty() {
                session.messages.push((Role::Assistant, history));
            }

            for call in round_result.tool_calls {
                ensure_not_cancelled(&cancellation, emitter)?;

                if !emitter.tool_call_requested(call.id.clone(), call.name.clone()) {
                    let _ = emitter.turn_failed();
                    return Err(AgentLoopError::SemanticEventRejected);
                }
                if !emitter.tool_execution_started(call.id.clone()) {
                    let _ = emitter.turn_failed();
                    return Err(AgentLoopError::SemanticEventRejected);
                }

                let result = if session.tools_enabled && tools.has_tools() {
                    tools
                        .call(ToolCallRequest {
                            session_id: session.id.clone(),
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                            cwd: session.cwd.clone(),
                            additional_dirs: session.additional_directories.clone(),
                            cancellation: cancellation.subscribe(),
                        })
                        .await
                } else {
                    ToolCallResult::error(format!(
                        "tool execution disabled for session: {}",
                        call.name
                    ))
                };

                ensure_not_cancelled(&cancellation, emitter)?;

                if !emitter.tool_result_received(call.id.clone(), result.content.clone()) {
                    let _ = emitter.turn_failed();
                    return Err(AgentLoopError::SemanticEventRejected);
                }

                ensure_not_cancelled(&cancellation, emitter)?;
                session
                    .messages
                    .push((Role::Tool, canonical_tool_result(&call.name, &result)));
            }
        }

        let _ = emitter.turn_failed();
        Err(AgentLoopError::MaxRounds(self.config.max_rounds))
    }
}

#[derive(Debug)]
struct PendingToolCall {
    id: String,
    name: String,
    arguments: serde_json::Value,
}

#[derive(Debug)]
struct RoundResult {
    text: String,
    tool_calls: Vec<PendingToolCall>,
    event_count: usize,
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

fn validate_tool_calls(
    calls: &[PendingToolCall],
    seen_tool_ids: &mut HashSet<String>,
) -> Result<(), AgentLoopError> {
    for call in calls {
        let id = call.id.trim();
        if id.is_empty() {
            return Err(AgentLoopError::InvalidToolCall(
                "tool call id must not be empty".into(),
            ));
        }
        if call.name.trim().is_empty() {
            return Err(AgentLoopError::InvalidToolCall(
                "tool name must not be empty".into(),
            ));
        }
        if !seen_tool_ids.insert(id.to_owned()) {
            return Err(AgentLoopError::InvalidToolCall(format!(
                "duplicate tool call id: {id}"
            )));
        }
    }
    Ok(())
}

fn ensure_not_cancelled(
    cancellation: &Cancellation,
    emitter: &mut TurnEventEmitter,
) -> Result<(), AgentLoopError> {
    if cancellation.is_cancelled() {
        let _ = emitter.turn_cancelled();
        return Err(AgentLoopError::Cancelled);
    }
    Ok(())
}

async fn consume_stream(
    mut stream: mpsc::Receiver<Result<ModelEvent, LlmError>>,
    cancellation: &Cancellation,
    emitter: &mut TurnEventEmitter,
) -> Result<RoundResult, AgentLoopError> {
    let mut cancel_rx = cancellation.subscribe();
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    let mut event_count = 0usize;
    let mut assistant_active = false;
    let mut thinking_active = false;
    let mut text_started = false;

    loop {
        let item = tokio::select! {
            item = stream.recv() => item,
            changed = cancel_rx.changed() => {
                if changed.is_ok() && *cancel_rx.borrow() {
                    close_active_scopes(emitter, &mut thinking_active, &mut assistant_active);
                    let _ = emitter.turn_cancelled();
                    return Err(AgentLoopError::Cancelled);
                }
                continue;
            }
        };

        let Some(item) = item else {
            break;
        };

        let event = match item {
            Ok(event) => event,
            Err(error) => {
                close_active_scopes(emitter, &mut thinking_active, &mut assistant_active);
                let _ = emitter.turn_failed();
                return Err(AgentLoopError::Llm(error));
            }
        };
        event_count = event_count.saturating_add(1);

        match event {
            ModelEvent::ReasoningDelta(delta) => {
                if text_started {
                    close_active_scopes(emitter, &mut thinking_active, &mut assistant_active);
                    let _ = emitter.turn_failed();
                    return Err(AgentLoopError::InvalidToolCall(
                        "reasoning resumed after assistant text started".into(),
                    ));
                }
                if !assistant_active {
                    if !emitter.assistant_started() {
                        let _ = emitter.turn_failed();
                        return Err(AgentLoopError::SemanticEventRejected);
                    }
                    assistant_active = true;
                }
                if !thinking_active {
                    if !emitter.thinking_started() {
                        let _ = emitter.turn_failed();
                        return Err(AgentLoopError::SemanticEventRejected);
                    }
                    thinking_active = true;
                }
                if !emitter.thinking_delta(delta) {
                    let _ = emitter.turn_failed();
                    return Err(AgentLoopError::SemanticEventRejected);
                }
            }
            ModelEvent::TextDelta(delta) => {
                if !assistant_active {
                    if !emitter.assistant_started() {
                        let _ = emitter.turn_failed();
                        return Err(AgentLoopError::SemanticEventRejected);
                    }
                    assistant_active = true;
                }
                if thinking_active {
                    if !emitter.thinking_completed() {
                        let _ = emitter.turn_failed();
                        return Err(AgentLoopError::SemanticEventRejected);
                    }
                    thinking_active = false;
                }
                if !delta.is_empty() {
                    text_started = true;
                }
                if !emitter.assistant_delta(delta.clone()) {
                    let _ = emitter.turn_failed();
                    return Err(AgentLoopError::SemanticEventRejected);
                }
                text.push_str(&delta);
            }
            ModelEvent::ToolCall { id, name, arguments } => {
                tool_calls.push(PendingToolCall { id, name, arguments });
            }
            ModelEvent::Usage { .. } => {}
        }
    }

    if thinking_active && !emitter.thinking_completed() {
        let _ = emitter.turn_failed();
        return Err(AgentLoopError::SemanticEventRejected);
    }
    if assistant_active && !emitter.assistant_completed() {
        let _ = emitter.turn_failed();
        return Err(AgentLoopError::SemanticEventRejected);
    }

    Ok(RoundResult {
        text,
        tool_calls,
        event_count,
    })
}

fn close_active_scopes(
    emitter: &mut TurnEventEmitter,
    thinking_active: &mut bool,
    assistant_active: &mut bool,
) {
    if *thinking_active {
        let _ = emitter.thinking_completed();
        *thinking_active = false;
    }
    if *assistant_active {
        let _ = emitter.assistant_completed();
        *assistant_active = false;
    }
}

fn format_tool_calls(text: &str, calls: &[PendingToolCall]) -> String {
    let mut result = String::new();
    if !text.trim().is_empty() {
        result.push_str(text.trim());
    }
    for call in calls {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("[tool_call ");
        result.push_str(&call.name);
        result.push_str(" id=");
        result.push_str(&call.id);
        result.push_str("] ");
        result.push_str(&call.arguments.to_string());
    }
    result
}

fn canonical_tool_result(name: &str, result: &ToolCallResult) -> String {
    let status = if result.is_ok { "ok" } else { "error" };
    format!("[tool_result {name} status={status}] {}", result.content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::sync::Mutex;

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
            crate::LlmModelInfo { supports_reasoning: true }
        }
    }

    #[derive(Clone)]
    struct CountingTools {
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl CountingTools {
        fn new() -> Self {
            Self { calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)) }
        }
    }

    #[async_trait::async_trait]
    impl ToolProvider for CountingTools {
        async fn for_session(&self, _: &str) -> Arc<dyn ToolProvider> { Arc::new(self.clone()) }
        async fn configure_session(&self, _: &str, _: std::path::PathBuf, _: Vec<crate::ToolServerConfig>) -> Result<(), String> { Ok(()) }
        async fn clear_session(&self, _: &str) {}
        fn definitions(&self) -> Vec<serde_json::Value> { Vec::new() }
        fn prompt_fragment(&self) -> Option<String> { None }
        fn has_tools(&self) -> bool { true }
        async fn call(&self, request: ToolCallRequest) -> ToolCallResult {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            ToolCallResult { content: format!("{} ok", request.name), is_ok: true, executed: true }
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

    fn emitter() -> TurnEventEmitter {
        let bus = crate::EventBus::new();
        TurnEventEmitter::new(bus, "sess_0123456789abcdef0123456789abcdef", "turn_test")
    }

    fn prompt(session: &Session) -> String {
        session.messages.iter().map(|(_, text)| text.clone()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn rejects_zero_limits() {
        assert!(AgentLoop::new(Arc::new(crate::NullLlmProvider), Arc::new(crate::NullToolProvider), AgentLoopConfig { max_rounds: 0, max_tool_calls_per_round: 1 }).is_err());
        assert!(AgentLoop::new(Arc::new(crate::NullLlmProvider), Arc::new(crate::NullToolProvider), AgentLoopConfig { max_rounds: 1, max_tool_calls_per_round: 0 }).is_err());
    }

    #[tokio::test]
    async fn completes_text_only_round() {
        let llm = Arc::new(FakeLlm::default());
        llm.rounds.lock().unwrap().push_back(vec![
            Ok(ModelEvent::TextDelta("hello".into())),
            Ok(ModelEvent::TextDelta(" world".into())),
        ]);
        let loop_ = AgentLoop::new(llm, Arc::new(crate::NullToolProvider), AgentLoopConfig::default()).unwrap();
        let mut session = session();
        let mut emitter = emitter();
        let outcome = loop_.run(&mut session, &[], Cancellation::new(), &mut emitter, |session, _| prompt(session)).await.unwrap();
        assert_eq!(outcome.output, "hello world");
        assert_eq!(outcome.rounds, 1);
        assert_eq!(session.messages.last(), Some(&(Role::Assistant, "hello world".into())));
    }

    #[tokio::test]
    async fn validates_all_tool_calls_before_any_execution() {
        let llm = Arc::new(FakeLlm::default());
        llm.rounds.lock().unwrap().push_back(vec![
            Ok(ModelEvent::ToolCall { id: "good".into(), name: "one".into(), arguments: json!({}) }),
            Ok(ModelEvent::ToolCall { id: "".into(), name: "bad".into(), arguments: json!({}) }),
        ]);
        let tools = CountingTools::new();
        let calls = Arc::clone(&tools.calls);
        let loop_ = AgentLoop::new(llm, Arc::new(tools), AgentLoopConfig::default()).unwrap();
        let mut session = session();
        let mut emitter = emitter();
        let error = loop_.run(&mut session, &[], Cancellation::new(), &mut emitter, |_, _| String::new()).await.unwrap_err();
        assert!(matches!(error, AgentLoopError::InvalidToolCall(_)));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn rejects_duplicate_tool_ids_across_rounds() {
        let llm = Arc::new(FakeLlm::default());
        for _ in 0..2 {
            llm.rounds.lock().unwrap().push_back(vec![Ok(ModelEvent::ToolCall { id: "dup".into(), name: "tool".into(), arguments: json!({}) })]);
        }
        let loop_ = AgentLoop::new(llm, Arc::new(CountingTools::new()), AgentLoopConfig::default()).unwrap();
        let mut session = session();
        let mut emitter = emitter();
        let error = loop_.run(&mut session, &[], Cancellation::new(), &mut emitter, |session, _| prompt(session)).await.unwrap_err();
        assert!(matches!(error, AgentLoopError::InvalidToolCall(message) if message.contains("duplicate tool call id")));
    }

    #[tokio::test]
    async fn rejects_reasoning_after_text() {
        let llm = Arc::new(FakeLlm::default());
        llm.rounds.lock().unwrap().push_back(vec![
            Ok(ModelEvent::TextDelta("answer".into())),
            Ok(ModelEvent::ReasoningDelta("late".into())),
        ]);
        let loop_ = AgentLoop::new(llm, Arc::new(crate::NullToolProvider), AgentLoopConfig::default()).unwrap();
        let mut session = session();
        let mut emitter = emitter();
        let error = loop_.run(&mut session, &[], Cancellation::new(), &mut emitter, |_, _| String::new()).await.unwrap_err();
        assert!(matches!(error, AgentLoopError::InvalidToolCall(message) if message.contains("reasoning resumed")));
    }

    #[tokio::test]
    async fn closes_active_scopes_on_stream_error() {
        let llm = Arc::new(FakeLlm::default());
        llm.rounds.lock().unwrap().push_back(vec![
            Ok(ModelEvent::ReasoningDelta("think".into())),
            Err(LlmError::Provider("boom".into())),
        ]);
        let loop_ = AgentLoop::new(llm, Arc::new(crate::NullToolProvider), AgentLoopConfig::default()).unwrap();
        let mut session = session();
        let mut emitter = emitter();
        let error = loop_.run(&mut session, &[], Cancellation::new(), &mut emitter, |_, _| String::new()).await.unwrap_err();
        assert!(matches!(error, AgentLoopError::Llm(_)));
    }

    #[tokio::test]
    async fn enforces_tool_call_limit_before_execution() {
        let llm = Arc::new(FakeLlm::default());
        llm.rounds.lock().unwrap().push_back(vec![
            Ok(ModelEvent::ToolCall { id: "a".into(), name: "one".into(), arguments: json!({}) }),
            Ok(ModelEvent::ToolCall { id: "b".into(), name: "two".into(), arguments: json!({}) }),
        ]);
        let tools = CountingTools::new();
        let calls = Arc::clone(&tools.calls);
        let loop_ = AgentLoop::new(llm, Arc::new(tools), AgentLoopConfig { max_rounds: 2, max_tool_calls_per_round: 1 }).unwrap();
        let mut session = session();
        let mut emitter = emitter();
        let error = loop_.run(&mut session, &[], Cancellation::new(), &mut emitter, |_, _| String::new()).await.unwrap_err();
        assert!(matches!(error, AgentLoopError::ToolCallLimit { actual: 2, limit: 1 }));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn executes_tool_then_runs_next_round() {
        let llm = Arc::new(FakeLlm::default());
        llm.rounds.lock().unwrap().push_back(vec![Ok(ModelEvent::ToolCall { id: "call-1".into(), name: "search".into(), arguments: json!({"q": "rust"}) })]);
        llm.rounds.lock().unwrap().push_back(vec![Ok(ModelEvent::TextDelta("result".into()))]);
        let loop_ = AgentLoop::new(llm, Arc::new(CountingTools::new()), AgentLoopConfig::default()).unwrap();
        let mut session = session();
        let mut emitter = emitter();
        let outcome = loop_.run(&mut session, &[], Cancellation::new(), &mut emitter, |session, _| prompt(session)).await.unwrap();
        assert_eq!(outcome.rounds, 2);
        assert_eq!(outcome.tool_calls, 1);
        assert_eq!(outcome.output, "result");
        assert!(session.messages.iter().any(|(role, text)| *role == Role::Tool && text.contains("search")));
    }

    #[tokio::test]
    async fn rejects_empty_stream() {
        let llm = Arc::new(FakeLlm::default());
        llm.rounds.lock().unwrap().push_back(vec![]);
        let loop_ = AgentLoop::new(llm, Arc::new(crate::NullToolProvider), AgentLoopConfig::default()).unwrap();
        let mut session = session();
        let mut emitter = emitter();
        let error = loop_.run(&mut session, &[], Cancellation::new(), &mut emitter, |_, _| String::new()).await.unwrap_err();
        assert!(matches!(error, AgentLoopError::EmptyStream));
    }

    #[tokio::test]
    async fn rejects_no_progress_after_usage_only_stream() {
        let llm = Arc::new(FakeLlm::default());
        llm.rounds.lock().unwrap().push_back(vec![Ok(ModelEvent::Usage { prompt_tokens: Some(1), completion_tokens: Some(1), total_tokens: Some(2) })]);
        let loop_ = AgentLoop::new(llm, Arc::new(crate::NullToolProvider), AgentLoopConfig::default()).unwrap();
        let mut session = session();
        let mut emitter = emitter();
        let error = loop_.run(&mut session, &[], Cancellation::new(), &mut emitter, |_, _| String::new()).await.unwrap_err();
        assert!(matches!(error, AgentLoopError::NoProgress));
    }

    #[tokio::test]
    async fn rejects_invalid_session_before_provider_call() {
        let llm = Arc::new(FakeLlm::default());
        let loop_ = AgentLoop::new(llm, Arc::new(crate::NullToolProvider), AgentLoopConfig::default()).unwrap();
        let mut session = session();
        session.model.clear();
        let mut emitter = emitter();
        let error = loop_.run(&mut session, &[], Cancellation::new(), &mut emitter, |_, _| String::new()).await.unwrap_err();
        assert!(matches!(error, AgentLoopError::InvalidSession(_)));
    }

    #[tokio::test]
    async fn disabled_tools_are_not_executed() {
        let llm = Arc::new(FakeLlm::default());
        llm.rounds.lock().unwrap().push_back(vec![Ok(ModelEvent::ToolCall { id: "call-1".into(), name: "disabled".into(), arguments: json!({}) })]);
        llm.rounds.lock().unwrap().push_back(vec![Ok(ModelEvent::TextDelta("done".into()))]);
        let loop_ = AgentLoop::new(llm, Arc::new(CountingTools::new()), AgentLoopConfig::default()).unwrap();
        let mut session = session();
        session.tools_enabled = false;
        let mut emitter = emitter();
        let outcome = loop_.run(&mut session, &[], Cancellation::new(), &mut emitter, |session, _| prompt(session)).await.unwrap();
        assert_eq!(outcome.output, "done");
        assert!(session.messages.iter().any(|(role, text)| *role == Role::Tool && text.contains("disabled")));
    }
}
