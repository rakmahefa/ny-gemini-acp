//! Gemini implementation of the provider-neutral runtime LLM contract.
use std::sync::Arc;

use crate::client::{Client, Config};
use crate::config::AgentConfig;
use crate::semantic_stream::GeminiSemanticStream;
use agent_runtime::{LlmError, LlmModelInfo, LlmProvider, LlmStream, ModelRequest};
use tokio::sync::mpsc;

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
    async fn stream(&self, request: ModelRequest) -> Result<LlmStream, LlmError> {
        let upstream = self
            .client
            .stream(
                &request.prompt,
                &request.model,
                request.generation.reasoning_budget,
                &request.references,
            )
            .await
            .map_err(|error| LlmError::Provider(format!("{error:#}")))?;

        let (tx, rx) = mpsc::channel(16);
        let supports_reasoning = self.model_info(&request.model).supports_reasoning;
        tokio::spawn(async move {
            let mut semantic = GeminiSemanticStream::new(supports_reasoning);
            let mut upstream = upstream;
            while let Some(item) = upstream.recv().await {
                match item {
                    Ok(delta) => {
                        for event in semantic.feed(&delta) {
                            if tx.send(Ok(event)).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        let _ = tx.send(Err(LlmError::Provider(error))).await;
                        return;
                    }
                }
            }
            for event in semantic.finish() {
                if tx.send(Ok(event)).await.is_err() {
                    return;
                }
            }
        });
        Ok(rx)
    }

    async fn upload_image(&self, base64: &str, mime: &str) -> Result<String, LlmError> {
        self.client
            .upload_image(base64, mime)
            .await
            .map_err(|error| LlmError::Provider(format!("{error:#}")))
    }

    fn model_info(&self, model: &str) -> LlmModelInfo {
        let supports_reasoning = crate::core::models::resolve(
            model,
            crate::core::models::DEFAULT_MODEL,
        )
        .map(|resolved| crate::core::models::is_thinking_mode(resolved.mode))
        .unwrap_or(false);
        LlmModelInfo { supports_reasoning }
    }
}
