use thiserror::Error;

#[derive(Debug, Error)]
pub enum EncapsError {
    #[error("ACP thread has already been started")]
    AlreadyStarted,
    #[error("ACP turn has already been started")]
    AlreadyRunning,
    #[error("ACP turn is already active for this session")]
    TurnAlreadyActive,
    #[error("ACP thread is not running")]
    NotRunning,
    #[error("ACP thread is shutting down")]
    ShuttingDown,
    #[error("ACP thread command channel is closed")]
    ChannelClosed,
    #[error("ACP thread task failed: {0}")]
    Task(String),
}
