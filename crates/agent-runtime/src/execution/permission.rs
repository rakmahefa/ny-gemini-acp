use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::Value;

use crate::Cancellation;
use crate::state::Session;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPermissionDecision {
    Allow,
    Reject(String),
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct ToolPermissionRequest {
    pub session_id: String,
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
    pub cwd: PathBuf,
    pub additional_dirs: Vec<PathBuf>,
}

#[async_trait]
pub trait ToolPermissionHandler: Send + Sync {
    fn needs_permission(&self, session: &Session, request: &ToolPermissionRequest) -> bool;

    async fn request_permission(
        &self,
        session: &Session,
        request: &ToolPermissionRequest,
        cancellation: Cancellation,
    ) -> ToolPermissionDecision;
}
