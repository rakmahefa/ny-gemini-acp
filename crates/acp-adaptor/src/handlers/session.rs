//! ACP session lifecycle handlers.
//!
//! Lifecycle invariants stay in `agent_runtime::SessionManager`; this layer owns
//! ACP request/response and replay presentation. Persisted history is replayed from
//! structured `HistoryEntry` values so tool identifiers survive save/load unchanged.

use std::collections::HashMap;

use agent_client_protocol::schema::v1::*;
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError, Responder};

use crate::config::config_options::build_config_options;
use crate::config::mcp::normalize_servers;
use agent_runtime::state::{HistoryEntry, SessionMode as RuntimeSessionMode};
use agent_runtime::{AppState, SessionManager, ToolUiModel};
use tools_provider::tools::tool_ux::{result_update, ToolInfo};

// P-07 : une seule validation d'id de session — celle du runtime
// (`SessionManager::validate_id`), réutilisée par tous les handlers.
pub(crate) fn is_valid_session_id(id: &str) -> bool {
    SessionManager::validate_id(id).is_ok()
}

fn session_id_error(id: &SessionId) -> AcpError {
    AcpError::invalid_params().data(serde_json::json!({
        "session_id": id.to_string(),
        "error": "invalid session id"
    }))
}

fn mcp_config_error(id: &SessionId, error: &str) -> AcpError {
    tracing::error!(session=%id,error,"forwarded MCP configuration rejected");
    AcpError::invalid_params().data(serde_json::json!({
        "session_id": id.to_string(),
        "error": "MCP configuration rejected",
        "mcp_error": error
    }))
}

async fn configure_mcp(
    state: &AppState,
    session_id: &str,
    servers: Vec<McpServer>,
    session_cwd: &std::path::Path,
) -> Result<(), String> {
    let servers = normalize_servers(servers, session_cwd)?;
    state.sessions.configure_mcp(session_id, servers).await
}

fn session_mode_id(mode: RuntimeSessionMode) -> SessionModeId {
    SessionModeId::from(match mode {
        RuntimeSessionMode::Default => "default",
        RuntimeSessionMode::AcceptEdits => "accept_edits",
        RuntimeSessionMode::BypassPermissions => "bypass_permissions",
    })
}

fn build_available_modes() -> Vec<SessionMode> {
    RuntimeSessionMode::all()
        .iter()
        .map(|mode| {
            SessionMode::new(session_mode_id(*mode), mode.display_name())
                .description(mode.description())
        })
        .collect()
}

fn build_mode_state(current: RuntimeSessionMode) -> SessionModeState {
    SessionModeState::new(session_mode_id(current), build_available_modes())
}

fn send_restored_title(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    title: Option<&str>,
) -> Result<(), AcpError> {
    let Some(title) = title else {
        return Ok(());
    };
    cx.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title(title.to_owned())),
    ))?;
    Ok(())
}

struct ReplayTool<'a> {
    id: &'a str,
    name: &'a str,
    arguments: &'a serde_json::Value,
    result_text: Option<&'a str>,
    result_ok: Option<bool>,
    cwd: &'a std::path::Path,
}

fn replay_tool_result(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    replay: ReplayTool<'_>,
) -> Result<(), AcpError> {
    let info = ToolInfo::build(replay.name, replay.arguments, replay.cwd, None);
    // D-09 : le replay se base sur le champ structuré persisté `is_ok` — la
    // détection par sous-chaînes françaises était inatteignable (result_text et
    // result_ok sortent du même tuple) et matchait des chaînes jamais produites.
    let is_ok = replay.result_ok.unwrap_or(false);

    let initial = ToolUiModel::pending(
        info.kind,
        info.title.clone(),
        info.title.clone(),
        replay.arguments.clone(),
    )
    .with_content(info.content.clone())
    .with_locations(info.locations.clone());
    let initial = if replay.result_text.is_some() {
        initial.completed(is_ok, None)
    } else {
        initial.running()
    };

    crate::prompt::notify::notify_tool_call(cx, session_id, replay.id, &initial)?;

    if let Some(result_text) = replay.result_text {
        let rendered = result_update(
            replay.name,
            replay.arguments,
            result_text,
            is_ok,
            replay.cwd,
            None,
        );
        let result_ui = ToolUiModel::pending(
            info.kind,
            info.title,
            "tool result",
            replay.arguments.clone(),
        )
        .with_content(rendered.content)
        .with_locations(rendered.locations)
        .completed(is_ok, Some(serde_json::json!({ "text": result_text })));

        crate::prompt::notify::notify_tool_call_update(
            cx,
            session_id,
            replay.id,
            &result_ui,
        )?;
    }
    Ok(())
}

