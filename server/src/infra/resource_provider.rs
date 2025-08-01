use crate::infra::error::ResourceError;


pub trait ResourceProvider: Send + Sync {
    fn allocate(&self, id: u32) -> Result<(), ResourceError>;
    fn release(&self, id: u32) -> Result<(), ResourceError>;
}