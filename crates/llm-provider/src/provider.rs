//! Gemini implementation of the provider-neutral runtime LLM contract.
use std::sync::Arc;

use crate::client::{Client, Config};
use crate::config::AgentConfig;
use agent_runtime::{LlmModelInfo, LlmProvider, LlmRequest, LlmStream};

#[derive(Clone)]
pub struct GeminiProvider {
    client: Arc<Client>,
}

impl GeminiProvider {
    pub async fn from_agent_config(config: &AgentConfig) -> anyhow::Result<Self> {
        let client_config = Config {
            cookie_file: config.cookie_file.clone(),
            default_model: config.default_model.clone(),
            auth_user: config.auth_user,
            proxy: config.proxy.clone(),
            ..Default::default()
        };
        Ok(Self {
            client: Arc::new(Client::new(client_config).await?),
        })
    }

    pub fn client(&self) -> Arc<Client> {
        Arc::clone(&self.client)
    }
}

#[async_trait::async_trait]
impl LlmProvider for GeminiProvider {
    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, String> {
        self.client
            .stream(
                &request.prompt,
                &request.model,
                request.think,
                &request.refs,
            )
            .await
            .map_err(|error| format!("{error:#}"))
    }

    async fn upload_image(&self, base64: &str, mime: &str) -> Result<String, String> {
        self.client
            .upload_image(base64, mime)
            .await
            .map_err(|error| format!("{error:#}"))
    }

    fn model_info(&self, model: &str) -> LlmModelInfo {
        let supports_thinking =
            crate::core::models::resolve(model, crate::core::models::DEFAULT_MODEL)
                .map(|resolved| crate::core::models::is_thinking_mode(resolved.mode))
                .unwrap_or(false);
        LlmModelInfo { supports_thinking }
    }
}
