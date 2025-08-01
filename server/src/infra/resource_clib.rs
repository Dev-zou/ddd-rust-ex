use crate::infra::{error::ResourceError, resource_provider::ResourceProvider};

pub struct CLibResourceProvider;

impl ResourceProvider for CLibResourceProvider {
    fn allocate(&self, id: u32) -> Result<(), ResourceError> {
        Ok(())
    }

    fn release(&self, id: u32) -> Result<(), ResourceError> {
        Ok(())
    }
}