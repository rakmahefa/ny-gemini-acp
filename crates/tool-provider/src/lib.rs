use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequest {
    pub name: String,
    pub arguments: Value,
    pub cwd: PathBuf,
    #[serde(default)]
    pub additional_directories: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResponse {
    pub content: String,
    pub is_error: bool,
}

#[async_trait]
pub trait ToolProvider: Send + Sync {
    fn definitions(&self) -> Vec<Value>;
    fn has_tools(&self) -> bool;
    async fn execute(&self, request: ToolRequest) -> Option<ToolResponse>;
}
