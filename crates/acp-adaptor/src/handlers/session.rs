//! ACP session lifecycle handlers.
//!
//! The handler layer deliberately delegates lifecycle invariants to
//! `agent_runtime::SessionManager`. This keeps validation, persistence and
//! user-visible error semantics consistent across new/load/resume/fork/close.
//!
//! UX principles borrowed from the mature Claude ACP adapter:
//! - session mode state is returned on every lifecycle response;
//! - loading replays history before resolving the request;
//! - the session title is restored as an explicit `session/update`;
//! - mode changes emit `CurrentModeUpdate` immediately;
//! - invalid lifecycle inputs are rejected as `invalid_params`;
//! - persisted tool_call/tool_result blocks are reconstructed into real ACP
//!   tool cards during replay instead of disappearing from the conversation.
//! - forwarded MCP servers are normalized and bound to the session before the
//!   lifecycle response is emitted.

use agent_client_protocol::schema::v1::*;
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError, Responder};

use crate::config::config_options::build_config_options;
use crate::config::mcp::normalize_servers;
use agent_runtime::state::{Role, SessionMode as AcpSessionMode};
use agent_runtime::AppState;
use tools_provider::tools::parse::parse_tool_calls;
use tools_provider::tools::tool_ux::{bounded_raw_input, result_update, ToolInfo};

fn is_valid_session_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("sess_") else { return false; };
    rest.len() == 32 && rest.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn session_id_error(id: &SessionId) -> AcpError {
    AcpError::invalid_params().data(serde_json::json!({"session_id":id.to_string(),"error":"identifiant de session invalide"}))
}
fn mcp_config_error(id: &SessionId, error: &str) -> AcpError {
    tracing::error!(session=%id,error,"forwarded MCP configuration rejected");
    AcpError::invalid_params().data(serde_json::json!({"session_id":id.to_string(),"error":"MCP configuration rejected","mcp_error":error}))
}
async fn configure_mcp(state: &AppState, session_id: &str, servers: Vec<McpServer>, session_cwd: &std::path::Path) -> Result<(), String> {
    let servers = normalize_servers(servers, session_cwd)?;
    state.sessions.configure_mcp(session_id, servers).await
}
fn session_mode_id(mode: AcpSessionMode) -> SessionModeId {
    SessionModeId::from(match mode { AcpSessionMode::Default => "default", AcpSessionMode::AcceptEdits => "accept_edits", AcpSessionMode::BypassPermissions => "bypass_permissions" })
}
fn build_available_modes() -> Vec<SessionMode> {
    AcpSessionMode::all().iter().map(|mode| SessionMode::new(session_mode_id(*mode), mode.display_name()).description(mode.description())).collect()
}
fn build_mode_state(current: AcpSessionMode) -> SessionModeState { SessionModeState::new(session_mode_id(current), build_available_modes()) }
fn send_restored_title(cx: &ConnectionTo<Client>, session_id: &SessionId, title: Option<&str>) -> Result<(), AcpError> {
    let Some(title) = title else { return Ok(()); };
    cx.send_notification(SessionNotification::new(session_id.clone(), SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().title(title.to_owned()))))?;
    Ok(())
}
fn is_rejected_or_cancelled_tool_result(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("refusé par l'utilisateur") || lower.contains("annulé par l'utilisateur") || lower.contains("échec de la demande de permission acp")
}
fn replay_tool_result(cx: &ConnectionTo<Client>, session_id: &SessionId, tool_call_index: usize, tool_name: &str, args: &serde_json::Value, result_text: Option<&str>, cwd: &std::path::Path) -> Result<(), AcpError> {
    let call_id = ToolCallId::from(format!("replay_call_{tool_call_index}"));
    let info = ToolInfo::build(tool_name, args, cwd, None);
    let is_ok = result_text.map(|text| !is_rejected_or_cancelled_tool_result(text)).unwrap_or(false);
    cx.send_notification(SessionNotification::new(session_id.clone(), SessionUpdate::ToolCall(ToolCall::new(call_id.clone(), info.title.clone()).kind(info.kind).status(if result_text.is_some() { if is_ok { ToolCallStatus::Completed } else { ToolCallStatus::Failed } } else { ToolCallStatus::InProgress }).content(info.content.clone()).locations(info.locations.clone()).raw_input(bounded_raw_input(args)))))?;
    if let Some(result_text) = result_text {
        let rendered = result_update(tool_name, args, result_text, is_ok, cwd, None);
        cx.send_notification(SessionNotification::new(session_id.clone(), SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(call_id, ToolCallUpdateFields::new().status(rendered.status).content(rendered.content).locations(rendered.locations)))))?;
    }
    Ok(())
}

