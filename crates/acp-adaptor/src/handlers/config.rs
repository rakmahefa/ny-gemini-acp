//! Handler `session/set_config_option`.

use agent_client_protocol::schema::v1::*;
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError, Responder};

use crate::config::config_options::build_config_options;
use agent_runtime::state::Session;
use agent_runtime::AppState;

pub async fn handle(
    req: SetSessionConfigOptionRequest,
    responder: Responder<SetSessionConfigOptionResponse>,
    state: &AppState,
    cx: &ConnectionTo<Client>,
) -> Result<(), AcpError> {
    if state.store.get(&req.session_id.0).await.is_none() {
        return responder.respond_with_error(
            AcpError::invalid_params()
                .data(serde_json::json!({ "session_id": req.session_id.0.to_string() })),
        );
    }

    let config_id = req.config_id.0.clone();
    let value = req.value.clone();
    let session = match state
        .store
        .update_session(&req.session_id.0, move |s: &mut Session| {
            match config_id.as_ref() {
                "model" => {
                    if let Some(v) = value.as_value_id() {
                        // D-12 : valider la valeur contre la table des modèles.
                        let candidate = v.0.to_string();
                        if llm_provider::core::models::MODEL_KEYS.contains(&candidate.as_str()) {
                            s.model = candidate;
                        } else {
                            tracing::warn!(
                                model = %candidate,
                                "unknown requested model, config option ignored"
                            );
                        }
                    }
                }
                "think" => {
                    if let Some(v) = value.as_value_id() {
                        // D-12 : une valeur invalide est signalée explicitement
                        // (warn) au lieu d'être ignorée silencieusement.
                        match v.0.parse::<u32>() {
                            Ok(n) => s.think = Some(n.min(4)),
                            Err(_) => tracing::warn!(
                                value = %v.0,
                                "invalid think value, config option ignored"
                            ),
                        }
                    }
                }
                "tools_enabled" => {
                    if let Some(v) = value.as_value_id() {
                        match v.0.as_ref().to_ascii_lowercase().as_str() {
                            "true" | "1" | "on" | "yes" => s.tools_enabled = true,
                            "false" | "0" | "off" | "no" => s.tools_enabled = false,
                            other => tracing::warn!(
                                value = other,
                                "invalid tools_enabled value, ignored"
                            ),
                        }
                    }
                }
                other => tracing::warn!(config_id = other, "unknown config_id"),
            }
        })
        .await
    {
        Ok(()) => state.store.get(&req.session_id.0).await,
        Err(e) => return responder.respond_with_internal_error(format!("{e:#}")),
    };

    let Some(session) = session else {
        return responder.respond_with_error(
            AcpError::invalid_params()
                .data(serde_json::json!({ "session_id": req.session_id.0.to_string() })),
        );
    };

    let options = build_config_options(&session.model, session.think, session.tools_enabled);
    cx.send_notification(SessionNotification::new(
        req.session_id.clone(),
        SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(options.clone())),
    ))?;
    responder.respond(SetSessionConfigOptionResponse::new(options))
}