pub async fn handle_new(
    req: NewSessionRequest,
    responder: Responder<NewSessionResponse>,
    state: &AppState,
) -> Result<(), AcpError> {
    let session = match state
        .sessions
        .create(
            req.cwd.clone(),
            req.additional_directories.clone(),
            &state.config.default_model,
        )
        .await
    {
        Ok(session) => session,
        Err(error) => {
            return responder.respond_with_internal_error(format!("session creation: {error:#}"))
        }
    };
    let session_id = SessionId::from(session.id.clone());
    if let Err(error) = configure_mcp(state, &session.id, req.mcp_servers, &session.cwd).await {
        if let Err(cleanup) = state.sessions.delete(&session.id).await {
            tracing::error!(session=%session.id,%cleanup,"failed to clean up session after MCP setup failure");
        }
        return responder.respond_with_error(mcp_config_error(&session_id, &error));
    }
    responder.respond(
        NewSessionResponse::new(session_id)
            .config_options(build_config_options(
                &session.model,
                session.think,
                session.tools_enabled,
            ))
            .modes(build_mode_state(session.mode)),
    )
}

pub async fn handle_list(
    req: ListSessionsRequest,
    responder: Responder<ListSessionsResponse>,
    state: &AppState,
) -> Result<(), AcpError> {
    let sessions = match state.sessions.list(req.cwd.as_deref()).await {
        Ok(sessions) => sessions,
        Err(error) => {
            return responder.respond_with_internal_error(format!("session listing: {error:#}"))
        }
    };
    let infos = sessions
        .into_iter()
        .map(|session| {
            SessionInfo::new(SessionId::from(session.id), session.cwd)
                .additional_directories(session.additional_directories)
                .title(session.title)
                .updated_at(Some(session.updated_at))
        })
        .collect();
    responder.respond(ListSessionsResponse::new(infos))
}

