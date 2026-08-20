use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    ToolCall as AcpToolCall, ToolCallStatus, ToolCallUpdate,
};
use serde_json::{json, Map, Value};

use super::super::tool_ux::bounded_raw_input;
use super::{PermissionKind, PermissionRequest, PermissionResult, ToolExecutor};

impl<'a> ToolExecutor<'a> {
    pub async fn request_permission(
        &self,
        request: &PermissionRequest,
        call_id: &agent_client_protocol::schema::v1::ToolCallId,
    ) -> PermissionResult {
        // The terminal resource does not exist yet at permission time. For shell_exec,
        // it is created only after the user grants permission by the ACP terminal request.
        // Therefore the permission prompt must never advertise a Terminal content block.
        let info = super::super::tool_ux::ToolInfo::build(
            &request.tool_name,
            &request.arguments,
            self.cwd,
            None,
        );
        let tool_call = AcpToolCall::new(call_id.clone(), request.summary.clone())
            .kind(match request.kind {
                PermissionKind::Read => agent_client_protocol::schema::v1::ToolKind::Read,
                PermissionKind::Write => agent_client_protocol::schema::v1::ToolKind::Edit,
                PermissionKind::Execute => agent_client_protocol::schema::v1::ToolKind::Execute,
                PermissionKind::Network => agent_client_protocol::schema::v1::ToolKind::Fetch,
            })
            .status(ToolCallStatus::Pending)
            .content(info.content)
            .locations(info.locations)
            .raw_input(bounded_raw_input(&request.arguments))
            .meta(permission_meta(request));
        let options = vec![
            PermissionOption::new(
                "allow_once",
                "Autoriser cette fois",
                PermissionOptionKind::AllowOnce,
            ),
            PermissionOption::new(
                "allow_always",
                "Toujours autoriser",
                PermissionOptionKind::AllowAlways,
            ),
            PermissionOption::new("reject_once", "Refuser", PermissionOptionKind::RejectOnce),
            PermissionOption::new(
                "reject_always",
                "Toujours refuser",
                PermissionOptionKind::RejectAlways,
            ),
        ];
        let rpc = RequestPermissionRequest::new(
            self.session_id.clone(),
            ToolCallUpdate::from(tool_call),
            options,
        )
        .meta(permission_meta(request));
        tracing::info!(session=%self.session_id, tool=%request.tool_name, kind=?request.kind, risk=%request.risk, summary=%request.summary, detail=%request.detail, warnings=?request.warnings, "envoi session/request_permission");

        let response = tokio::select! {
            response = self.cx.send_request(rpc).block_task() => match response { Ok(response) => response, Err(error) => return PermissionResult::TransportError(error.to_string()) },
            _ = super::super::lifecycle::wait_for_session_cancel(self.session_id.0.as_ref()) => return PermissionResult::Cancelled,
        };
        match response.outcome {
            RequestPermissionOutcome::Cancelled => PermissionResult::Cancelled,
            RequestPermissionOutcome::Selected(selected) => match selected.option_id.0.as_ref() {
                "allow_once" | "allow_always" => PermissionResult::Allow,
                "reject_once" | "reject_always" => PermissionResult::Reject,
                unknown => PermissionResult::TransportError(format!(
                    "option de permission ACP inconnue: {unknown}"
                )),
            },
            _ => PermissionResult::TransportError("outcome de permission ACP non reconnu".into()),
        }
    }
}

fn permission_meta(request: &PermissionRequest) -> Map<String, Value> {
    let mut meta = Map::new();
    meta.insert("claudeCode".into(), json!({ "toolName": request.tool_name, "permission": { "kind": request.kind.label(), "risk": request.risk.label(), "warnings": request.warnings } }));
    meta
}

impl PermissionKind {
    pub fn label(&self) -> &'static str {
        match self {
            PermissionKind::Read => "read",
            PermissionKind::Write => "write",
            PermissionKind::Execute => "execute",
            PermissionKind::Network => "network",
        }
    }
}
