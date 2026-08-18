//! Provider-neutral contracts owned by the agent runtime.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::{mpsc, watch};

pub type LlmStream = mpsc::Receiver<Result<String, String>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GenerationOptions {
    /// Optional provider-neutral reasoning budget.
    /// Concrete providers map this to their native reasoning controls.
    pub reasoning_budget: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct LlmRequest {
    pub prompt: String,
    pub model: String,
    pub generation: GenerationOptions,
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LlmModelInfo {
    pub supports_reasoning: bool,
}

#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, String>;
    async fn upload_image(&self, base64: &str, mime: &str) -> Result<String, String>;
    fn model_info(&self, model: &str) -> LlmModelInfo;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTransportKind {
    Stdio,
    Http,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransportKind,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub url: Option<String>,
    pub headers: HashMap<String, String>,
}

impl McpServerConfig {
    pub fn stdio(
        name: impl Into<String>,
        command: impl Into<String>,
        args: Vec<String>,
        env: HashMap<String, String>,
        cwd: Option<PathBuf>,
    ) -> Self {
        Self {
            name: name.into(),
            transport: McpTransportKind::Stdio,
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
            transport: McpTransportKind::Http,
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            url: Some(url.into()),
            headers,
        }
    }
}

/// Boundary hook for protocol adapters that need to translate an external
/// MCP representation into the runtime-owned configuration model.
pub trait IntoMcpServerConfig: Send {
    fn into_runtime_mcp(self, session_cwd: &Path) -> Result<McpServerConfig, String>;
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

#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub content: String,
    pub is_ok: bool,
    pub executed: bool,
}

impl ToolCallResult {
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_ok: false,
            executed: false,
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
        servers: Vec<McpServerConfig>,
    ) -> Result<(), String>;
    async fn clear_session(&self, session_id: &str);
    fn definitions(&self) -> Vec<Value>;
    fn prompt_fragment(&self) -> Option<String>;
    fn has_tools(&self) -> bool;
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
        _: Vec<McpServerConfig>,
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
    async fn call(&self, request: ToolCallRequest) -> ToolCallResult {
        ToolCallResult::error(format!("outil indisponible: {}", request.name))
    }
}

#[derive(Clone, Default)]
pub struct NullLlmProvider;

#[async_trait::async_trait]
impl LlmProvider for NullLlmProvider {
    async fn stream(&self, _: LlmRequest) -> Result<LlmStream, String> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }
    async fn upload_image(&self, _: &str, _: &str) -> Result<String, String> {
        Err("LLM provider indisponible".into())
    }
    fn model_info(&self, _: &str) -> LlmModelInfo {
        LlmModelInfo::default()
    }
}

pub type SharedLlmProvider = Arc<dyn LlmProvider>;
pub type SharedToolProvider = Arc<dyn ToolProvider>;
