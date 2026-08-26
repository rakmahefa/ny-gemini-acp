use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_runtime::events::{EventBus, TurnEventEmitter};
use agent_runtime::state::{HistoryEntry, Store};
use agent_runtime::{
    AgentLoopConfig, Cancellation, LlmError, LlmModelInfo, LlmProvider, ModelEvent,
    ModelRequest, ToolCallRequest, ToolCallResult, ToolConfigurationError, ToolProvider,
    ToolUiModel, TurnExecutionRequest, TurnService,
};
use serde_json::{json, Value};
use tokio::sync::mpsc;

struct ScriptedLlm {
    rounds: Mutex<VecDeque<Result<Vec<ModelEvent>, LlmError>>>,
}

impl ScriptedLlm {
    fn new(rounds: Vec<Result<Vec<ModelEvent>, LlmError>>) -> Self {
        Self {
            rounds: Mutex::new(rounds.into()),
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for ScriptedLlm {
    async fn stream(&self, _request: ModelRequest) -> Result<agent_runtime::LlmStream, LlmError> {
        let next = self
            .rounds
            .lock()
            .expect("scripted LLM mutex poisoned")
            .pop_front()
            .expect("unexpected extra model round");
        let events = next?;
        let (tx, rx) = mpsc::channel(events.len().max(1));
        for event in events {
            tx.send(Ok(event)).await.expect("test receiver must stay alive");
        }
        drop(tx);
        Ok(rx)
    }

    async fn upload_image(&self, _base64: &str, _mime: &str) -> Result<String, LlmError> {
        Err(LlmError::Upload("image upload is not part of this test".into()))
    }

    fn model_info(&self, _model: &str) -> LlmModelInfo {
        LlmModelInfo::default()
    }
}

#[derive(Clone)]
struct RecordingTool {
    calls: Arc<Mutex<Vec<String>>>,
}

impl RecordingTool {
    fn new(calls: Arc<Mutex<Vec<String>>>) -> Self {
        Self { calls }
    }
}

#[async_trait::async_trait]
impl ToolProvider for RecordingTool {
    async fn for_session(&self, _session_id: &str) -> Arc<dyn ToolProvider> {
        Arc::new(self.clone())
    }

    async fn configure_session(
        &self,
        _session_id: &str,
        _cwd: PathBuf,
        _servers: Vec<agent_runtime::ToolServerConfig>,
    ) -> Result<(), ToolConfigurationError> {
        Ok(())
    }

    async fn clear_session(&self, _session_id: &str) {}

    fn definitions(&self) -> Vec<Value> {
        vec![json!({
            "name": "record",
            "description": "records one argument",
            "parameters": {"type": "object"}
        })]
    }

    fn prompt_fragment(&self) -> Option<String> {
        Some("record tool available".into())
    }

    fn has_tools(&self) -> bool {
        true
    }

    fn ui_model(&self, _call_id: &str, _name: &str, _arguments: &Value) -> Option<ToolUiModel> {
        None
    }

    async fn call(&self, request: ToolCallRequest) -> ToolCallResult {
        let value = request.arguments["value"].as_str().unwrap_or("missing");
        self.calls
            .lock()
            .expect("tool calls mutex poisoned")
            .push(format!("{}:{}", request.call_id, value));
        ToolCallResult {
            content: "recorded".into(),
            is_ok: true,
            executed: true,
            ui: None,
        }
    }
}

fn build_prompt(_session: &agent_runtime::state::Session, _tools: &dyn ToolProvider) -> String {
    "test prompt".into()
}

async fn begin_test_turn(
    prefix: &str,
    llm: Arc<dyn LlmProvider>,
    tools: Arc<dyn ToolProvider>,
) -> (
    Arc<Store>,
    String,
    agent_runtime::state::Session,
    u64,
    TurnService,
    TurnEventEmitter,
    EventBus,
    std::path::PathBuf,
) {
    let dir = std::env::temp_dir().join(format!(
        "ny-gemini-acp-{prefix}-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let store = Arc::new(Store::open(&dir).await.expect("store must open"));
    let session = store
        .create("/tmp".into(), vec![], "test-model")
        .await
        .expect("session must be created");
    let (started, generation) = store
        .begin_turn(&session.id)
        .await
        .expect("turn must begin");

    let bus = EventBus::new();
    let _projection = bus.subscribe_turn("integration-turn");
    let semantic = TurnEventEmitter::new_with_required_transport(
        bus.clone(),
        session.id.clone(),
        "integration-turn",
    );
    let service = TurnService::new(store.clone(), llm, tools, AgentLoopConfig::default());

    (
        store,
        session.id.clone(),
        started,
        generation,
        service,
        semantic,
        bus,
        dir,
    )
}

#[tokio::test]
async fn success_crosses_model_runtime_events_and_persistence() {
    let (store, session_id, session, generation, service, mut semantic, bus, dir) =
        begin_test_turn(
            "success",
            Arc::new(ScriptedLlm::new(vec![Ok(vec![ModelEvent::TextDelta(
                "pipeline success".into(),
            )])])),
            Arc::new(agent_runtime::NullToolProvider),
        )
        .await;

    let result = service
        .run_started(TurnExecutionRequest {
            session_id: session_id.clone(),
            session,
            generation,
            references: Vec::new(),
            cancellation: Cancellation::new(),
            semantic: &mut semantic,
            action_handler: None,
            permission_handler: None,
            build_prompt,
        })
        .await
        .expect("successful turn must complete");

    assert_eq!(result.outcome.output, "pipeline success");
    assert_eq!(result.outcome.rounds, 1);
    assert_eq!(result.outcome.tool_calls, 0);
    assert!(semantic.is_terminal());

    let persisted = store
        .get(&session_id)
        .await
        .expect("persisted session must exist");
    assert!(persisted.messages.entries().iter().any(|entry| {
        matches!(entry, HistoryEntry::Assistant { content } if content == "pipeline success")
    }));
    assert_eq!(persisted.turn_count, 1);

    bus.close_turn("integration-turn");
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn tool_round_crosses_execution_and_preserves_canonical_call_id() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let llm = Arc::new(ScriptedLlm::new(vec![
        Ok(vec![ModelEvent::ToolCall {
            id: "upstream-42".into(),
            name: "record".into(),
            arguments: json!({"value": "alpha"}),
        }]),
        Ok(vec![ModelEvent::TextDelta("tool result consumed".into())]),
    ]));
    let tools = Arc::new(RecordingTool::new(calls.clone()));
    let (store, session_id, session, generation, service, mut semantic, bus, dir) =
        begin_test_turn("tool", llm, tools).await;

    let result = service
        .run_started(TurnExecutionRequest {
            session_id: session_id.clone(),
            session,
            generation,
            references: Vec::new(),
            cancellation: Cancellation::new(),
            semantic: &mut semantic,
            action_handler: None,
            permission_handler: None,
            build_prompt,
        })
        .await
        .expect("tool round must complete");

    assert_eq!(result.outcome.output, "tool result consumed");
    assert_eq!(result.outcome.rounds, 2);
    assert_eq!(result.outcome.tool_calls, 1);
    assert_eq!(
        calls.lock().expect("tool calls mutex poisoned").as_slice(),
        ["upstream-42:alpha".to_string()]
    );
    assert!(semantic.is_terminal());

    let persisted = store
        .get(&session_id)
        .await
        .expect("persisted session must exist");
    let entries = persisted.messages.entries();
    assert!(entries.iter().any(|entry| {
        matches!(entry, HistoryEntry::ToolCall { id, name, .. } if id == "upstream-42" && name == "record")
    }));
    assert!(entries.iter().any(|entry| {
        matches!(entry, HistoryEntry::ToolResult { id, content, .. } if id == "upstream-42" && content == "recorded")
    }));

    bus.close_turn("integration-turn");
    std::fs::remove_dir_all(dir).ok();
}

#[tokio::test]
async fn provider_failure_is_terminal_and_turn_is_finalized() {
    let (store, session_id, session, generation, service, mut semantic, bus, dir) =
        begin_test_turn(
            "provider-failure",
            Arc::new(ScriptedLlm::new(vec![Err(LlmError::Network(
                "simulated outage".into(),
            ))])),
            Arc::new(agent_runtime::NullToolProvider),
        )
        .await;

    let error = service
        .run_started(TurnExecutionRequest {
            session_id: session_id.clone(),
            session,
            generation,
            references: Vec::new(),
            cancellation: Cancellation::new(),
            semantic: &mut semantic,
            action_handler: None,
            permission_handler: None,
            build_prompt,
        })
        .await
        .expect_err("provider failure must propagate");

    assert!(matches!(error, agent_runtime::TurnServiceError::Agent(_)));
    assert!(semantic.is_terminal());

    let (_, persisted_generation) = store
        .begin_turn(&session_id)
        .await
        .expect("failed turn must release the busy state");
    assert_eq!(persisted_generation, generation + 1);

    bus.close_turn("integration-turn");
    std::fs::remove_dir_all(dir).ok();
}
