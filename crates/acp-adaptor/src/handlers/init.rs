//! Handler `initialize`.

use agent_client_protocol::schema::v1::*;
use agent_client_protocol::{Error as AcpError, Responder};

use crate::config::config_options::build_agent_capabilities;
use gemini_acp_runtime::AppState;

pub async fn handle(
    req: InitializeRequest,
    responder: Responder<InitializeResponse>,
    _state: &AppState,
) -> Result<(), AcpError> {
    let mut caps = build_agent_capabilities();
    caps.session_capabilities = caps
        .session_capabilities
        .fork(SessionForkCapabilities::new());
    caps = caps.mcp_capabilities(McpCapabilities::new().http(true).sse(false));

    responder.respond(
        InitializeResponse::new(req.protocol_version)
            .agent_capabilities(caps)
            .auth_methods(vec![])
            .agent_info(
                Implementation::new("gemini-acp", env!("CARGO_PKG_VERSION")).title("Gemini (Web)"),
            ),
    )?;
    Ok(())
}
