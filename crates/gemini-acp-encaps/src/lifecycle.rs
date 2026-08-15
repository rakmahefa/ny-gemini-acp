/// Lifecycle of an encapsulated ACP execution thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Created,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
}

impl ThreadState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Stopped | Self::Failed)
    }
}
