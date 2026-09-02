use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{
    Content, ContentBlock, Diff, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
    RequestPermissionRequest, TextContent, ToolCall as AcpToolCall, ToolCallContent, ToolCallId,
    ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolKind,
};
use serde_json::{json, Map, Value};

use super::super::sandbox::{RiskLevel, ShellSandbox};
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
    pub arguments: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionKind {
    Read,
    Write,
    Execute,
}

impl PermissionRequest {
    pub fn from_tool_call(tool_name: &str, args: &Value, cwd: &std::path::Path) -> Self {
        let info = ToolInfo::build(tool_name, args, cwd, None);
        let kind = match info.kind {
            agent_runtime::ToolUiKind::FileRead
            | agent_runtime::ToolUiKind::Search
            | agent_runtime::ToolUiKind::Glob
            | agent_runtime::ToolUiKind::DirectoryList
            | agent_runtime::ToolUiKind::SearchAndRead => PermissionKind::Read,
            agent_runtime::ToolUiKind::FileWrite
            | agent_runtime::ToolUiKind::FileEdit
            | agent_runtime::ToolUiKind::ReplaceInFile => PermissionKind::Write,
            agent_runtime::ToolUiKind::Shell => PermissionKind::Execute,
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
                // The analysis and the classification share a single entry
                // point (`ShellSandbox`): a command the sandbox would refuse
                // gets one honest warning instead of a heuristic guess.
                match ShellSandbox::new().analyze_command(command) {
                    Ok(analysis) => {
                        if analysis.has_dangerous_pipe_chain {
                            warnings.push(
                                "Chaîne de commandes potentiellement dangereuse détectée.".into(),
                            );
                        }
                        if analysis.has_env_injection {
                            warnings
                                .push("Injection de variables d'environnement détectée.".into());
                        }
                        if analysis.risk >= RiskLevel::High {
                            warnings.push(format!(
                                "Niveau de risque {} : {}",
                                analysis.risk.emoji(),
                                analysis.risk.description()
                            ));
                        }
                    }
                    Err(_) => warnings.push(
                        "Cette commande sera refusée par le filtre heuristique de la sandbox (sans confinement OS).".into(),
                    ),
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
            arguments: args.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionResult {
    Allow,
    /// The client selected "always allow": the caller is responsible for
    /// remembering the decision (session-scoped).
    AllowAlways,
    Reject(String),
    /// The client selected "always reject": the caller is responsible for
    /// remembering the decision (session-scoped).
    RejectAlways,
    Cancelled,
    TransportError(String),
}

impl<'a> ToolExecutor<'a> {
    pub async fn request_permission(
        &self,
        request: &PermissionRequest,
        call_id: &ToolCallId,
    ) -> PermissionResult {
        // The permission request is itself an ACP protocol interaction, so its presentation
        // remains explicitly projected at this boundary from the host-neutral ToolInfo.
        let info = ToolInfo::build(&request.tool_name, &request.arguments, self.cwd, None);
        let content = info
            .content
            .iter()
            .map(project_permission_content)
            .collect::<anyhow::Result<Vec<_>>>()
            .unwrap_or_else(|error| {
                tracing::warn!(error = %error, "permission content projection failed");
                Vec::new()
            });
        let locations = info
            .locations
            .iter()
            .map(project_permission_location)
            .collect::<anyhow::Result<Vec<_>>>()
            .unwrap_or_else(|error| {
                tracing::warn!(error = %error, "permission location projection failed");
                Vec::new()
            });
        let tool_call = AcpToolCall::new(call_id.clone(), request.summary.clone())
            .kind(permission_tool_kind(request.kind))
            .status(ToolCallStatus::Pending)
            .content(content)
            .locations(locations)
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
                "allow_once" => PermissionResult::Allow,
                "allow_always" => PermissionResult::AllowAlways,
                "reject_once" => PermissionResult::Reject(format!(
                    "{} ({}) refusé par l'utilisateur.",
                    request.kind.label(),
                    request.summary
                )),
                "reject_always" => PermissionResult::RejectAlways,
                unknown => PermissionResult::TransportError(format!(
                    "option de permission ACP inconnue: {unknown}"
                )),
            },
            _ => PermissionResult::TransportError("outcome de permission ACP non reconnu".into()),
        }
    }
}

fn project_permission_content(value: &Value) -> anyhow::Result<ToolCallContent> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("permission content missing type"))?;
    match kind {
        "text" => {
            let text = value
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("permission text content missing text"))?;
            Ok(ToolCallContent::Content(Content::new(ContentBlock::Text(
                TextContent::new(text.to_owned()),
            ))))
        }
        "diff" => {
            let path = value
                .get("path")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("permission diff missing path"))?;
            let old_text = value.get("old_text").and_then(Value::as_str).unwrap_or("");
            let new_text = value.get("new_text").and_then(Value::as_str).unwrap_or("");
            Ok(ToolCallContent::Diff(
                Diff::new(path.to_owned(), new_text.to_owned()).old_text(old_text.to_owned()),
            ))
        }
        other => Err(anyhow::anyhow!(
            "unsupported permission content kind: {other}"
        )),
    }
}

fn project_permission_location(value: &Value) -> anyhow::Result<ToolCallLocation> {
    let path = value
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("permission location missing path"))?;
    Ok(ToolCallLocation::new(path.to_owned()))
}

fn permission_tool_kind(kind: PermissionKind) -> ToolKind {
    match kind {
        PermissionKind::Read => ToolKind::Read,
        PermissionKind::Write => ToolKind::Edit,
        PermissionKind::Execute => ToolKind::Execute,
    }
}

fn permission_meta(request: &PermissionRequest) -> Map<String, Value> {
    let mut meta = Map::new();
    meta.insert(
        "claudeCode".into(),
        json!({ "toolName": request.tool_name, "permission": { "kind": request.kind.label(), "risk": request.risk.label(), "warnings": request.warnings } }),
    );
    meta
}

impl PermissionKind {
    pub fn label(&self) -> &'static str {
        match self {
            PermissionKind::Read => "read",
            PermissionKind::Write => "write",
            PermissionKind::Execute => "execute",
        }
    }
}
