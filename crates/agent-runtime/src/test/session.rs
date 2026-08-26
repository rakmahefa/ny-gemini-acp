use super::*;

#[test]
fn valide_id_session() {
    assert!(SessionManager::validate_id("sess_0123456789abcdef0123456789abcdef").is_ok());
    assert!(SessionManager::validate_id("sess_0123456789ABCDEF0123456789abcdef").is_err());
    assert!(SessionManager::validate_id("../sess_0123456789abcdef0123456789abcdef").is_err());
}

#[test]
fn sanitize_title_collabse_et_tronque() {
    assert_eq!(
        SessionManager::sanitize_title("  hello\n   world  ").as_deref(),
        Some("hello world")
    );
    let long = "a".repeat(MAX_TITLE_LENGTH + 40);
    let title = SessionManager::sanitize_title(&long).unwrap();
    assert_eq!(title.chars().count(), MAX_TITLE_LENGTH);
    assert!(title.ends_with('…'));
}

#[tokio::test]
async fn configure_mcp_reports_missing_session_structurally() {
    let dir = std::env::temp_dir().join(format!(
        "acp-session-tool-error-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let store = std::sync::Arc::new(Store::open(&dir).await.unwrap());
    let manager = SessionManager::new(store);

    let error = manager
        .configure_mcp_typed("sess_missing", Vec::new())
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        SessionToolConfigurationError::SessionNotFound(id) if id == "sess_missing"
    ));

    std::fs::remove_dir_all(dir).ok();
}
