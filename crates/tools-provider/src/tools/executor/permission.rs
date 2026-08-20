use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    ToolCall as AcpToolCall, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolKind,
};
use serde_json::{json, Map, Value};

use super::super::sandbox::{RiskLevel, ShellAnalysis};
use super::super::tool_ux::{bounded_raw_input, classify_risk, ToolInfo};
use super::ToolExecutor;

#[derive(Debug, Clone)]
pub struct PermissionRequest {
    pub kind: PermissionKind,
    pub risk: RiskLevel,
    pub summary: String,
    pub detail: String,
    pub tool_name: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionKind {
    Read,
    Write,
    Execute,
    #[allow(dead_code)]
    Network,
}

impl PermissionRequest {
    pub fn from_tool_call(tool_name: &str, args: &Value, cwd: &std::path::Path) -> Self {
        let info = ToolInfo::build(
            tool_name,
            args,
            cwd,
            (tool_name == "shell_exec").then(|| "permission-preview").as_deref(),
        );
        let kind = match info.kind {
            ToolKind::Read | ToolKind::Search => PermissionKind::Read,
            ToolKind::Edit => PermissionKind::Write,
            ToolKind::Execute => PermissionKind::Execute,
            _ => PermissionKind::Execute,
        };
        let risk = classify_risk(tool_name, args);
        let mut warnings = Vec::new();

        match tool_name {
            "file_write" | "file_edit" | "replace_in_file" => {
                if let Some(path) = args.get("path").and_then(Value::as_str) {
                    let resolved = if Path::new(path).is_absolute() {
                        PathBuf::from(path)
                    } else {
                        cwd.join(path)
                    };
                    if resolved.exists() {
                        warnings.push(format!(
                            "Le fichier '{}' existe déjà et sera modifié.",
                            path
                        ));
                    }
                }
            }
            "shell_exec" => {
                let command = args.get("command").and_then(Value::as_str).unwrap_or("");
                let analysis = ShellAnalysis::analyze(command);
                if analysis.has_dangerous_pipe_chain {
                    warnings
                        .push("Chaîne de commandes potentiellement dangereuse détectée.".into());
                }
                if analysis.has_env_injection {
                    warnings.push("Injection de variables d'environnement détectée.".into());
                }
                if analysis.risk >= RiskLevel::High {
                    warnings.push(format!(
                        "Niveau de risque {} : {}",
                        analysis.risk.emoji(),
                        analysis.risk.description()
                    ));
                }
            }
            _ => {}
        }
        if risk >= RiskLevel::High {
            warnings.push("Cette opération peut avoir des effets irréversibles.".into());
        }

        let detail = if warnings.is_empty() {
            format!("{}\n{} {}", info.title, risk.emoji(), risk.label())
        } else {
            format!(
                "{}\n{} {}\n\nAvertissements :\n{}",
                info.title,
                risk.emoji(),
                risk.label(),
                warnings
                    .iter()
                    .map(|w| format!("  - {w}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        };
        Self {
            kind,
            risk,
            summary: info.title,
            detail,
            tool_name: tool_name.to_owned(),
            warnings,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionResult {
    Allow,
    Reject,
    Cancelled,
    TransportError(String),
}

impl<'a> ToolExecutor<'a> {
    pub async fn request_permission(
        &self,
        request: &PermissionRequest,
        call_id: &ToolCallId,
    ) -> PermissionResult {
        let terminal_id = (request.tool_name == "shell_exec")
            .then(|| format!("terminal-{call_id}"));
        let info = ToolInfo::build(
            &request.tool_name,
            &serde_json::Value::Object(serde_json::Map::new()),
            self.cwd,
            terminal_id.as_deref(),
        );
        let tool_call = AcpToolCall::new(call_id.clone(), request.summary.clone())
            .kind(match request.kind {
                PermissionKind::Read => ToolKind::Read,
                PermissionKind::Write => ToolKind::Edit,
                PermissionKind::Execute => ToolKind::Execute,
                PermissionKind::Network => ToolKind::Fetch,
            })
            .status(ToolCallStatus::Pending)
            .content(info.content)
            .locations(info.locations)
            .raw_input(bounded_raw_input(&serde_json::json!({"tool": request.tool_name})))
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
