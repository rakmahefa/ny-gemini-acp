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
        let Some(session) = self.session.take() else {
            self.finished = true;
            return;
        };

        match self.store.end_turn(&self.session_id, session.clone(), self.generation).await {
            Ok(()) => {
                self.finished = true;
            }
            Err(error) => {
                tracing::warn!(session=%self.session_id, "end_turn a échoué; le Drop conservera une finalisation best-effort: {error}");
                self.session = Some(session);
            }
        }
    }
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        if self.finished { return; }
        let sid = self.session_id.clone();
        let store = self.store.clone();
        let generation = self.generation;
        let Some(session) = self.session.take() else { return; };

        tokio::spawn(async move {
            if let Err(error) = store.end_turn(&sid, session, generation).await {
                tracing::error!(session=%sid, "TurnGuard::drop: finalisation de persistance échouée: {error}");
            }
        });
    }
}
