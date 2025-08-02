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

pub struct MockFailResourceProvider;

impl ResourceProvider for MockFailResourceProvider {
    fn allocate(&self, id: u32) -> Result<(), ResourceError> {
        if id == 15 {
            return Err(ResourceError::ResourceAllocFailed(id as u16));
        }
        Ok(())
    }

    fn release(&self, id: u32) -> Result<(), ResourceError> {
        if id == 0 {
            return Err(ResourceError::ResourceAllocFailed(id as u16));
        }
        Ok(())
    }
}