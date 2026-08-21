use std::sync::Arc;

use agent_runtime::state::{Session, Store};

pub(crate) struct TurnGuard {
    store: Arc<Store>,
    session_id: String,
    session: Option<Session>,
    finished: bool,
    generation: u64,
}

impl TurnGuard {
    pub(crate) fn new(store: Arc<Store>, session_id: String, session: Session, generation: u64) -> Self {
        Self { store, session_id, session: Some(session), finished: false, generation }
    }

    pub(crate) fn session_mut(&mut self) -> &mut Session {
        self.session.as_mut().expect("TurnGuard: session déjà consommée")
    }

    pub(crate) async fn finish(mut self) {
        if let Some(session) = self.session.take() {
            if let Err(error) = self.store.end_turn(&self.session_id, session, self.generation).await {
                tracing::warn!(session=%self.session_id, "end_turn a échoué dans TurnGuard: {error}");
            }
        }
        self.finished = true;
    }
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        if self.finished { return; }
        let sid = self.session_id.clone();
        let store = self.store.clone();
        let generation = self.generation;
        if let Some(session) = self.session.take() {
            tokio::spawn(async move {
                if let Err(error) = store.end_turn(&sid, session, generation).await {
                    tracing::warn!(session=%sid, "TurnGuard::drop: persistence finalization failed safely: {error}");
                }
            });
        } else {
            tracing::warn!(session=%self.session_id, "TurnGuard dropped after session ownership was already consumed");
        }
    }
}
