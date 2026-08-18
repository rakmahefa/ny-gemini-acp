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

#[derive(Debug, Clone)]
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
}

impl From<anyhow::Error> for LlmError {
    fn from(error: anyhow::Error) -> Self {
        Self::Provider(error.to_string())
    }
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, LlmError>;
}

impl fmt::Debug for LlmStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("LlmStream").finish_non_exhaustive()
    }
}
