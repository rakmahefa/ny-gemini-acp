use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("agent turn has already been started")]
    AlreadyRunning,
    #[error("agent turn is already active for this session")]
    TurnAlreadyActive,
    #[error("agent turn task failed: {0}")]
    Task(String),
    #[error("agent turn cancellation channel is closed")]
    ChannelClosed,
}
