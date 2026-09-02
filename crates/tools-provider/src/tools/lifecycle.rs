//! Session cancellation adapter for tool execution.
//!
//! This module owns the session-cancellation map used by the live tool path
//! (`DefaultToolProvider::call` -> `registry.call_async` and the ACP
//! permission request). The former execution-state machine that lived here
//! too served only the removed provider-local execution path (SPEC-P1-05)
//! and was deleted with it.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use super::contracts::ToolCancellation;

type SessionCancellationMap = HashMap<String, ToolCancellation>;
static SESSION_CANCELLATION: OnceLock<Mutex<SessionCancellationMap>> = OnceLock::new();

fn cancellation_map() -> &'static Mutex<SessionCancellationMap> {
    SESSION_CANCELLATION.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_or_recover<T>(mutex: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            tracing::warn!("recovering poisoned tool lifecycle mutex");
            poisoned.into_inner()
        }
    }
}

pub fn bind_session_cancellation(session_id: &str, cancellation: ToolCancellation) {
    lock_or_recover(cancellation_map()).insert(session_id.to_owned(), cancellation);
}
pub fn unbind_session_cancellation(session_id: &str) {
    lock_or_recover(cancellation_map()).remove(session_id);
}
pub fn session_cancelled(session_id: &str) -> bool {
    lock_or_recover(cancellation_map())
        .get(session_id)
        .is_some_and(ToolCancellation::is_cancelled)
}
pub async fn wait_for_session_cancel(session_id: &str) {
    let cancellation = {
        let map = lock_or_recover(cancellation_map());
        map.get(session_id).cloned()
    };
    let Some(cancellation) = cancellation else {
        std::future::pending::<()>().await;
        return;
    };
    cancellation.cancelled().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_bridge_uses_provider_contract() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let cancellation = ToolCancellation::from_receiver(rx);
        bind_session_cancellation("sess-test", cancellation.clone());
        assert!(!session_cancelled("sess-test"));
        tx.send(true).unwrap();
        wait_for_session_cancel("sess-test").await;
        assert!(session_cancelled("sess-test"));
        unbind_session_cancellation("sess-test");
        assert!(!session_cancelled("sess-test"));
    }
}
