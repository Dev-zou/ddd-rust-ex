use crate::domain::error::{UserSessionsError, ResourcePoolError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Session(#[from] UserSessionsError),
    
    #[error(transparent)]
    Resource(#[from] ResourcePoolError),

    #[error("Concurrent modification detected")]
    OptimisticLockError,
    
    #[error("Session {0} holding resource")]
    SessionHoldingResource(String),
    
    #[error("Operation not allowed: {0}")]
    ForbiddenOperation(String),
}