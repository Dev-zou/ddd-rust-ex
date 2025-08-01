use thiserror::Error;

#[derive(Debug, Error)]
pub enum UserSessionsError {
    #[error("Session quota exceeded (max: {0})")]
    QuotaExceeded(u32),
    #[error("Session {0} not exists")]
    InvalidSession(String),
}

#[derive(Debug, Error)]
pub enum ResourcePoolError {
    #[error("Session {0} not exists")]
    InvalidSession(String),
    #[error("Resource {0} is already held by {1}")]
    ResourceConflict(u16, String),
    #[error("Resource {0} not found")]
    ResourceNotFound(u16),
    #[error("Resource {0} alloc failed")]
    ResourceAllocFailed(u16),
    #[error("Resource {0} is idle status")]
    ResourceIdle(u16),
}