pub async fn handle_load(
    req: LoadSessionRequest,
    responder: Responder<LoadSessionResponse>,
    state: &AppState,
    cx: &ConnectionTo<Client>,
) -> Result<(), AcpError> {
    if !is_valid_session_id(&req.session_id.0) {
        return responder.respond_with_error(session_id_error(&req.session_id));
    }
    let session = match state.sessions.load(&req.session_id.0, &req.cwd).await {
        Ok(session) => session,
        Err(error) => {
            return responder.respond_with_error(AcpError::invalid_params().data(
                serde_json::json!({
                    "session_id": req.session_id.to_string(),
                    "error": format!("session not found or workspace mismatch: {error:#}")
                }),
            ))
        }
    };
    // D-13 : comme `session/delete` et `session/close`, un `session/load`
    // doit attendre la fin d'un turn éventuel — sinon le replay s'entrelace
    // avec les notifications du turn en cours.
    if let Err(error) = state.turns.cancel_and_wait(&req.session_id.0).await {
        return responder.respond_with_error(AcpError::invalid_params().data(
            serde_json::json!({
                "session_id": req.session_id.to_string(),
                "error": error.to_string(),
            }),
        ));
    }
    if let Err(error) = configure_mcp(state, &req.session_id.0, req.mcp_servers, &session.cwd).await
    {
        state.sessions.clear_mcp(&req.session_id.0).await;
        return responder.respond_with_error(mcp_config_error(&req.session_id, &error));
    }

    send_restored_title(cx, &req.session_id, session.title.as_deref())?;

    let entries = session.messages.entries();
    let mut results_by_id: HashMap<String, (String, String, bool)> = HashMap::new();
    for entry in &entries {
        if let HistoryEntry::ToolResult {
            id,
            name,
            content,
            is_ok,
        } = entry
        {
            results_by_id.insert(id.clone(), (name.clone(), content.clone(), *is_ok));
        }
    }

    for (index, entry) in entries.iter().enumerate() {
        match entry {
            HistoryEntry::User { content } => {
                let chunk =
                    ContentChunk::new(ContentBlock::Text(TextContent::new(content.clone())))
                        .message_id(MessageId::from(format!("msg_{index}")));
                cx.send_notification(SessionNotification::new(
                    req.session_id.clone(),
                    SessionUpdate::UserMessageChunk(chunk),
                ))?;
            }
            HistoryEntry::Assistant { content } => {
                if content.trim().is_empty() {
                    continue;
                }
                let chunk =
                    ContentChunk::new(ContentBlock::Text(TextContent::new(content.clone())))
                        .message_id(MessageId::from(format!("msg_{index}")));
                cx.send_notification(SessionNotification::new(
                    req.session_id.clone(),
                    SessionUpdate::AgentMessageChunk(chunk),
                ))?;
            }
            HistoryEntry::ToolCall {
                id,
                name,
                arguments,
            } => {
                let result = results_by_id.get(id);
                let (result_text, result_ok) = result
                    .map(|(_, content, is_ok)| (Some(content.as_str()), Some(*is_ok)))
                    .unwrap_or((None, None));
                replay_tool_result(
                    cx,
                    &req.session_id,
                    ReplayTool {
                        id,
                        name,
                        arguments,
                        result_text,
                        result_ok,
                        cwd: &session.cwd,
                    },
                )?;
            }
            HistoryEntry::ToolResult { .. } => {}
        }
    }

    responder.respond(
        LoadSessionResponse::new()
            .config_options(build_config_options(
                &session.model,
                session.think,
                session.tools_enabled,
            ))
            .modes(build_mode_state(session.mode)),
    )
}

pub async fn handle_resume(
    req: ResumeSessionRequest,
    responder: Responder<ResumeSessionResponse>,
    state: &AppState,
    cx: &ConnectionTo<Client>,
) -> Result<(), AcpError> {
    if !is_valid_session_id(&req.session_id.0) {
        return responder.respond_with_error(session_id_error(&req.session_id));
    }
    let session = match state.sessions.resume(&req.session_id.0, &req.cwd).await {
        Ok(session) => session,
        Err(error) => {
            return responder.respond_with_error(AcpError::invalid_params().data(
                serde_json::json!({
                    "session_id": req.session_id.to_string(),
                    "error": format!("session not found or workspace mismatch: {error:#}")
                }),
            ))
        }
    };
    // D-13 : même garde que pour `session/load`.
    if let Err(error) = state.turns.cancel_and_wait(&req.session_id.0).await {
        return responder.respond_with_error(AcpError::invalid_params().data(
            serde_json::json!({
                "session_id": req.session_id.to_string(),
                "error": error.to_string(),
            }),
        ));
    }
    if let Err(error) = configure_mcp(state, &req.session_id.0, req.mcp_servers, &session.cwd).await
    {
        state.sessions.clear_mcp(&req.session_id.0).await;
        return responder.respond_with_error(mcp_config_error(&req.session_id, &error));
    }
    send_restored_title(cx, &req.session_id, session.title.as_deref())?;
    responder.respond(
        ResumeSessionResponse::new()
            .config_options(build_config_options(
                &session.model,
                session.think,
                session.tools_enabled,
            ))
            .modes(build_mode_state(session.mode)),
    )
}

