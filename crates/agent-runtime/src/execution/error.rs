use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("agent thread has already been started")]
    AlreadyStarted,
    #[error("agent turn has already been started")]
    AlreadyRunning,
    #[error("agent turn is already active for this session")]
    TurnAlreadyActive,
    #[error("agent thread is not running")]
    NotRunning,
    #[error("agent thread is shutting down")]
    ShuttingDown,
    #[error("agent thread command channel is closed")]
    ChannelClosed,
    #[error("agent thread task failed: {0}")]
    Task(String),
}
