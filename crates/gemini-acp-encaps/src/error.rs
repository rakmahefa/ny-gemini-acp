use thiserror::Error;

#[derive(Debug, Error)]
pub enum EncapsError {
    #[error("ACP thread is already running")]
    AlreadyRunning,
    #[error("ACP thread is not running")]
    NotRunning,
    #[error("ACP thread is shutting down")]
    ShuttingDown,
    #[error("ACP thread command channel is closed")]
    ChannelClosed,
    #[error("ACP thread task failed: {0}")]
    Task(String),
}
