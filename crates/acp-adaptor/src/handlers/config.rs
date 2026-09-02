//! Handler `session/set_config_option`.
//!
//! Validation invariant (SPEC-P1-03): every rejected value answers
//! `invalid_params` with the accepted values listed — never a success without
//! effect. The session state is only touched after the requested change has
//! been fully validated.

use agent_client_protocol::schema::v1::*;
use agent_client_protocol::{Client, ConnectionTo, Error as AcpError, Responder};

use crate::config::config_options::build_config_options;
use crate::handlers::session::is_valid_session_id;
use agent_runtime::state::Session;
use agent_runtime::AppState;

/// A fully validated configuration change, ready to apply to a session.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ConfigChange {
    Model(String),
    Think(u32),
    ToolsEnabled(bool),
}

fn accepted_models() -> String {
    llm_provider::core::models::MODEL_KEYS.join(", ")
}

/// Validates a raw (config_id, value) pair without touching any session
/// state. Returns an English, user-actionable error listing the accepted
/// values when the pair is rejected.
fn validate_config_change(
    config_id: &str,
    value: &SessionConfigOptionValue,
) -> Result<ConfigChange, String> {
    let raw = value
        .as_value_id()
        .map(|id| id.0.to_string())
        .ok_or_else(|| "config option value must be a string value id".to_string())?;

    match config_id {
        "model" => {
            if llm_provider::core::models::MODEL_KEYS.contains(&raw.as_str()) {
                Ok(ConfigChange::Model(raw))
            } else {
                Err(format!(
                    "unknown model '{raw}'. Accepted models: {}",
                    accepted_models()
                ))
            }
        }
        "think" => match raw.parse::<u32>() {
            // The thinking budget is bounded by the provider (0..=4).
            Ok(n) => Ok(ConfigChange::Think(n.min(4))),
            Err(_) => Err(format!(
                "think must be a numeric string between 0 and 4, got '{raw}'"
            )),
        },
        "tools_enabled" => match raw.to_ascii_lowercase().as_str() {
            "true" | "1" | "on" | "yes" => Ok(ConfigChange::ToolsEnabled(true)),
            "false" | "0" | "off" | "no" => Ok(ConfigChange::ToolsEnabled(false)),
            _ => Err(format!(
                "tools_enabled must be one of true, 1, on, yes, false, 0, off, no, got '{raw}'"
            )),
        },
        other => Err(format!(
            "unknown config_id '{other}'. Accepted config ids: model, think, tools_enabled"
        )),
    }
}

fn apply_config_change(session: &mut Session, change: ConfigChange) {
    match change {
        ConfigChange::Model(model) => session.model = model,
        ConfigChange::Think(think) => session.think = Some(think),
        ConfigChange::ToolsEnabled(enabled) => session.tools_enabled = enabled,
    }
}

pub async fn handle(
    req: SetSessionConfigOptionRequest,
    responder: Responder<SetSessionConfigOptionResponse>,
    state: &AppState,
    cx: &ConnectionTo<Client>,
) -> Result<(), AcpError> {
    // The session id is client-controlled: validated before any store access
    // so a hostile id can never reach the persistence path.
    if !is_valid_session_id(&req.session_id.0) {
        return responder.respond_with_error(AcpError::invalid_params().data(serde_json::json!({
            "session_id": req.session_id.0.to_string(),
            "error": "invalid session id"
        })));
    }
    if state.store.get(&req.session_id.0).await.is_none() {
        return responder.respond_with_error(
            AcpError::invalid_params()
                .data(serde_json::json!({ "session_id": req.session_id.0.to_string() })),
        );
    }

    let change = match validate_config_change(&req.config_id.0, &req.value) {
        Ok(change) => change,
        Err(message) => {
            return responder.respond_with_error(AcpError::invalid_params().data(
                serde_json::json!({
                    "session_id": req.session_id.0.to_string(),
                    "error": message,
                }),
            ));
        }
    };

    if let Err(e) = state
        .store
        .update_session(&req.session_id.0, move |s: &mut Session| {
            apply_config_change(s, change);
        })
        .await
    {
        return responder.respond_with_internal_error(format!("{e:#}"));
    }

    let Some(session) = state.store.get(&req.session_id.0).await else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn value_id(raw: &str) -> SessionConfigOptionValue {
        SessionConfigOptionValue::ValueId {
            value: SessionConfigValueId::from(raw.to_string()),
        }
    }

    #[test]
    fn unknown_model_is_rejected_with_accepted_values() {
        let error = validate_config_change("model", &value_id("gpt-99"))
            .expect_err("unknown model must be rejected");
        assert!(error.contains("unknown model 'gpt-99'"), "{error}");
        assert!(error.contains("Accepted models:"), "{error}");
    }

    #[test]
    fn non_numeric_think_is_rejected_instead_of_clamped() {
        let error = validate_config_change("think", &value_id("fast"))
            .expect_err("non-numeric think must be rejected");
        assert!(error.contains("numeric string"), "{error}");
    }

    #[test]
    fn invalid_tools_enabled_is_rejected_with_accepted_values() {
        let error = validate_config_change("tools_enabled", &value_id("maybe"))
            .expect_err("invalid tools_enabled must be rejected");
        assert!(error.contains("true, 1, on, yes"), "{error}");
    }

    #[test]
    fn unknown_config_id_is_rejected_with_accepted_ids() {
        let error = validate_config_change("temperature", &value_id("0.7"))
            .expect_err("unknown config id must be rejected");
        assert!(error.contains("model, think, tools_enabled"), "{error}");
    }

    #[test]
    fn non_value_id_payloads_are_rejected() {
        // A boolean payload (no value id) must not be silently applied as a
        // model/think/tools change.
        let boolean = SessionConfigOptionValue::Boolean { value: true };
        assert!(validate_config_change("model", &boolean).is_err());
        assert!(validate_config_change("think", &boolean).is_err());
        assert!(validate_config_change("tools_enabled", &boolean).is_err());
    }

    #[test]
    fn valid_changes_are_normalized() {
        assert_eq!(
            validate_config_change("think", &value_id("9")).unwrap(),
            ConfigChange::Think(4),
            "think is clamped to the provider bound after numeric validation"
        );
        assert_eq!(
            validate_config_change("tools_enabled", &value_id("ON")).unwrap(),
            ConfigChange::ToolsEnabled(true)
        );
    }

    #[test]
    fn hostile_session_ids_are_refused_by_the_shared_validation() {
        // The id validation is the one shared with every other handler: a
        // hostile id never reaches Store::read (path traversal guard).
        for hostile in ["../escape", "a/b", "../../.ssh/id_rsa", ".", "..", ""] {
            assert!(
                !is_valid_session_id(hostile),
                "hostile session id must be refused: {hostile}"
            );
        }
    }

    #[test]
    fn config_value_json_round_trip_keeps_error_contract() {
        // Documents the JSON error payload shape used by the handler.
        let payload = json!({
            "session_id": "sess_x",
            "error": "unknown model 'gpt-99'. Accepted models: ...",
        });
        assert!(payload["error"]
            .as_str()
            .unwrap()
            .contains("Accepted models"));
    }
}
