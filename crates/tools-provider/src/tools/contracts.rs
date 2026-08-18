//! Runtime contracts consumed by tool execution.
//! The semantic lifecycle sink is owned by `agent-runtime`; this crate only
//! reuses it at the provider boundary.
pub use agent_runtime::ToolEventSink;

use std::sync::Arc;
use tokio::sync::{watch, Notify};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPermissionMode {
    Default,
    AcceptEdits,
    BypassPermissions,
}

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
