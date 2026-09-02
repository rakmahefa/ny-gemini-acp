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
async fn end_turn_after_delete_aborts_instead_of_resurrecting() {
    // D-05 : une session supprimée pendant un tour ne doit pas être
    // réécrite par end_turn (résurrection).
    let dir = std::env::temp_dir().join(format!(
        "acp-delete-race-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let store = Store::open(&dir).await.unwrap();
    let s = store
        .create("/tmp".into(), vec![], TEST_MODEL)
        .await
        .unwrap();
    let (session1, generation1) = store.begin_turn(&s.id).await.unwrap();

    // Suppression concurrente (comme session/delete pendant un tour).
    assert!(store.delete(&s.id).await);
    assert!(!dir.join(format!("{}.json", s.id)).exists());

    // Le commit du tour doit abandonner, pas réécrire le JSON.
    let mut finished = session1;
    finished.messages.push((Role::User, "ghost".into()));
    let result = store.end_turn(&s.id, finished, generation1).await;
    assert!(
        matches!(result, Err(StoreError::SessionDeleted(_))),
        "got: {result:?}"
    );
    assert!(!dir.join(format!("{}.json", s.id)).exists());

    // La session n'est pas réapparue et un nouveau tour est possible côté
    // busy sentinel (pas de sentinel orpheline bloquante).
    assert!(store.get(&s.id).await.is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn end_turn_merges_concurrent_config_updates() {
    // D-05 : un update_session concurrent (ex. session/set_mode pendant un
    // tour) ne doit pas être écrasé par le commit de fin de tour.
    let dir = std::env::temp_dir().join(format!(
        "acp-endturn-merge-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let store = Store::open(&dir).await.unwrap();
    let s = store
        .create("/tmp".into(), vec![], TEST_MODEL)
        .await
        .unwrap();
    let (session1, generation1) = store.begin_turn(&s.id).await.unwrap();

    // Mise à jour concurrente du mode pendant le tour.
    store
        .update_session(&s.id, |current| {
            current.mode = crate::state::SessionMode::BypassPermissions;
        })
        .await
        .unwrap();

    let mut finished = session1;
    finished.messages.push((Role::User, "hello".into()));
    store.end_turn(&s.id, finished, generation1).await.unwrap();

    let reloaded = store.get(&s.id).await.unwrap();
    assert_eq!(
        reloaded.mode,
        crate::state::SessionMode::BypassPermissions,
        "le mode modifié en concurrent doit survivre au end_turn"
    );
    assert_eq!(reloaded.turn_count, 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn delete_cleans_up_snapshots() {
    // D-05 : Store::delete retire aussi les snapshots.
    let dir = std::env::temp_dir().join(format!(
        "acp-delete-snapshots-test-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let store = Store::open(&dir).await.unwrap();
    let s = store
        .create("/tmp".into(), vec![], TEST_MODEL)
        .await
        .unwrap();
    let snapshot = store.snapshot_path(&s.id, 3);
    tokio::fs::write(&snapshot, b"{}").await.unwrap();

    assert!(store.delete(&s.id).await);
    assert!(!snapshot.exists());
    std::fs::remove_dir_all(&dir).ok();
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

#[tokio::test]
async fn derived_title_survives_end_turn_and_store_reload() {
    // SPEC-P0-02 acceptance: create -> derive title (written through the
    // store path) -> end_turn -> reopen a store on the same data-dir ->
    // session list and read return the title.
    let dir = std::env::temp_dir().join(format!("acp-title-{}", uuid::Uuid::new_v4().simple()));
    let store = Store::open(&dir).await.unwrap();
    let s = store
        .create("/tmp".into(), vec![], TEST_MODEL)
        .await
        .unwrap();
    let (session, generation) = store.begin_turn(&s.id).await.unwrap();
    store
        .update_session(&s.id, |live| live.title = Some("Derived title".into()))
        .await
        .unwrap();
    store.end_turn(&s.id, session, generation).await.unwrap();

    let reloaded = Store::open(&dir).await.unwrap();
    let restored = reloaded.get(&s.id).await.unwrap();
    assert_eq!(restored.title.as_deref(), Some("Derived title"));
    assert!(reloaded
        .list(None)
        .await
        .iter()
        .any(|session| session.title.as_deref() == Some("Derived title")));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn fork_of_titled_session_keeps_title() {
    let dir =
        std::env::temp_dir().join(format!("acp-title-fork-{}", uuid::Uuid::new_v4().simple()));
    let store = Store::open(&dir).await.unwrap();
    let s = store
        .create("/tmp".into(), vec![], TEST_MODEL)
        .await
        .unwrap();
    store
        .update_session(&s.id, |live| live.title = Some("Derived title".into()))
        .await
        .unwrap();

    let forked = store.fork(&s.id).await.unwrap();
    assert_eq!(forked.title.as_deref(), Some("Derived title (fork)"));
    // The fork copy itself must be titled on disk, not only in the live map.
    let reopened = Store::open(&dir).await.unwrap();
    let restored = reopened.get(&forked.id).await.unwrap();
    assert_eq!(restored.title.as_deref(), Some("Derived title (fork)"));
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn session_without_derived_title_stays_untitled() {
    let dir =
        std::env::temp_dir().join(format!("acp-title-none-{}", uuid::Uuid::new_v4().simple()));
    let store = Store::open(&dir).await.unwrap();
    let s = store
        .create("/tmp".into(), vec![], TEST_MODEL)
        .await
        .unwrap();
    let (session, generation) = store.begin_turn(&s.id).await.unwrap();
    store.end_turn(&s.id, session, generation).await.unwrap();
    let restored = store.get(&s.id).await.unwrap();
    assert!(restored.title.is_none());
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn io_failure_on_begin_turn_reports_busy_io_not_already_running() {
    // SPEC-P1-01 acceptance: a failing storage backend must surface as a
    // BusyIo diagnostic, never as "a turn is already active". A session id
    // longer than the filesystem limit makes the sentinel creation fail with
    // an I/O error regardless of the environment's privileges.
    let dir = std::env::temp_dir().join(format!("acp-busyio-{}", uuid::Uuid::new_v4().simple()));
    let store = Store::open(&dir).await.unwrap();
    let hostile_id = format!("sess_{}", "a".repeat(300));
    match store.begin_turn(&hostile_id).await {
        Err(TurnError::BusyIo(message)) => {
            assert!(
                !message.is_empty(),
                "BusyIo must carry the underlying error"
            );
        }
        other => panic!("expected BusyIo, got {other:?}"),
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn concurrent_begin_turn_still_reports_already_running() {
    // SPEC-P1-01 acceptance: the concurrency signal is preserved.
    let dir = std::env::temp_dir().join(format!("acp-busy-conc-{}", uuid::Uuid::new_v4().simple()));
    let store = Store::open(&dir).await.unwrap();
    let s = store
        .create("/tmp".into(), vec![], TEST_MODEL)
        .await
        .unwrap();
    let _first = store.begin_turn(&s.id).await.unwrap();
    assert!(matches!(
        store.begin_turn(&s.id).await,
        Err(TurnError::AlreadyRunning)
    ));
    std::fs::remove_dir_all(&dir).ok();
}
