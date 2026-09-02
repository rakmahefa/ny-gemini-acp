//! Session lifecycle and persistence.
use crate::providers::{
    NullToolProvider, SharedToolProvider, ToolConfigurationError, ToolServerConfig,
};
use crate::state::{Session, SessionMode, Store};
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const SESSION_ID_PREFIX: &str = "sess_";
pub const MAX_TITLE_LENGTH: usize = 256;

#[derive(Debug, thiserror::Error)]
pub enum SessionToolConfigurationError {
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error(transparent)]
    Tool(#[from] ToolConfigurationError),
}

#[derive(Clone)]
pub struct SessionManager {
    store: Arc<Store>,
    tools: SharedToolProvider,
}

impl SessionManager {
    pub fn new(store: Arc<Store>) -> Self {
        Self::with_tool_provider(store, Arc::new(NullToolProvider))
    }

    pub fn with_tool_provider(store: Arc<Store>, tools: SharedToolProvider) -> Self {
        Self { store, tools }
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub async fn clear_mcp(&self, id: &str) {
        self.tools.clear_session(id).await;
    }

    /// Canonical typed MCP/session configuration contract for runtime callers.
    pub async fn configure_mcp_typed(
        &self,
        id: &str,
        servers: Vec<ToolServerConfig>,
    ) -> std::result::Result<(), SessionToolConfigurationError> {
        let session = self
            .store
            .get(id)
            .await
            .ok_or_else(|| SessionToolConfigurationError::SessionNotFound(id.to_string()))?;
        self.tools
            .configure_session(id, session.cwd, servers)
            .await
            .map_err(SessionToolConfigurationError::from)
    }

    /// ACP compatibility boundary: protocol callers receive a presentation string,
    /// while the runtime keeps the canonical typed error above.
    pub async fn configure_mcp(
        &self,
        id: &str,
        servers: Vec<ToolServerConfig>,
    ) -> Result<(), String> {
        self.configure_mcp_typed(id, servers)
            .await
            .map_err(|error| error.to_string())
    }

    pub fn validate_id(id: &str) -> Result<()> {
        let Some(rest) = id.strip_prefix(SESSION_ID_PREFIX) else {
            bail!("invalid session id: expected prefix `{SESSION_ID_PREFIX}`");
        };
        if rest.len() != 32
            || !rest
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            bail!("invalid session id: expected lowercase hexadecimal UUID");
        }
        Ok(())
    }

    pub async fn validate_cwd(cwd: &Path) -> Result<()> {
        if !cwd.is_absolute() {
            bail!("session path must be absolute");
        }
        let metadata = tokio::fs::metadata(cwd)
            .await
            .with_context(|| format!("workspace not accessible: {}", cwd.display()))?;
        if !metadata.is_dir() {
            bail!("workspace is not a directory: {}", cwd.display());
        }
        Ok(())
    }

    pub fn sanitize_title(text: &str) -> Option<String> {
        let title = text
            .replace(['\r', '\n'], " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if title.is_empty() {
            return None;
        }
        let mut chars = title.chars();
        let truncated: String = chars.by_ref().take(MAX_TITLE_LENGTH).collect();
        if chars.next().is_some() {
            let keep = MAX_TITLE_LENGTH.saturating_sub(1);
            Some(format!(
                "{}…",
                truncated.chars().take(keep).collect::<String>()
            ))
        } else {
            Some(truncated)
        }
    }

    pub async fn create(
        &self,
        cwd: PathBuf,
        additional_directories: Vec<PathBuf>,
        model: &str,
    ) -> Result<Session> {
        Self::validate_cwd(&cwd).await?;
        for directory in &additional_directories {
            Self::validate_cwd(directory).await.with_context(|| {
                format!("répertoire additionnel invalide: {}", directory.display())
            })?;
        }
        self.store
            .create(cwd, additional_directories, model)
            .await
            .context("session creation")
    }

    pub async fn get(&self, id: &str) -> Result<Session> {
        Self::validate_id(id)?;
        self.store
            .get(id)
            .await
            .ok_or_else(|| anyhow::anyhow!("session not found: {id}"))
    }

    pub async fn list(&self, cwd: Option<&Path>) -> Result<Vec<Session>> {
        if let Some(cwd) = cwd {
            Self::validate_cwd(cwd).await?;
        }
        Ok(self.store.list(cwd).await)
    }

    pub async fn load(&self, id: &str, cwd: &Path) -> Result<Session> {
        Self::validate_id(id)?;
        Self::validate_cwd(cwd).await?;
        let session = self.get(id).await?;
        if session.cwd != cwd {
            bail!("cwd does not match the session");
        }
        Ok(session)
    }

    pub async fn resume(&self, id: &str, cwd: &Path) -> Result<Session> {
        self.load(id, cwd).await
    }

    pub async fn set_mode(&self, id: &str, mode: SessionMode) -> Result<Session> {
        let mut updated = self.get(id).await?;
        self.store
            .update_session(id, |session| session.mode = mode)
            .await
            .context("failed to update session mode")?;
        updated.mode = mode;
        Ok(updated)
    }

    pub async fn fork(&self, id: &str) -> Result<Session> {
        self.get(id).await?;
        self.store.fork(id).await.context("fork de session")
    }

    pub async fn close(&self, id: &str) -> Result<bool> {
        Self::validate_id(id)?;
        let closed = self.store.close(id).await;
        if closed {
            self.clear_mcp(id).await;
        }
        Ok(closed)
    }

    pub async fn delete(&self, id: &str) -> Result<bool> {
        Self::validate_id(id)?;
        let deleted = self.store.delete(id).await;
        if deleted {
            self.clear_mcp(id).await;
        }
        Ok(deleted)
    }
}

#[cfg(test)]
#[path = "test/session.rs"]
mod tests;