pub async fn handle_new(req: NewSessionRequest, responder: Responder<NewSessionResponse>, state: &AppState) -> Result<(), AcpError> {
    let session = match state.sessions.create(req.cwd.clone(), req.additional_directories.clone(), &state.config.default_model).await { Ok(session) => session, Err(error) => return responder.respond_with_internal_error(format!("création de session: {error:#}")) };
    let session_id = SessionId::from(session.id.clone());
    if let Err(error) = configure_mcp(state, &session.id, req.mcp_servers, &session.cwd).await {
        if let Err(cleanup) = state.sessions.delete(&session.id).await { tracing::error!(session=%session.id,%cleanup,"failed to clean up session after MCP setup failure"); }
        return responder.respond_with_error(mcp_config_error(&session_id, &error));
    }
    responder.respond(NewSessionResponse::new(session_id).config_options(build_config_options(&session.model, session.think, session.tools_enabled)).modes(build_mode_state(session.mode)))
}

pub async fn handle_list(req: ListSessionsRequest, responder: Responder<ListSessionsResponse>, state: &AppState) -> Result<(), AcpError> {
    let sessions = match state.sessions.list(req.cwd.as_deref()).await { Ok(sessions) => sessions, Err(error) => return responder.respond_with_internal_error(format!("liste des sessions: {error:#}")) };
    let infos = sessions.into_iter().map(|session| SessionInfo::new(SessionId::from(session.id), session.cwd).additional_directories(session.additional_directories).title(session.title).updated_at(Some(session.updated_at))).collect();
    responder.respond(ListSessionsResponse::new(infos))
}

pub async fn handle_load(req: LoadSessionRequest, responder: Responder<LoadSessionResponse>, state: &AppState, cx: &ConnectionTo<Client>) -> Result<(), AcpError> {
    if !is_valid_session_id(&req.session_id.0) { return responder.respond_with_error(session_id_error(&req.session_id)); }
    let session = match state.sessions.load(&req.session_id.0, &req.cwd).await { Ok(session) => session, Err(error) => return responder.respond_with_error(AcpError::invalid_params().data(serde_json::json!({"session_id":req.session_id.to_string(),"error":format!("session introuvable ou workspace incompatible: {error:#}")}))) };
    if let Err(error) = configure_mcp(state, &req.session_id.0, req.mcp_servers, &session.cwd).await { state.sessions.clear_mcp(&req.session_id.0).await; return responder.respond_with_error(mcp_config_error(&req.session_id, &error)); }
    send_restored_title(cx, &req.session_id, session.title.as_deref())?;
    let mut replay_index = 0usize;
    let mut index = 0usize;
    while index < session.messages.len() {
        let (role, text) = &session.messages[index];
        match role {
            Role::User => {
                let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(text.clone()))).message_id(MessageId::from(format!("msg_{index}")));
                cx.send_notification(SessionNotification::new(req.session_id.clone(), SessionUpdate::UserMessageChunk(chunk)))?;
            }
            Role::Assistant => {
                let (clean_text, calls) = parse_tool_calls(text);
                if !clean_text.trim().is_empty() {
                    let chunk = ContentChunk::new(ContentBlock::Text(TextContent::new(clean_text))).message_id(MessageId::from(format!("msg_{index}")));
                    cx.send_notification(SessionNotification::new(req.session_id.clone(), SessionUpdate::AgentMessageChunk(chunk)))?;
                }
                let mut result_cursor = index + 1;
                for call in &calls {
                    let result_text = if result_cursor < session.messages.len() && session.messages[result_cursor].0 == Role::Tool { let result = session.messages[result_cursor].1.as_str(); result_cursor += 1; Some(result) } else { None };
                    replay_tool_result(cx, &req.session_id, replay_index, &call.name, &call.arguments, result_text, &session.cwd)?;
                    replay_index += 1;
                }
                index = result_cursor.saturating_sub(1);
            }
            Role::Tool => {}
        }
        index += 1;
    }
    responder.respond(LoadSessionResponse::new().config_options(build_config_options(&session.model, session.think, session.tools_enabled)).modes(build_mode_state(session.mode)))
}

