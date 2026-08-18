use super::*;
use async_trait::async_trait;
use gemini_acp_config::{LlmError, LlmProvider, LlmRequest, LlmStream};
use tokio::sync::mpsc;

#[derive(Clone)]
struct TestProvider;

#[async_trait]
impl LlmProvider for TestProvider {
    fn name(&self) -> &'static str { "test" }
    async fn stream(&self, _request: LlmRequest) -> Result<LlmStream, LlmError> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(LlmStream::new(rx))
    }
}

fn test_config() -> AgentConfig {
    let dir = std::env::temp_dir().join(format!("gemini-acp-runtime-test-{}", uuid::Uuid::new_v4().simple()));
    AgentConfig { cookie_file: dir.join("cookies.json"), default_model: gemini_acp_config::core::models::DEFAULT_MODEL.to_string(), data_dir: dir.join("data"), auth_user: None, proxy: None }
}

async fn test_runtime() -> AgentRuntime {
    AgentRuntime::from_parts(test_config(), std::sync::Arc::new(TestProvider)).await.expect("runtime")
}

#[tokio::test]
async fn runtime_from_parts_creates_state_and_session_manager() {
    let runtime = test_runtime().await;
    assert!(runtime.state().store.list(None).await.is_empty());
    assert!(runtime.settings().await.is_object());
    let names = runtime.state().tools.definitions();
    assert!(names.iter().any(|tool| tool["name"] == "AskUserQuestion"));
    assert_eq!(runtime.state().provider.name(), "test");
    let _ = runtime.state().sessions.store().clone();
    runtime.shutdown().await;
}

#[tokio::test]
async fn runtime_shutdown_is_safe_without_active_turns() {
    let runtime = test_runtime().await;
    runtime.shutdown().await;
}
