use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub prompt: String,
    pub model: String,
    pub thinking: Option<u32>,
    pub refs: Vec<String>,
}

pub struct LlmStream {
    receiver: mpsc::Receiver<Result<String, LlmError>>,
}

impl LlmStream {
    pub fn new(receiver: mpsc::Receiver<Result<String, LlmError>>) -> Self {
        Self { receiver }
    }

    pub async fn recv(&mut self) -> Option<Result<String, LlmError>> {
        self.receiver.recv().await
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum LlmError {
    #[error("provider error: {0}")]
    Provider(String),

    #[error("provider capability is not supported: {0}")]
    UnsupportedCapability(String),
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;

    fn is_thinking_model(&self, _model: &str) -> bool {
        false
    }

    async fn upload_images(
        &self,
        _images: &[(String, String)],
    ) -> Result<Vec<String>, LlmError> {
        Err(LlmError::UnsupportedCapability(
            "image upload".to_owned(),
        ))
    }

    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, LlmError>;
}

impl fmt::Display for LlmStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<llm stream>")
    }
}
