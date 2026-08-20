//! Provider-neutral contracts owned by the agent runtime.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{mpsc, watch};

use crate::tool_ui::ToolUiModel;

/// Canonical semantic events emitted by an LLM provider.
///
/// Provider wire formats are deliberately normalized before the runtime sees
/// them. A provider may expose richer native events, but the runtime only
/// reasons about this stable vocabulary.
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
    #[error("request cancelled")]
    Cancelled,
}

pub type LlmStream = mpsc::Receiver<Result<ModelEvent, LlmError>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GenerationOptions {
    /// Optional provider-neutral reasoning budget.
    /// Concrete providers map this to their native reasoning controls.
    pub reasoning_budget: Option<u32>,
}

/// Provider-neutral model request.
///
/// `prompt` is the runtime's serialized context representation. Provider
/// adapters must not infer ACP or provider wire-format semantics from it.
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
    async fn upload_image(&self, base64: &str, mime: &str) -> Result<String, LlmError>;
    fn model_info(&self, model: &str) -> LlmModelInfo;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolTransportKind {
    Process,
    Http,
}

/// Session-scoped tool server configuration.
///
/// The runtime deliberately models a generic tool transport instead of MCP.
/// ACP/MCP-specific representations are converted at the adapter boundary and
/// provider-specific transports remain inside `tools-provider`.
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
    /// Structured, host-neutral presentation data for the tool invocation/result.
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

pub trait ToolEventSink: Send {
    fn tool_call_requested(&mut self, upstream_id: String, name: String) -> bool;
    fn permission_requested(&mut self, upstream_id: String) -> bool;
    fn tool_execution_started(&mut self, upstream_id: String) -> bool;
    fn tool_result_received(&mut self, upstream_id: String, result: String) -> bool;
}

#[async_trait::async_trait]
pub trait ToolProvider: Send + Sync {
    async fn for_session(&self, session_id: &str) -> Arc<dyn ToolProvider>;
    async fn configure_session(
        &self,
        session_id: &str,
        cwd: PathBuf,
        servers: Vec<ToolServerConfig>,
    ) -> Result<(), String>;
    async fn clear_session(&self, session_id: &str);
    fn definitions(&self) -> Vec<Value>;
    fn prompt_fragment(&self) -> Option<String>;
    fn has_tools(&self) -> bool;
    /// Returns the host-neutral presentation model for a tool invocation.
    /// `cwd` is part of the presentation contract because paths/locations are
    /// inherently workspace-relative.
    fn ui_model(&self, name: &str, arguments: &Value, cwd: &Path) -> Option<ToolUiModel>;
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
    ) -> Result<(), String> {
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
    fn ui_model(&self, name: &str, arguments: &Value, _: &Path) -> Option<ToolUiModel> {
        Some(ToolUiModel::generic(name, arguments.clone()))
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
