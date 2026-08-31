use std::sync::Arc;

use agent_client_protocol::schema::v1::{SessionId, ToolCallId};
use agent_client_protocol::{Client, ConnectionTo};
use agent_runtime::state::SessionMode;
use agent_runtime::{
    Cancellation, ToolPermissionDecision, ToolPermissionHandler, ToolPermissionRequest,
    ToolProvider,
};
use tools_provider::tools::executor::{
    PermissionKind, PermissionRequest, PermissionResult, ToolExecutor,
};
use tools_provider::tools::ToolPermissionMode;

pub(crate) struct AcpToolPermissionHandler {
    cx: ConnectionTo<Client>,
    tools: Arc<dyn ToolProvider>,
}

impl AcpToolPermissionHandler {
    pub(crate) fn new(cx: ConnectionTo<Client>, tools: Arc<dyn ToolProvider>) -> Self {
        Self { cx, tools }
    }

    fn permission_request(&self, request: &ToolPermissionRequest) -> PermissionRequest {
        PermissionRequest::from_tool_call(&request.name, &request.arguments, &request.cwd)
    }
}

#[async_trait::async_trait]
impl ToolPermissionHandler for AcpToolPermissionHandler {
    fn needs_permission(
        &self,
        session: &agent_runtime::state::Session,
        request: &ToolPermissionRequest,
    ) -> bool {
        let permission = self.permission_request(request);
        match permission.kind {
            PermissionKind::Read => false,
            PermissionKind::Write | PermissionKind::Execute => match session.mode {
                SessionMode::BypassPermissions => false,
                SessionMode::AcceptEdits => {
                    permission.kind == PermissionKind::Execute
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
        cancellation: Cancellation,
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
        let permission = self.permission_request(request);
        match executor
            .request_permission(&permission, &ToolCallId::from(request.call_id.clone()))
            .await
        {
            PermissionResult::Allow => ToolPermissionDecision::Allow,
            PermissionResult::Reject => ToolPermissionDecision::Reject(format!(
                "{} ({}) refusé par l'utilisateur.",
                permission.kind.label(),
                permission.summary
            )),
            PermissionResult::Cancelled => ToolPermissionDecision::Cancelled,
            PermissionResult::TransportError(error) => ToolPermissionDecision::Reject(format!(
                "Échec de la demande de permission ACP : {error}"
            )),
        }
    }
}
