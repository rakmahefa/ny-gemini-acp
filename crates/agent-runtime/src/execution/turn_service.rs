use std::sync::Arc;

use crate::events::TurnEventSink;
use crate::state::{Session, Store};
use crate::{
    AgentActionHandler, AgentLoop, AgentLoopConfig, AgentLoopError, Cancellation,
    LlmProvider, ToolPermissionHandler, ToolProvider,
};

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
}

/// Owns the runtime portion of a turn once the session has been acquired.
///
/// ACP and other hosts remain responsible for request parsing, host-specific
/// preparation, interaction handlers, and presentation. This service owns the
/// provider-neutral execution boundary and guarantees that the acquired
/// session is finalized on every execution path.
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

    /// Executes a turn for a session that has already been acquired with
    /// `Store::begin_turn`.
    pub async fn run_started<F>(
        &self,
        session_id: &str,
        session: Session,
        generation: u64,
        references: &[String],
        cancellation: Cancellation,
        semantic: &mut dyn TurnEventSink,
        action_handler: Option<Arc<dyn AgentActionHandler>>,
        permission_handler: Option<Arc<dyn ToolPermissionHandler>>,
        build_prompt: F,
    ) -> Result<TurnExecutionResult, TurnServiceError>
    where
        F: Fn(&Session, &dyn ToolProvider) -> String + Send + Sync,
    {
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
                self.finish(session_id, session, generation).await;
                if !semantic.is_terminal() {
                    let _ = semantic.turn_started();
                    let _ = semantic.turn_failed();
                }
                return Err(error.into());
            }
        };

        let result = agent_loop
            .run(
                &mut session,
                references,
                cancellation,
                semantic,
                build_prompt,
            )
            .await;

        match result {
            Ok(outcome) => {
                let result = TurnExecutionResult {
                    outcome,
                    session: session.clone(),
                };
                self.finish(session_id, session, generation).await;
                Ok(result)
            }
            Err(error) => {
                if !semantic.is_terminal() {
                    if matches!(error, AgentLoopError::Cancelled) {
                        let _ = semantic.turn_cancelled();
                    } else {
                        let _ = semantic.turn_failed();
                    }
                }
                self.finish(session_id, session, generation).await;
                Err(error.into())
            }
        }
    }

    async fn finish(&self, session_id: &str, session: Session, generation: u64) {
        if let Err(error) = self
            .store
            .end_turn(session_id, session, generation)
            .await
        {
            tracing::warn!(session=%session_id, error=%error, "turn finalization failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_is_provider_neutral() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TurnService>();
    }
}
