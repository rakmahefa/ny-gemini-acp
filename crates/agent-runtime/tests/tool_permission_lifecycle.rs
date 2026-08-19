use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use agent_runtime::state::Session;
use agent_runtime::{
    AgentLoop, AgentLoopConfig, Cancellation, EventBus, LlmError, LlmProvider, ModelEvent,
    ModelRequest, SemanticEvent, ToolPermissionDecision, ToolPermissionHandler,
    ToolPermissionRequest, TurnEventEmitter,
};
use async_trait::async_trait;
use serde_json::json;
use tokio::sync::mpsc;

#[derive(Default)]
struct FakeLlm {
    rounds: Mutex<VecDeque<Vec<Result<ModelEvent, LlmError>>>>,
}

#[async_trait]
impl LlmProvider for FakeLlm {
    async fn stream(&self, _: ModelRequest) -> Result<agent_runtime::LlmStream, LlmError> {
        let items = self.rounds.lock().unwrap().pop_front().unwrap_or_default();
        let (tx, rx) = mpsc::channel(items.len().max(1));
        for item in items { tx.send(item).await.map_err(|_| LlmError::Provider("fake channel closed".into()))?; }
        drop(tx);
        Ok(rx)
    }

    async fn upload_image(&self, _: &str, _: &str) -> Result<String, LlmError> {
        Err(LlmError::Unavailable("not supported".into()))
    }

    fn model_info(&self, _: &str) -> agent_runtime::LlmModelInfo {
        agent_runtime::LlmModelInfo { supports_reasoning: false }
    }
}

#[derive(Default)]
struct FakePermission {
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl ToolPermissionHandler for FakePermission {
    fn needs_permission(&self, _: &Session, _: &ToolPermissionRequest) -> bool { true }

    async fn request_permission(
        &self,
        _: &Session,
        request: &ToolPermissionRequest,
        _: Cancellation,
    ) -> ToolPermissionDecision {
        self.calls.lock().unwrap().push(request.name.clone());
        ToolPermissionDecision::Allow
    }
}

fn session() -> Session {
    Session::new("sess_0123456789abcdef0123456789abcdef".into(), PathBuf::from("/tmp"), vec![], "fake-model")
}

#[tokio::test]
async fn emits_permission_before_execution() {
    let llm = Arc::new(FakeLlm::default());
    llm.rounds.lock().unwrap().push_back(vec![Ok(ModelEvent::ToolCall {
        id: "call-1".into(),
        name: "file_write".into(),
        arguments: json!({"path":"x.txt","content":"x"}),
    })]);

    let permission = Arc::new(FakePermission::default());
    let agent = AgentLoop::new(llm, Arc::new(agent_runtime::NullToolProvider), AgentLoopConfig::default())
        .unwrap()
        .with_permission_handler(permission.clone());

    let bus = EventBus::new();
    let mut events = bus.subscribe();
    let mut emitter = TurnEventEmitter::new(bus, "sess_0123456789abcdef0123456789abcdef", "turn_test");
    let mut session = session();

    let _ = agent.run(&mut session, &[], Cancellation::new(), &mut emitter, |_, _| "prompt".into()).await;

    let collected: Vec<SemanticEvent> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    let tool_positions: Vec<&'static str> = collected.iter().filter_map(|event| match event {
        SemanticEvent::ToolCallRequested { .. } => Some("call"),
        SemanticEvent::PermissionRequested { .. } => Some("permission"),
        SemanticEvent::ToolExecutionStarted { .. } => Some("execute"),
        SemanticEvent::ToolResultReceived { .. } => Some("result"),
        _ => None,
    }).collect();

    assert_eq!(tool_positions, vec!["call", "permission", "execute", "result"]);
    assert_eq!(permission.calls.lock().unwrap().as_slice(), &["file_write"]);
}
