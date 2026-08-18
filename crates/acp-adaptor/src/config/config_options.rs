//! ACP configuration options and capabilities exposed to the client.

use agent_client_protocol::schema::v1::*;

use llm_provider::core::models;

pub fn build_config_options(
    model: &str,
    think: Option<u32>,
    tools_enabled: bool,
) -> Vec<SessionConfigOption> {
    let tools_options = vec![
        SessionConfigSelectOption::new(SessionConfigValueId::from("true"), "Activé"),
        SessionConfigSelectOption::new(SessionConfigValueId::from("false"), "Désactivé"),
    ];
    let model_options: Vec<SessionConfigSelectOption> = models::MODEL_KEYS
        .iter()
        .map(|key| SessionConfigSelectOption::new(SessionConfigValueId::from(*key), *key))
        .collect();
    let think_default = think.unwrap_or_else(|| {
        models::resolve(model, models::DEFAULT_MODEL)
            .map(|r| r.think)
            .unwrap_or(4)
    });
    let think_options: Vec<SessionConfigSelectOption> = (0..=4)
        .map(|n| {
            SessionConfigSelectOption::new(
                SessionConfigValueId::from(n.to_string()),
                format!("Réflexion {n}"),
            )
        })
        .collect();
    vec![
        SessionConfigOption::new(
            SessionConfigId::from("model"),
            "Modèle",
            SessionConfigKind::Select(SessionConfigSelect::new(
                SessionConfigValueId::from(model.to_string()),
                SessionConfigSelectOptions::Ungrouped(model_options),
            )),
        )
        .category(SessionConfigOptionCategory::Model),
        SessionConfigOption::new(
            SessionConfigId::from("think"),
            "Réflexion",
            SessionConfigKind::Select(SessionConfigSelect::new(
                SessionConfigValueId::from(think_default.to_string()),
                SessionConfigSelectOptions::Ungrouped(think_options),
            )),
        )
        .category(SessionConfigOptionCategory::ThoughtLevel),
        SessionConfigOption::new(
            SessionConfigId::from("tools_enabled"),
            "Outils (file, shell, search)",
            SessionConfigKind::Select(SessionConfigSelect::new(
                SessionConfigValueId::from(if tools_enabled { "true" } else { "false" }),
                SessionConfigSelectOptions::Ungrouped(tools_options),
            )),
        )
        .category(SessionConfigOptionCategory::Model),
    ]
}

pub fn build_agent_capabilities() -> AgentCapabilities {
    AgentCapabilities::new()
        .load_session(true)
        .session_capabilities(
            SessionCapabilities::new()
                .list(SessionListCapabilities::new())
                .delete(SessionDeleteCapabilities::new())
                .resume(SessionResumeCapabilities::new())
                .close(SessionCloseCapabilities::new())
                .fork(SessionForkCapabilities::new()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_config_options_retourne_3_options() {
        let options = build_config_options("test-model", Some(2), true);
        assert_eq!(options.len(), 3);
    }

    #[test]
    fn build_agent_capabilities_active_load_et_resume() {
        let caps = build_agent_capabilities();
        assert!(caps.load_session);
        assert!(caps.session_capabilities.resume.is_some());
    }
}
