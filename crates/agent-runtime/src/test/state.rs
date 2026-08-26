use super::*;
use std::path::Path;

const TEST_MODEL: &str = "test-model";

fn history(entries: &[(Role, &str)]) -> History {
    History::from(
        entries
            .iter()
            .map(|(role, text)| (*role, (*text).to_string()))
            .collect::<Vec<_>>(),
    )
}

#[tokio::test]
async fn cycle_create_persist_reload() {
    let dir = std::env::temp_dir().join(format!("acp-test-{}", uuid::Uuid::new_v4().simple()));
    let store = Store::open(&dir).await.unwrap();
    let s = store
        .create("/tmp".into(), vec!["/other".into()], TEST_MODEL)
        .await
        .unwrap();
    let mut s2 = store.get(&s.id).await.unwrap();
    s2.messages.push((Role::User, "bonjour".into()));
    s2.created_at = "2000-01-01T00:00:00Z".to_string();
    store.end_turn(&s.id, s2, 0).await.unwrap();
    let reloaded = store.get(&s.id).await.unwrap();
    assert_eq!(
        reloaded.messages.entries(),
        history(&[(Role::User, "bonjour")]).entries()
    );
    assert_ne!(reloaded.updated_at, reloaded.created_at);
    assert_eq!(store.list(None).await.len(), 1);
    assert_eq!(store.list(Some(Path::new("/nope"))).await.len(), 0);
    assert!(store.delete(&s.id).await);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn busy_sentinel_blocks_second_persistence_transaction() {
    let dir = std::env::temp_dir().join(format!("acp-test-{}", uuid::Uuid::new_v4().simple()));
    let store = Store::open(&dir).await.unwrap();
    let s = store
        .create("/tmp".into(), vec![], TEST_MODEL)
        .await
        .unwrap();
    let (session1, generation1) = store.begin_turn(&s.id).await.unwrap();
    assert!(matches!(
        store.begin_turn(&s.id).await,
        Err(TurnError::AlreadyRunning)
    ));
    store.end_turn(&s.id, session1, generation1).await.unwrap();
    assert!(store.begin_turn(&s.id).await.is_ok());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn cleanup_tmp_orphelins_au_demarrage() {
    let dir = std::env::temp_dir().join(format!("acp-test-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(dir.join("sessions")).unwrap();
    std::fs::write(
        dir.join("sessions").join("orphelin.json.tmp"),
        r#"{"incomplete": true}"#,
    )
    .unwrap();
    let _store = Store::open(&dir).await.unwrap();
    assert!(!dir.join("sessions").join("orphelin.json.tmp").exists());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn generation_rejects_stale_turn_finish() {
    let dir = std::env::temp_dir().join(format!("acp-test-{}", uuid::Uuid::new_v4().simple()));
    let store = Store::open(&dir).await.unwrap();
    let s = store
        .create("/tmp".into(), vec![], TEST_MODEL)
        .await
        .unwrap();
    let (session1, generation1) = store.begin_turn(&s.id).await.unwrap();
    store.end_turn(&s.id, session1, generation1).await.unwrap();
    let (session2, generation2) = store.begin_turn(&s.id).await.unwrap();
    assert_eq!(generation2, generation1 + 1);
    assert!(store.end_turn(&s.id, session2, generation1).await.is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn failed_persist_does_not_corrupt_live_session() {
    let dir = std::env::temp_dir().join(format!(
        "acp-persist-failure-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let store = Store::open(&dir).await.unwrap();
    let session = store
        .create("/tmp".into(), vec![], TEST_MODEL)
        .await
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    let result = store
        .update_session(&session.id, |current| {
            current.title = Some("should-not-be-live".into());
        })
        .await;
    assert!(result.is_err());

    let live = store.get(&session.id).await.unwrap();
    assert_eq!(live.title, None);
}

#[tokio::test]
async fn open_removes_orphan_busy_sentinel() {
    let dir = std::env::temp_dir().join(format!(
        "acp-busy-recovery-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("sess_orphan.busy"), b"").unwrap();

    let _store = Store::open(&dir).await.unwrap();
    assert!(!dir.join("sess_orphan.busy").exists());

    std::fs::remove_dir_all(&dir).ok();
}
