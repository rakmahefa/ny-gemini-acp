//! Provider-neutral contracts owned by the agent runtime.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{mpsc, watch};

use crate::tool_ui::ToolUiModel;

/// Canonical semantic events emitted by an LLM provider.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCall {
        id: String,
        name: String,
        arguments: Value,
    },
    Usage {
        prompt_tokens: Option<u64>,
        completion_tokens: Option<u64>,
        total_tokens: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProviderErrorKind {
    Authentication,
    InvalidRequest,
    ModelUnavailable,
    Network,
    Upstream,
    StreamDivergence,
    Upload,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum LlmError {
    #[error("invalid model request: {0}")]
    InvalidRequest(String),
    #[error("authentication failed: {0}")]
    Authentication(String),
    #[error("model is unavailable: {0}")]
    Unavailable(String),
    #[error("provider request failed: {0}")]
    Provider(String),
    #[error("provider network failure: {0}")]
    Network(String),
    #[error("provider stream diverged")]
    StreamDivergence,
    #[error("provider image upload failed: {0}")]
    Upload(String),
    #[error("request cancelled")]
    Cancelled,
}

impl LlmError {
    pub fn kind(&self) -> Option<LlmProviderErrorKind> {
        match self {
            Self::InvalidRequest(_) => Some(LlmProviderErrorKind::InvalidRequest),
            Self::Authentication(_) => Some(LlmProviderErrorKind::Authentication),
            Self::Unavailable(_) => Some(LlmProviderErrorKind::ModelUnavailable),
            Self::Provider(_) => Some(LlmProviderErrorKind::Upstream),
            Self::Network(_) => Some(LlmProviderErrorKind::Network),
            Self::StreamDivergence => Some(LlmProviderErrorKind::StreamDivergence),
            Self::Upload(_) => Some(LlmProviderErrorKind::Upload),
            Self::Cancelled => None,
        }
    }
}

pub type LlmStream = mpsc::Receiver<Result<ModelEvent, LlmError>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GenerationOptions {
    pub reasoning_budget: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ModelRequest {
    pub prompt: String,
    pub model: String,
    pub generation: GenerationOptions,
    pub references: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LlmModelInfo {
    pub supports_reasoning: bool,
}

#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream(&self, request: ModelRequest) -> Result<LlmStream, LlmError>;

    /// Cancellation-aware boundary for establishing the provider stream.
    ///
    /// Implementations keep the stable `stream()` contract while the runtime
    /// prevents a provider that is blocked before stream creation from holding
    /// a cancelled turn indefinitely.
    async fn stream_with_cancellation(
        &self,
        request: ModelRequest,
        mut cancellation: watch::Receiver<bool>,
    ) -> Result<LlmStream, LlmError> {
        if *cancellation.borrow() {
            return Err(LlmError::Cancelled);
        }
        tokio::select! {
            result = self.stream(request) => result,
            changed = cancellation.changed() => {
                match changed {
                    Ok(()) if *cancellation.borrow() => Err(LlmError::Cancelled),
                    Ok(()) => self.stream_with_cancellation_after_change(cancellation).await,
                    Err(_) => Err(LlmError::Cancelled),
                }
            }
        }
    }

    async fn stream_with_cancellation_after_change(
        &self,
        mut cancellation: watch::Receiver<bool>,
    ) -> Result<LlmStream, LlmError> {
        loop {
            if *cancellation.borrow() {
                return Err(LlmError::Cancelled);
            }
            match cancellation.changed().await {
                Ok(()) => continue,
                Err(_) => return Err(LlmError::Cancelled),
            }
        }
    }

    async fn upload_image(&self, base64: &str, mime: &str) -> Result<String, LlmError>;
    fn model_info(&self, model: &str) -> LlmModelInfo;
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum ToolConfigurationError {
    #[error("tool configuration is invalid: {0}")]
    InvalidConfiguration(String),
    #[error("tool transport '{transport}' failed: {message}")]
    Transport { transport: String, message: String },
    #[error("tool protocol error: {0}")]
    Protocol(String),
    #[error("tool provider rejected request: code={code}, message={message}")]
    Remote { code: i64, message: String },
    #[error("tool response exceeded the configured message limit")]
    MessageTooLarge,
    #[error("tool pagination exceeded the configured page limit")]
    PaginationLimit,
    #[error("tool provider configuration failed: {0}")]
    Provider(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolTransportKind {
    Process,
    Http,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolServerConfig {
    pub name: String,
    pub transport: ToolTransportKind,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub url: Option<String>,
    pub headers: HashMap<String, String>,
}

impl ToolServerConfig {
    pub fn process(
        name: impl Into<String>,
        command: impl Into<String>,
        args: Vec<String>,
        env: HashMap<String, String>,
        cwd: Option<PathBuf>,
    ) -> Self {
        Self {
            name: name.into(),
            transport: ToolTransportKind::Process,
            command: Some(command.into()),
            args,
            env,
            cwd,
            url: None,
            headers: HashMap::new(),
        }
    }

    pub fn http(
        name: impl Into<String>,
        url: impl Into<String>,
        headers: HashMap<String, String>,
    ) -> Self {
        Self {
            name: name.into(),
            transport: ToolTransportKind::Http,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            url: Some(url.into()),
            headers,
        }
    }
}

#[derive(Debug)]
pub struct ToolCallRequest {
    /// Canonical semantic model/tool invocation identity. Providers must preserve it for UX correlation.
    pub call_id: String,
    pub session_id: String,
    pub name: String,
    pub arguments: Value,
    pub cwd: PathBuf,
    pub additional_dirs: Vec<PathBuf>,
    pub cancellation: watch::Receiver<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallResult {
    pub content: String,
    pub is_ok: bool,
    pub executed: bool,
    pub ui: Option<ToolUiModel>,
}

impl ToolCallResult {
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_ok: false,
            executed: false,
            ui: None,
        }
    }
}

#[async_trait::async_trait]
pub trait ToolProvider: Send + Sync {
    async fn for_session(&self, session_id: &str) -> Arc<dyn ToolProvider>;
    async fn configure_session(
        &self,
        session_id: &str,
        cwd: PathBuf,
        servers: Vec<ToolServerConfig>,
    ) -> Result<(), ToolConfigurationError>;
    async fn clear_session(&self, session_id: &str);
    fn definitions(&self) -> Vec<Value>;
    fn prompt_fragment(&self) -> Option<String>;
    fn has_tools(&self) -> bool;
    /// Returns the host-neutral presentation model for a concrete invocation.
    /// The call ID is part of the UX contract so terminal content can be correlated end-to-end.
    fn ui_model(&self, call_id: &str, name: &str, arguments: &Value) -> Option<ToolUiModel>;
    async fn call(&self, request: ToolCallRequest) -> ToolCallResult;
}

#[derive(Clone, Default)]
pub struct NullToolProvider;

#[async_trait::async_trait]
impl ToolProvider for NullToolProvider {
    async fn for_session(&self, _: &str) -> Arc<dyn ToolProvider> {
        Arc::new(Self)
    }
    async fn configure_session(
        &self,
        _: &str,
        _: PathBuf,
        _: Vec<ToolServerConfig>,
    ) -> Result<(), ToolConfigurationError> {
        Ok(())
    }
    async fn clear_session(&self, _: &str) {}
    fn definitions(&self) -> Vec<Value> {
        Vec::new()
    }
    fn prompt_fragment(&self) -> Option<String> {
        None
    }
    fn has_tools(&self) -> bool {
        false
    }
    fn ui_model(&self, _call_id: &str, _name: &str, _arguments: &Value) -> Option<ToolUiModel> {
        None
    }
    async fn call(&self, request: ToolCallRequest) -> ToolCallResult {
        ToolCallResult::error(format!("outil indisponible: {}", request.name))
    }
}

#[derive(Clone, Default)]
pub struct NullLlmProvider;

#[async_trait::async_trait]
impl LlmProvider for NullLlmProvider {
    async fn stream(&self, _: ModelRequest) -> Result<LlmStream, LlmError> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }
    async fn upload_image(&self, _: &str, _: &str) -> Result<String, LlmError> {
        Err(LlmError::Unavailable("LLM provider indisponible".into()))
    }
    fn model_info(&self, _: &str) -> LlmModelInfo {
        LlmModelInfo::default()
    }
}

pub type SharedLlmProvider = Arc<dyn LlmProvider>;
pub type SharedToolProvider = Arc<dyn ToolProvider>;
