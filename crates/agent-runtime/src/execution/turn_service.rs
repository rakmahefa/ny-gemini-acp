use std::sync::Arc;

use super::TurnExecutionRequest;
use crate::state::{Session, Store, StoreError};
use crate::{AgentLoop, AgentLoopConfig, AgentLoopError, LlmProvider, ToolProvider};

/// Result of one provider-neutral turn execution.
#[derive(Debug)]
pub struct TurnExecutionResult {
    pub outcome: crate::AgentLoopOutcome,
    pub session: Session,
}

/// Error returned by the runtime turn execution service.
#[derive(Debug, thiserror::Error)]
pub enum TurnServiceError {
    #[error("agent loop failed: {0}")]
    Agent(#[from] AgentLoopError),
    #[error("turn finalization failed: {0}")]
    Persistence(#[from] StoreError),
    #[error("agent loop failed and turn finalization also failed: agent={agent}; persistence={persistence}")]
    AgentAndPersistence {
        agent: AgentLoopError,
        persistence: StoreError,
    },
}

/// Owns the runtime portion of a turn once the session has been acquired.
#[derive(Clone)]
pub struct TurnService {
    store: Arc<Store>,
    llm: Arc<dyn LlmProvider>,
    tools: Arc<dyn ToolProvider>,
    config: AgentLoopConfig,
}

impl TurnService {
    pub fn new(
        store: Arc<Store>,
        llm: Arc<dyn LlmProvider>,
        tools: Arc<dyn ToolProvider>,
        config: AgentLoopConfig,
    ) -> Self {
        Self {
            store,
            llm,
            tools,
            config,
        }
    }

    /// Executes one already-acquired turn.
    pub async fn run_started(
        &self,
        request: TurnExecutionRequest<'_>,
    ) -> Result<TurnExecutionResult, TurnServiceError> {
        let TurnExecutionRequest {
            session_id,
            session,
            generation,
            references,
            cancellation,
            semantic,
            action_handler,
            permission_handler,
            build_prompt,
        } = request;

        let mut session = session;
        let agent_loop = match AgentLoop::new(self.llm.clone(), self.tools.clone(), self.config) {
            Ok(agent_loop) => {
                let agent_loop = match action_handler {
                    Some(handler) => agent_loop.with_action_handler(handler),
                    None => agent_loop,
                };
                match permission_handler {
                    Some(handler) => agent_loop.with_permission_handler(handler),
                    None => agent_loop,
                }
            }
            Err(error) => {
                let finalization = self.finish(&session_id, session, generation).await;
                if !semantic.is_terminal() {
                    let _ = semantic.turn_started();
                    let _ = semantic.turn_failed();
                }
                return match finalization {
                    Ok(()) => Err(error.into()),
                    Err(persistence) => Err(TurnServiceError::AgentAndPersistence {
                        agent: error,
                        persistence,
                    }),
                };
            }
        };

        if cancellation.is_cancelled() {
            let semantic_ok = if semantic.is_terminal() {
                true
            } else {
                semantic.turn_started() && semantic.turn_cancelled()
            };
            let finalization = self.finish(&session_id, session, generation).await;
            if !semantic_ok {
                return match finalization {
                    Ok(()) => Err(AgentLoopError::SemanticEventRejected.into()),
                    Err(persistence) => Err(TurnServiceError::AgentAndPersistence {
                        agent: AgentLoopError::SemanticEventRejected,
                        persistence,
                    }),
                };
            }
            return match finalization {
                Ok(()) => Err(AgentLoopError::Cancelled.into()),
                Err(persistence) => Err(TurnServiceError::AgentAndPersistence {
                    agent: AgentLoopError::Cancelled,
                    persistence,
                }),
            };
        }

        let result = agent_loop
            .run(
                &mut session,
                &references,
                cancellation,
                semantic,
                build_prompt,
            )
            .await;

        match result {
            Ok(outcome) => {
                self.finish(&session_id, session, generation).await?;
                // Read back the canonical state after commit so callers never
                // observe a pre-finalization snapshot (turn_count/updated_at
                // and any normalized history changes are included).
                let committed = self.store.get(&session_id).await.ok_or_else(|| {
                    StoreError::Persistence(format!(
                        "session {} disappeared after successful turn commit",
                        session_id
                    ))
                })?;
                Ok(TurnExecutionResult {
                    outcome,
                    session: committed,
                })
            }
            Err(error) => {
                if !semantic.is_terminal() {
                    if matches!(error, AgentLoopError::Cancelled) {
                        let _ = semantic.turn_cancelled();
                    } else {
                        let _ = semantic.turn_failed();
                    }
                }
                match self.finish(&session_id, session, generation).await {
                    Ok(()) => Err(error.into()),
                    Err(persistence) => Err(TurnServiceError::AgentAndPersistence {
                        agent: error,
                        persistence,
                    }),
                }
            }
        }
    }

    async fn finish(
        &self,
        session_id: &str,
        session: Session,
        generation: u64,
    ) -> Result<(), StoreError> {
        self.store.end_turn(session_id, session, generation).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{EventBus, TurnEventEmitter};
    use crate::providers::{LlmError, ModelEvent, ModelRequest};
    use crate::state::HistoryEntry;
    use tokio::sync::mpsc;

    struct TextProvider;

    #[async_trait::async_trait]
    impl LlmProvider for TextProvider {
        async fn stream(&self, _: ModelRequest) -> Result<crate::LlmStream, LlmError> {
            let (tx, rx) = mpsc::channel(4);
            tx.send(Ok(ModelEvent::TextDelta("hello from runtime".into())))
                .await
                .unwrap();
            drop(tx);
            Ok(rx)
        }

        async fn upload_image(&self, _: &str, _: &str) -> Result<String, LlmError> {
            Err(LlmError::Unavailable("images unsupported in test".into()))
        }

        fn model_info(&self, _: &str) -> crate::LlmModelInfo {
            crate::LlmModelInfo::default()
        }
    }

    fn build_prompt_empty(_: &Session, _: &dyn ToolProvider) -> String {
        String::new()
    }

    #[test]
    fn service_is_provider_neutral() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TurnService>();
    }

    #[tokio::test]
    async fn successful_turn_persists_and_terminalizes_semantics() {
        let dir = std::env::temp_dir().join(format!(
            "acp-turn-service-test-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = Arc::new(Store::open(&dir).await.unwrap());
        let session = store.create("/tmp".into(), vec![], "test-model").await.unwrap();
        let (started, generation) = store.begin_turn(&session.id).await.unwrap();
        let bus = EventBus::new();
        let _projection = bus.subscribe_turn("turn-test");
        let mut semantic = TurnEventEmitter::new_with_required_transport(
            bus.clone(),
            session.id.clone(),
            "turn-test",
        );
        let service = TurnService::new(
            store.clone(),
            Arc::new(TextProvider),
            Arc::new(crate::NullToolProvider),
            AgentLoopConfig::default(),
        );

        let result = service
            .run_started(TurnExecutionRequest {
                session_id: session.id.clone(),
                session: started,
                generation,
                references: Vec::new(),
                cancellation: crate::Cancellation::new(),
                semantic: &mut semantic,
                action_handler: None,
                permission_handler: None,
                build_prompt: build_prompt_empty,
            })
            .await
            .unwrap();

        assert_eq!(result.outcome.output, "hello from runtime");
        assert_eq!(result.outcome.rounds, 1);
        assert!(semantic.is_terminal());
        assert_eq!(result.session.turn_count, 1);

        let persisted = store.get(&session.id).await.unwrap();
        assert!(persisted.messages.entries().iter().any(|entry| {
            matches!(entry, HistoryEntry::Assistant { content } if content == "hello from runtime")
        }));
        assert_eq!(result.session.updated_at, persisted.updated_at);
        assert_eq!(result.session.turn_count, persisted.turn_count);

        bus.close_turn("turn-test");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn pre_cancelled_turn_has_started_and_cancelled_semantics() {
        let dir = std::env::temp_dir().join(format!(
            "acp-turn-service-pre-cancel-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let store = Arc::new(Store::open(&dir).await.unwrap());
        let session = store.create("/tmp".into(), vec![], "test-model").await.unwrap();
        let (started, generation) = store.begin_turn(&session.id).await.unwrap();
        let bus = EventBus::new();
        let mut receiver = bus.subscribe_turn("turn-pre-cancel");
        let mut semantic = TurnEventEmitter::new_with_required_transport(
            bus.clone(),
            session.id.clone(),
            "turn-pre-cancel",
        );
        let service = TurnService::new(
            store.clone(),
            Arc::new(TextProvider),
            Arc::new(crate::NullToolProvider),
            AgentLoopConfig::default(),
        );
        let cancellation = crate::Cancellation::new();
        cancellation.cancel();

        let result = service
            .run_started(TurnExecutionRequest {
                session_id: session.id.clone(),
                session: started,
                generation,
                references: Vec::new(),
                cancellation,
                semantic: &mut semantic,
                action_handler: None,
                permission_handler: None,
                build_prompt: build_prompt_empty,
            })
            .await;

        assert!(matches!(result, Err(TurnServiceError::Agent(AgentLoopError::Cancelled))));
        assert!(semantic.is_terminal());
        let first = receiver.recv().await.unwrap();
        let second = receiver.recv().await.unwrap();
        assert!(matches!(first, crate::events::SemanticEvent::TurnStarted { .. }));
        assert!(matches!(second, crate::events::SemanticEvent::TurnCancelled { .. }));

        bus.close_turn("turn-pre-cancel");
        std::fs::remove_dir_all(&dir).ok();
    }
}
