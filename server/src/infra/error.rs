use thiserror::Error;

#[derive(Debug, Error)]
pub enum ResourceError {
    #[error("Resource {0} alloc failed")]
    ResourceAllocFailed(u16),
}