use super::follow_up::{request_action, FollowUpError, FollowUpOutcome};
use agent_client_protocol::schema::v1::SessionId;
use agent_client_protocol::{Client, ConnectionTo};
use agent_runtime::{AgentActionHandler, Cancellation};
use serde_json::Value;
use std::sync::Arc;

pub(crate) struct AcpActionHandler {
    cx: ConnectionTo<Client>,
    session_id: SessionId,
}

impl AcpActionHandler {
    pub(crate) fn new(cx: ConnectionTo<Client>, session_id: SessionId) -> Self {
        Self { cx, session_id }
    }
}

#[async_trait::async_trait]
impl AgentActionHandler for AcpActionHandler {
    fn supports(&self, name: &str) -> bool {
        name.eq_ignore_ascii_case("FollowUp")
    }

    async fn handle(
        &self,
        _session_id: &str,
        call_id: &str,
        name: &str,
        arguments: Value,
        cancellation: Cancellation,
    ) -> Result<Option<String>, String> {
        if !self.supports(name) {
            return Err(format!("unsupported agent action: {name}"));
        }
        let label = arguments
            .get("label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let query = arguments
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        let mut cancel = cancellation.subscribe();
        match request_action(&self.cx, &self.session_id, call_id, label, query, &mut cancel).await {
            Ok(FollowUpOutcome::Selected(text)) => Ok(Some(text)),
            Ok(FollowUpOutcome::Rejected) => Ok(None),
            Ok(FollowUpOutcome::Cancelled) => Err("FollowUp cancelled".to_owned()),
            Err(FollowUpError::InvalidInput(message)) => Err(message.to_owned()),
            Err(error) => Err(error.to_string()),
        }
    }
}

pub(crate) fn shared(cx: ConnectionTo<Client>, session_id: SessionId) -> Arc<AcpActionHandler> {
    Arc::new(AcpActionHandler::new(cx, session_id))
}
