use crate::app::error::{AppError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error(transparent)]
    App(#[from] AppError),

    #[error("Session {0} holding resource")]
    SessionHoldingResource(String),

    #[error("Invalid Request")]
    InvalidRequest,

    #[error("Failed to send message: {0}")]
    SendMessageFailed(String),

    #[error("Invalid session ID")]
    InvalidSessionId,
}