pub async fn handle_resume(req: ResumeSessionRequest, responder: Responder<ResumeSessionResponse>, state: &AppState, cx: &ConnectionTo<Client>) -> Result<(), AcpError> {
    if !is_valid_session_id(&req.session_id.0) { return responder.respond_with_error(session_id_error(&req.session_id)); }
    let session = match state.sessions.resume(&req.session_id.0, &req.cwd).await { Ok(session) => session, Err(error) => return responder.respond_with_error(AcpError::invalid_params().data(serde_json::json!({"session_id":req.session_id.to_string(),"error":format!("session introuvable ou workspace incompatible: {error:#}")}))) };
    if let Err(error) = configure_mcp(state, &req.session_id.0, req.mcp_servers, &session.cwd).await { state.sessions.clear_mcp(&req.session_id.0).await; return responder.respond_with_error(mcp_config_error(&req.session_id, &error)); }
    send_restored_title(cx, &req.session_id, session.title.as_deref())?;
    responder.respond(ResumeSessionResponse::new().config_options(build_config_options(&session.model, session.think, session.tools_enabled)).modes(build_mode_state(session.mode)))
}

pub async fn handle_delete(req: DeleteSessionRequest, responder: Responder<DeleteSessionResponse>, state: &AppState) -> Result<(), AcpError> {
    if !is_valid_session_id(&req.session_id.0) { return responder.respond_with_error(session_id_error(&req.session_id)); }
    match state.sessions.delete(&req.session_id.0).await { Ok(true) => responder.respond(DeleteSessionResponse::new()), Ok(false) => responder.respond_with_error(AcpError::invalid_params().data(serde_json::json!({"session_id":req.session_id.to_string(),"error":"session introuvable"}))), Err(error) => responder.respond_with_internal_error(format!("suppression de session: {error:#}")) }
}

pub async fn handle_close(req: CloseSessionRequest, responder: Responder<CloseSessionResponse>, state: &AppState) -> Result<(), AcpError> {
    if !is_valid_session_id(&req.session_id.0) { return responder.respond_with_error(session_id_error(&req.session_id)); }
    match state.sessions.close(&req.session_id.0).await { Ok(true) => responder.respond(CloseSessionResponse::new()), Ok(false) => responder.respond_with_error(AcpError::invalid_params().data(serde_json::json!({"session_id":req.session_id.to_string(),"error":"session introuvable"}))), Err(error) => responder.respond_with_internal_error(format!("fermeture de session: {error:#}")) }
}

pub async fn handle_set_mode(req: SetSessionModeRequest, responder: Responder<SetSessionModeResponse>, state: &AppState, cx: &ConnectionTo<Client>) -> Result<(), AcpError> {
    let Some(new_mode) = AcpSessionMode::from_str_lossy(&req.mode_id.0) else {
        let valid = AcpSessionMode::all().iter().map(|mode| session_mode_id(*mode).0.to_string()).collect::<Vec<_>>().join(", ");
        return responder.respond_with_error(AcpError::invalid_params().data(serde_json::json!({"mode_id":req.mode_id.to_string(),"error":format!("mode_id invalide. Modes valides: {valid}")})));
    };
    let updated = match state.sessions.set_mode(&req.session_id.0, new_mode).await { Ok(session) => session, Err(error) => return responder.respond_with_error(AcpError::invalid_params().data(serde_json::json!({"session_id":req.session_id.to_string(),"error":format!("impossible de changer le mode: {error:#}")}))) };
    cx.send_notification(SessionNotification::new(req.session_id.clone(), SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(session_mode_id(updated.mode)))))?;
    responder.respond(SetSessionModeResponse::new())
}

pub async fn handle_fork(req: ForkSessionRequest, responder: Responder<ForkSessionResponse>, state: &AppState) -> Result<(), AcpError> {
    if !is_valid_session_id(&req.session_id.0) { return responder.respond_with_error(session_id_error(&req.session_id)); }
    let forked = match state.sessions.fork(&req.session_id.0).await { Ok(forked) => forked, Err(error) => return responder.respond_with_internal_error(format!("fork de session: {error:#}")) };
    responder.respond(ForkSessionResponse::new(SessionId::from(forked.id)))
}
