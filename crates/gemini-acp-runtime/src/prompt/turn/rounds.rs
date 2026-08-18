use agent_client_protocol::schema::v1::{MessageId, SessionId, StopReason};
use agent_client_protocol::{Client, ConnectionTo};
use gemini_acp_config::{LlmProvider, LlmRequest};
use gemini_acp_runtime::events::TurnEventEmitter;
use gemini_acp_runtime::state::{Role, Session};
use gemini_acp_runtime::tools::executor::{emit_error_chunk, ToolExecutor};
use gemini_acp_runtime::tools::ToolRegistry;
use tokio::sync::watch;

use super::context::{compact_messages, COMPACTION_THRESHOLD_CHARS, EMERGENCY_COMPACTION_CHARS};
use crate::prompt::error::actionable_error_message;
use crate::prompt::follow_up::{replace_components, request_action, FollowUpError, FollowUpOutcome};
use crate::prompt::stream;
use gemini_acp_runtime::tools::lifecycle::clear_partial_output;

// ... existing round implementation remains unchanged except that the provider
// boundary is imported from the unified llm-provider crate.
