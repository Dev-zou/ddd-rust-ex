use crate::infra::{error::ResourceError, resource_provider::ResourceProvider};

pub struct MockResourceProvider;

impl ResourceProvider for MockResourceProvider {
    fn allocate(&self, id: u32) -> Result<(), ResourceError> {
        Ok(())
    }

    fn release(&self, id: u32) -> Result<(), ResourceError> {
        Ok(())
    }
}