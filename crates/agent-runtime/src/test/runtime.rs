use super::*;
use crate::{NullLlmProvider, NullToolProvider};
use std::sync::Arc;

const TEST_MODEL: &str = "test-model";

fn test_config() -> RuntimeConfig {
    let dir = std::env::temp_dir().join(format!(
        "agent-runtime-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    RuntimeConfig {
        data_dir: dir.join("data"),
        default_model: TEST_MODEL.into(),
    }
}

#[tokio::test]
async fn runtime_new_creates_state_and_session_manager() {
    let runtime = AgentRuntime::new(
        test_config(),
        Arc::new(NullLlmProvider),
        Arc::new(NullToolProvider),
    )
    .await
    .expect("runtime");
    assert!(runtime.state().store.list(None).await.is_empty());
    assert!(!runtime.state().tools.has_tools());
    let _ = runtime.state().sessions.store().clone();
    runtime.shutdown().await;
}

#[tokio::test]
async fn runtime_shutdown_is_safe_without_active_turns() {
    let runtime = AgentRuntime::new(
        test_config(),
        Arc::new(NullLlmProvider),
        Arc::new(NullToolProvider),
    )
    .await
    .expect("runtime");
    runtime.shutdown().await;
}
