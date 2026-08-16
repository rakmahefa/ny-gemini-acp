//! Module `tools` — architecture d'outils pour l'agent ACP.
//!
//! Conception :
//! - [`executor`]  : exécution, permissions ACP et cycle de vie des tool calls.
//! - [`lifecycle`] : machine d'état déterministe interne, projetée sur les statuts ACP v1.
//! - [`request`]   : modèle normalisé `ToolCallRequest`, son type et sa machine d'état.
//! - [`tool_ux`]   : mapping UX ACP inspiré de `claude-agent-acp/src/tools.ts`.
//! - [`registry`]  : trait `Tool`, `ToolDef`, `ToolRegistry`, `ToolResult`.
//! - [`parse`]     : extraction des blocs `tool_call` depuis la réponse Gemini.
//! - [`prompt`]    : injection `# Tool Use` dans le prompt + formatage historique.
//! - [`tool_history`] : sérialisation sûre des résultats arbitraires d'outils.
//! - [`sandbox`]   : validation de sécurité (path traversal, shell sandbox).
//! - [`builtin`]   : outils intégrés.
//! - [`interactive`] : outils qui utilisent directement les capacités interactives ACP.

pub mod builtin;
pub mod executor;
pub mod interactive;
pub mod lifecycle;
pub mod lifecycle_events;
pub mod parse;
pub mod prompt;
pub mod registry;
pub mod request;
pub mod sandbox;
pub mod tool_history;
pub mod tool_ux;

pub use lifecycle::{LifecycleError, ToolLifecycle, ToolLifecycleState};
pub use lifecycle_events::{context as tool_event_context, emit_tool_state};
pub use registry::ToolRegistry;
pub use request::{ToolCallKind, ToolCallRequest, ToolCallRequestError, ToolCallState};
