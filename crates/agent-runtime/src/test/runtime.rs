use super::*;
use crate::{NullLlmProvider, NullToolProvider};
use std::sync::Arc;

fn test_config() -> RuntimeConfig {
    let dir = std::env::temp_dir().join(format!(
        "gemini-acp-runtime-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    RuntimeConfig {
        data_dir: dir.join("data"),
        default_model: "gemini-3.6-flash".into(),
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
