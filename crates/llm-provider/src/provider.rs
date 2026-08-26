//! Gemini implementation of the provider-neutral runtime LLM contract.
use std::sync::Arc;

use crate::client::{Client, Config, StreamItem};
use crate::config::AgentConfig;
use crate::core::GeminiError;
use crate::semantic_stream::GeminiSemanticStream;
use agent_runtime::{
    LlmError, LlmModelInfo, LlmProvider, LlmStream, ModelEvent, ModelRequest,
};
use tokio::sync::mpsc;

fn map_gemini_error(error: &anyhow::Error) -> LlmError {
    let Some(error) = error.downcast_ref::<GeminiError>() else {
        return LlmError::Provider(format!("{error:#}"));
    };

    match error {
        GeminiError::CookiesExpired { code } => {
            LlmError::Authentication(format!("cookies expired or invalid (BardErrorInfo [{code}])"))
        }
        GeminiError::UnknownModel(model) => LlmError::Unavailable(model.clone()),
        GeminiError::Network(message) => LlmError::Network(message.clone()),
        GeminiError::Http { status, body } => {
            LlmError::Provider(format!("HTTP {status}: {body}"))
        }
        GeminiError::StreamDivergence => LlmError::StreamDivergence,
        GeminiError::UploadFailed(message) => LlmError::Upload(message.clone()),
        GeminiError::SafetyBlocked(message) => LlmError::Provider(message.clone()),
        GeminiError::Other(error) => LlmError::Provider(format!("{error:#}")),
    }
}

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
            .map_err(|error| map_gemini_error(&error))?;

        let (tx, rx) = mpsc::channel(16);
        let supports_reasoning = self.model_info(&request.model).supports_reasoning;
        tokio::spawn(async move {
            let mut semantic = GeminiSemanticStream::new(supports_reasoning);
            let mut upstream = upstream;
            while let Some(item) = upstream.recv().await {
                match item {
                    Ok(StreamItem::Text(delta)) => {
                        for event in
                            semantic.feed(crate::core::frames::GeminiFrameEvent::Text(delta))
                        {
                            if tx.send(Ok(event)).await.is_err() {
                                return;
                            }
                        }
                    }
                    Ok(StreamItem::ToolCall {
                        id,
                        name,
                        arguments,
                    }) => {
                        let frame = crate::core::frames::GeminiFrameEvent::ToolCall {
                            id,
                            name,
                            arguments,
                        };
                        for event in semantic.feed(frame) {
                            if tx.send(Ok(event)).await.is_err() {
                                return;
                            }
                        }
                    }
                    Ok(StreamItem::Metadata { kind, value }) => {
                        let frame = crate::core::frames::GeminiFrameEvent::Metadata { kind, value };
                        for event in semantic.feed(frame) {
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
            .map_err(|error| map_gemini_error(&error))
    }

    fn model_info(&self, model: &str) -> LlmModelInfo {
        let supports_reasoning =
            crate::core::models::resolve(model, crate::core::models::DEFAULT_MODEL)
                .map(|resolved| crate::core::models::is_thinking_mode(resolved.mode))
                .unwrap_or(false);
        LlmModelInfo { supports_reasoning }
    }
}

#[allow(dead_code)]
fn _model_event_type_is_stable(_: ModelEvent) {}
