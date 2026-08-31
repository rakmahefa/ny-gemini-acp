//! Types du module state : rôles, modes de session, données persistées, erreurs.

use crate::time;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

use super::History;

pub const MAX_SNAPSHOTS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    #[serde(rename = "user")]
    User,
    #[serde(rename = "assistant")]
    Assistant,
    #[serde(rename = "tool")]
    Tool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SessionMode {
    #[default]
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "accept_edits")]
    AcceptEdits,
    #[serde(rename = "bypass_permissions")]
    BypassPermissions,
}

impl SessionMode {
    pub fn all() -> &'static [SessionMode] {
        &[Self::Default, Self::AcceptEdits, Self::BypassPermissions]
    }

    pub fn from_str_lossy(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "default" => Some(Self::Default),
            "accept_edits" => Some(Self::AcceptEdits),
            "bypass_permissions" => Some(Self::BypassPermissions),
            _ => None,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Default => "Ask for permission",
            Self::AcceptEdits => "Auto-approve edits",
            Self::BypassPermissions => "Bypass all permissions",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Default => "Ask for permission before edits and commands.",
            Self::AcceptEdits => {
                "Edits run without prompting. High-risk commands still require explicit permission."
            }
            Self::BypassPermissions => "Edits and commands run without prompting.",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub cwd: PathBuf,
    pub additional_directories: Vec<PathBuf>,
    pub title: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub model: String,
    pub think: Option<u32>,
    pub tools_enabled: bool,
    #[serde(default)]
    pub mode: SessionMode,
    #[serde(default)]
    pub turn_count: u64,
    #[serde(alias = "history")]
    pub messages: History,
}

impl Session {
    pub fn new(
        id: String,
        cwd: PathBuf,
        additional_directories: Vec<PathBuf>,
        model: &str,
    ) -> Self {
        let now = time::now_iso();
        Self {
            id,
            cwd,
            additional_directories,
            title: None,
            created_at: now.clone(),
            updated_at: now,
            model: model.to_string(),
            think: None,
            tools_enabled: true,
            mode: SessionMode::Default,
            turn_count: 0,
            messages: History::new(),
        }
    }

    pub fn fork(&self, new_id: String) -> Self {
        let now = time::now_iso();
        Self {
            id: new_id,
            cwd: self.cwd.clone(),
            additional_directories: self.additional_directories.clone(),
            title: self.title.as_ref().map(|t| format!("{t} (fork)")),
            created_at: now.clone(),
            updated_at: now,
            model: self.model.clone(),
            think: self.think,
            tools_enabled: self.tools_enabled,
            mode: self.mode,
            turn_count: 0,
            messages: self.messages.clone(),
        }
    }
}

#[derive(Debug, Error)]
pub enum TurnError {
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("a turn is already active on this session — send session/cancel first")]
    AlreadyRunning,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("persisted session write failed: {0}")]
    Persistence(String),
    #[error("stale turn generation: expected {expected}, current {current}")]
    StaleGeneration { expected: u64, current: u64 },
    /// La session a été supprimée pendant le tour : le commit est abandonné
    /// plutôt que de ressusciter la session supprimée (D-05).
    #[error("session deleted during turn: {0}")]
    SessionDeleted(String),
}

/// Runtime session cache. Turn concurrency and cancellation are owned by `TurnManager`.
pub struct Live {
    pub session: Session,
    pub generation: u64,
}
