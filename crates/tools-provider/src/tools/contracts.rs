//! Runtime contracts consumed by tool execution.
//!
//! The tool provider must not depend on the agent runtime. These small contracts
//! keep cancellation, permission policy, and semantic event emission at the
//! provider boundary.

use std::sync::Arc;

use tokio::sync::{watch, Notify};

/// Permission mode projected from the agent session policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermissionMode {
    Default,
    AcceptEdits,
    BypassPermissions,
}

/// Runtime-owned cancellation observed by tool execution.
#[derive(Clone)]
pub struct ToolCancellation {
    cancelled: Arc<watch::Receiver<bool>>,
    notify: Arc<Notify>,
}

impl ToolCancellation {
    pub fn from_receiver(receiver: watch::Receiver<bool>) -> Self {
        let notify = Arc::new(Notify::new());
        let mut observed = receiver.clone();
        let notify_clone = notify.clone();
        tokio::spawn(async move {
            if *observed.borrow() {
                notify_clone.notify_waiters();
                return;
            }
            while observed.changed().await.is_ok() {
                if *observed.borrow() {
                    notify_clone.notify_waiters();
                    break;
                }
            }
        });
        Self {
            cancelled: Arc::new(receiver),
            notify,
        }
    }

    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }
}

/// Semantic events emitted by tool execution without coupling the provider to
/// the runtime's event bus implementation.
pub trait ToolEventSink {
    fn tool_call_requested(&mut self, upstream_id: String, name: String) -> bool;
    fn permission_requested(&mut self, upstream_id: String) -> bool;
    fn tool_execution_started(&mut self, upstream_id: String) -> bool;
    fn tool_result_received(&mut self, upstream_id: String, result: String) -> bool;
}
