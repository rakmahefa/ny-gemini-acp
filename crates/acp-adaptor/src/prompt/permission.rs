use std::path::Path;
use std::sync::Arc;

use agent_client_protocol::schema::v1::{SessionId, ToolKind, ToolCallId};
use agent_client_protocol::{Client, ConnectionTo};
use agent_runtime::{SessionMode, ToolPermissionDecision, ToolPermissionHandler, ToolPermissionRequest};
use tools_provider::tools::executor::{PermissionRequest, PermissionResult, ToolExecutor};
use tools_provider::tools::ToolPermissionMode;
use agent_runtime::ToolProvider;

pub(crate) struct AcpToolPermissionHandler {
    cx: ConnectionTo<Client>,
    tools: Arc<dyn ToolProvider>,
}

impl AcpToolPermissionHandler {
    pub(crate) fn new(cx: ConnectionTo<Client>, tools: Arc<dyn ToolProvider>) -> Self {
        Self { cx, tools }
    }
}

impl AcpToolPermissionHandler {
    fn permission_kind(&self, request: &ToolPermissionRequest) -> PermissionRequest {
        PermissionRequest::from_tool_call(&request.name, &request.arguments, Path::new(&request.cwd))
    }
}

#[async_trait::async_trait]
impl ToolPermissionHandler for AcpToolPermissionHandler {
    fn needs_permission(&self, session: &agent_runtime::state::Session, request: &ToolPermissionRequest) -> bool {
        let permission = self.permission_kind(request);
        match permission.kind {
            tools_provider::tools::executor::PermissionKind::Read
            | tools_provider::tools::executor::PermissionKind::Network => false,
            tools_provider::tools::executor::PermissionKind::Write
            | tools_provider::tools::executor::PermissionKind::Execute => match session.mode {
                SessionMode::BypassPermissions => false,
                SessionMode::AcceptEdits => {
                    permission.kind == tools_provider::tools::executor::PermissionKind::Execute
                        && permission.risk >= tools_provider::tools::sandbox::RiskLevel::High
                }
                SessionMode::Default => true,
            },
        }
    }

    async fn request_permission(
        &self,
        session: &agent_runtime::state::Session,
        request: &ToolPermissionRequest,
        cancellation: agent_runtime::Cancellation,
    ) -> ToolPermissionDecision {
        let mode = match session.mode {
            SessionMode::Default => ToolPermissionMode::Default,
            SessionMode::AcceptEdits => ToolPermissionMode::AcceptEdits,
            SessionMode::BypassPermissions => ToolPermissionMode::BypassPermissions,
        };
        let get_mode = move || mode;
        let session_id = SessionId::from(request.session_id.clone());
        let executor = ToolExecutor::new(
            &self.cx,
            &session_id,
            &*self.tools,
            &request.cwd,
            &request.additional_dirs,
            &get_mode,
            cancellation.clone().subscribe(),
        );
        let permission = PermissionRequest::from_tool_call(&request.name, &request.arguments, &request.cwd);
        match executor
            .request_permission(&permission, &ToolCallId::from(request.call_id.clone()))
            .await
        {
            PermissionResult::Allow => ToolPermissionDecision::Allow,
            PermissionResult::Reject => ToolPermissionDecision::Reject(
                format!("{} ({}) refusé par l'utilisateur.", permission.kind.label(), permission.summary),
            ),
            PermissionResult::Cancelled => ToolPermissionDecision::Cancelled,
            PermissionResult::TransportError(error) => ToolPermissionDecision::Reject(
                format!("Échec de la demande de permission ACP : {error}"),
            ),
        }
    }
}