pub async fn handle_delete(
    req: DeleteSessionRequest,
    responder: Responder<DeleteSessionResponse>,
    state: &AppState,
) -> Result<(), AcpError> {
    if !is_valid_session_id(&req.session_id.0) {
        return responder.respond_with_error(session_id_error(&req.session_id));
    }
    state
        .turns
        .cancel_and_wait(&req.session_id.0)
        .await
        .map_err(|error| {
            AcpError::invalid_params().data(serde_json::json!({
                "session_id": req.session_id.to_string(),
                "error": error.to_string(),
            }))
        })?;
    match state.sessions.delete(&req.session_id.0).await {
        Ok(true) => responder.respond(DeleteSessionResponse::new()),
        Ok(false) => {
            responder.respond_with_error(AcpError::invalid_params().data(serde_json::json!({
                "session_id": req.session_id.to_string(), "error": "session not found"
            })))
        }
        Err(error) => {
            responder.respond_with_internal_error(format!("session deletion: {error:#}"))
        }
    }
}

pub async fn handle_close(
    req: CloseSessionRequest,
    responder: Responder<CloseSessionResponse>,
    state: &AppState,
) -> Result<(), AcpError> {
    if !is_valid_session_id(&req.session_id.0) {
        return responder.respond_with_error(session_id_error(&req.session_id));
    }
    state
        .turns
        .cancel_and_wait(&req.session_id.0)
        .await
        .map_err(|error| {
            AcpError::invalid_params().data(serde_json::json!({
                "session_id": req.session_id.to_string(),
                "error": error.to_string(),
            }))
        })?;
    match state.sessions.close(&req.session_id.0).await {
        Ok(true) => responder.respond(CloseSessionResponse::new()),
        Ok(false) => {
            responder.respond_with_error(AcpError::invalid_params().data(serde_json::json!({
                "session_id": req.session_id.to_string(), "error": "session not found"
            })))
        }
        Err(error) => {
            responder.respond_with_internal_error(format!("session close: {error:#}"))
        }
    }
}

pub async fn handle_set_mode(
    req: SetSessionModeRequest,
    responder: Responder<SetSessionModeResponse>,
    state: &AppState,
    cx: &ConnectionTo<Client>,
) -> Result<(), AcpError> {
    let Some(new_mode) = RuntimeSessionMode::from_str_lossy(&req.mode_id.0) else {
        let valid = RuntimeSessionMode::all()
            .iter()
            .map(|mode| session_mode_id(*mode).0.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return responder.respond_with_error(AcpError::invalid_params().data(serde_json::json!({
            "mode_id": req.mode_id.to_string(),
            "error": format!("invalid mode_id. Valid modes: {valid}")
        })));
    };
    let updated = match state.sessions.set_mode(&req.session_id.0, new_mode).await {
        Ok(session) => session,
        Err(error) => {
            return responder.respond_with_error(AcpError::invalid_params().data(
                serde_json::json!({
                    "session_id": req.session_id.to_string(),
                    "error": format!("failed to set mode: {error:#}")
                }),
            ))
        }
    };
    cx.send_notification(SessionNotification::new(
        req.session_id.clone(),
        SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(session_mode_id(updated.mode))),
    ))?;
    responder.respond(SetSessionModeResponse::new())
}

pub async fn handle_fork(
    req: ForkSessionRequest,
    responder: Responder<ForkSessionResponse>,
    state: &AppState,
) -> Result<(), AcpError> {
    if !is_valid_session_id(&req.session_id.0) {
        return responder.respond_with_error(session_id_error(&req.session_id));
    }
    // D-13 : même garde que pour `session/load` — le fork copie l'état persisté
    // pendant qu'un turn pourrait encore l'écrire.
    if let Err(error) = state.turns.cancel_and_wait(&req.session_id.0).await {
        return responder.respond_with_error(AcpError::invalid_params().data(
            serde_json::json!({
                "session_id": req.session_id.to_string(),
                "error": error.to_string(),
            }),
        ));
    }
    let forked = match state.sessions.fork(&req.session_id.0).await {
        Ok(forked) => forked,
        Err(error) => {
            return responder.respond_with_internal_error(format!("session fork: {error:#}"))
        }
    };
    responder.respond(ForkSessionResponse::new(SessionId::from(forked.id)))
}
