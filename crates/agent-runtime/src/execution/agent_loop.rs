use std::collections::HashSet;
use std::sync::Arc;

use crate::events::{consume_model_stream, ModelProjectionError, ModelRound, PendingToolCall, TurnEventEmitter};
use crate::state::{Role, Session};
use crate::{
    Cancellation, GenerationOptions, LlmError, LlmProvider, ModelRequest,
    ToolCallRequest, ToolCallResult, ToolPermissionDecision, ToolPermissionHandler,
    ToolPermissionRequest, ToolProvider,
};

// ...
