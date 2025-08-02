use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use crate::domain::error::ResourcePoolError;
use crate::infra::resource_provider::ResourceProvider;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ResourceStatus {
    Idle,
    Applying,
    Releasing,
    InUse,
}

#[derive(Clone)]
#[allow(dead_code)]
struct Resource {
    id: u16,
    status: ResourceStatus,
    holder_id: Option<String>,
    allocation_time: Option<SystemTime>,
}

#[derive(Clone)]
pub struct ResourcePool {
    resources: Vec<Arc<RwLock<Resource>>>,
    provider: Arc<dyn ResourceProvider>,
    timeout: Duration, // 10秒超时阈值
}

impl ResourcePool {
    pub fn new(max_resource_num: usize, provider: Arc<dyn ResourceProvider>, timeout: usize) -> Self {
        let mut resources = Vec::with_capacity(max_resource_num);
        for id in 0..max_resource_num {
            resources.push(Arc::new(RwLock::new(Resource {
                id: id.try_into().unwrap(),
                status: ResourceStatus::Idle,
                holder_id: None,
                allocation_time: None,
            })));
        }
        Self {
            resources,
            provider,
            timeout: Duration::from_secs(timeout as u64),
        }
    }

    pub async fn allocate_resource(&self, resource_id: u16, holder_id: String) -> Result<(), ResourcePoolError> {
        if let Some(resource) = self.resources.get(resource_id as usize) {
            let mut guard = resource.write().await;
            if matches!(guard.status, ResourceStatus::Idle) {
                guard.status = ResourceStatus::Applying;
                drop(guard);
            } else {
                return Err(ResourcePoolError::ResourceConflict(resource_id, holder_id));
            }

            // 模拟调用第三方接口
            let allocation_result = self.provider.allocate(resource_id.into());

            if allocation_result.is_ok() {
                let mut guard = resource.write().await;
                guard.status = ResourceStatus::InUse;
                guard.holder_id = Some(holder_id);
                guard.allocation_time = Some(SystemTime::now());
                drop(guard);
                return Ok(());
            }
            let mut guard = resource.write().await;
            guard.status = ResourceStatus::Idle;
            drop(guard);
            return Err(ResourcePoolError::ResourceAllocFailed(resource_id));
        }
        Err(ResourcePoolError::ResourceNotFound(resource_id))
    }

    // 3. 释放资源方法
    pub async fn release_resource(&self, resource_id: u16) -> Result<(), ResourcePoolError> {
        if let Some(resource) = self.resources.get(resource_id as usize) {
                let mut guard = resource.write().await;
                if matches!(guard.status, ResourceStatus::InUse | ResourceStatus::Applying) {
                    guard.status = ResourceStatus::Releasing;
                    drop(guard);
                } else {
                    return Err(ResourcePoolError::ResourceIdle(resource_id));
                }

            // 调用外部接口
            let release_result = self.provider.release(resource_id.into());

            if release_result.is_ok() {
                let mut guard = resource.write().await;
                guard.status = ResourceStatus::Idle;
                guard.holder_id = None;
                guard.allocation_time = None;
                drop(guard);
                return Ok(());
            }
            let mut guard = resource.write().await;
            guard.status = ResourceStatus::InUse;
            drop(guard);
            return Err(ResourcePoolError::ResourceAllocFailed(resource_id));
        }
        Err(ResourcePoolError::ResourceNotFound(resource_id))
    }

    // 获取超时资源列表（持有超过10秒）
    pub async fn get_timeout_resources(&self) -> Vec<(u16, String)> {
        let mut timeouts = Vec::new();
        for resource in &self.resources {
            let guard = resource.read().await;
            if guard.status == ResourceStatus::InUse {
                if let (Some(holder), Some(allocation_time)) = (&guard.holder_id, guard.allocation_time) {
                    if let Ok(elapsed) = allocation_time.elapsed() {
                        if elapsed > self.timeout {
                            timeouts.push((guard.id, holder.clone()));
                        }
                    }
                }
            }
        }
        timeouts
    }

    // 辅助方法：打印资源状态
    pub async fn status_report(&self) -> Vec<String> {
        let mut statuss = Vec::with_capacity(self.resources.len());
        for id in 0..self.resources.len() {
            let resource = self.resources.get(id).unwrap().read().await;
            statuss.push(format!("Resource {}: Status={:?}, Holder={:?}", resource.id, resource.status, resource.holder_id));
        }
        statuss
    }

    pub async fn get_holder(&self, resource_id: u16) -> Result<Option<String>, ResourcePoolError> {
        if let Some(resource) = self.resources.get(resource_id as usize) {
            // 将锁的作用域限定在holder_id的访问中
            let holder_id = {
                let guard = resource.read().await; // 读锁作用域开始
                match guard.status {
                    ResourceStatus::InUse => guard.holder_id.clone(),
                    _ => None // Idle或Applying状态返回None
                }
            }; // 此处guard离开作用域，锁自动释放
            return Ok(holder_id);
        }
        Err(ResourcePoolError::ResourceNotFound(resource_id))
    }

    pub async fn set_resource_allocation_time(&self, resource_id: u16, time: Option<SystemTime>) -> Result<(), ResourcePoolError> {
        if let Some(resource) = self.resources.get(resource_id as usize) {
            let mut guard = resource.write().await;
            guard.allocation_time = time;
            Ok(())
        } else {
            Err(ResourcePoolError::ResourceNotFound(resource_id))
        }
    }
}


#[cfg(test)]
mod tests {
    use tokio::{sync::Barrier, time::Instant};
    use crate::{app::config, infra::resource_mock::{MockResourceProvider, MockFailResourceProvider}};

    use super::*;

    #[tokio::test]
    async fn test_resource_pool_init() {
        let pool = ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS);
        assert_eq!(pool.resources.len(), 16);
        for (i, resource) in pool.resources.iter().enumerate() {
            let guard = resource.read().await;
            assert_eq!(guard.id, i as u16);
            assert_eq!(guard.status, ResourceStatus::Idle);
            assert!(guard.holder_id.is_none());
            assert!(guard.allocation_time.is_none());
        }
    }

    #[tokio::test]
    async fn test_resource_pool_allocate_and_release_resource() {
        let pool = ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS);
        let holder = "test_holder".to_string();

        // 成功分配
        assert!(pool.allocate_resource(0, holder.clone()).await.is_ok());
        {
            let resource = pool.resources[0].read().await;
            assert_eq!(resource.status, ResourceStatus::InUse);
            assert_eq!(resource.holder_id, Some(holder.clone()));
        }

        // 重复分配失败
        assert!(pool.allocate_resource(0, holder.clone()).await.is_err());

        // 成功释放
        assert!(pool.release_resource(0).await.is_ok());
        {
            let resource = pool.resources[0].read().await;
            assert_eq!(resource.status, ResourceStatus::Idle);
            assert!(resource.holder_id.is_none());
        }

        // 释放后重新分配成功
        assert!(pool.allocate_resource(0, holder).await.is_ok());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_resource_pool_concurrent_allocation() {
        let pool = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS));
        let barrier = Arc::new(Barrier::new(10)); // 同步10个任务的启动
        let mut handles = vec![];

        // 模拟10个异步任务并发分配不同资源
        for i in 0..10 {
            let pool = pool.clone();
            let barrier = barrier.clone();
            let holder = format!("task_{}", i);
            handles.push(tokio::spawn(async move {
                // 等待所有任务就绪，确保并发性
                barrier.wait().await;
                
                match pool.allocate_resource(i, holder).await {
                    Ok(_) => println!("Task {} allocated resource {}", i, i),
                    Err(e) => println!("Task {} failed: {}", i, e),
                }
            }));
        }

        // 等待所有任务完成
        for handle in handles {
            handle.await.unwrap();
        }

        // 验证分配结果
        let status = pool.status_report().await;
        for (i, line) in status.iter().enumerate() {
            if i < 10 {
                assert!(line.contains("InUse"), "Resource {} should be InUse", i);
            }
        }
    }

    #[tokio::test]
    async fn test_get_timeout_resources() {
        // 创建资源池
        let resource_pool = Arc::new(ResourcePool::new(
            config::MAX_RESOURCE_NUM, 
            Arc::new(MockResourceProvider), 
            config::RESOURCE_TIMEOUT_SECS
        ));
        
        // 初始化状态下，没有超时资源
        let timeout_resources = resource_pool.get_timeout_resources().await;
        assert!(timeout_resources.is_empty());
        
        // 分配一些资源
        let session_id = "test_session".to_string();
        resource_pool.allocate_resource(1, session_id.clone()).await.unwrap();
        resource_pool.allocate_resource(2, session_id.clone()).await.unwrap();
        
        // 刚分配的资源不会超时
        let timeout_resources = resource_pool.get_timeout_resources().await;
        assert!(timeout_resources.is_empty());
        
        // 修改资源的分配时间，模拟超时
        {
            let resource = &resource_pool.resources[1];
            let mut guard = resource.write().await;
            guard.allocation_time = Some(SystemTime::now() - Duration::from_secs(config::RESOURCE_TIMEOUT_SECS as u64 + 1));
        }
        
        // 现在应该有一个超时资源
        let timeout_resources = resource_pool.get_timeout_resources().await;
        assert_eq!(timeout_resources.len(), 1);
        assert_eq!(timeout_resources[0].0, 1);
        assert_eq!(timeout_resources[0].1, session_id.clone());
        
        // 模拟另一个资源超时
        {
            let resource = &resource_pool.resources[2];
            let mut guard = resource.write().await;
            guard.allocation_time = Some(SystemTime::now() - Duration::from_secs(config::RESOURCE_TIMEOUT_SECS as u64 + 1));
        }
        
        // 现在应该有两个超时资源
        let timeout_resources = resource_pool.get_timeout_resources().await;
        assert_eq!(timeout_resources.len(), 2);
        assert!(timeout_resources.iter().any(|&(id, _)| id == 1));
        assert!(timeout_resources.iter().any(|&(id, _)| id == 2));
        
        // 释放一个资源
        resource_pool.release_resource(1).await.unwrap();
        
        // 释放的资源不再超时
        let timeout_resources = resource_pool.get_timeout_resources().await;
        assert_eq!(timeout_resources.len(), 1);
        assert_eq!(timeout_resources[0].0, 2);
    }

    #[tokio::test]
    async fn test_allocate_release_resource_fail() {
        let pool = ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockFailResourceProvider), config::RESOURCE_TIMEOUT_SECS);
        let holder = "test_holder".to_string();

        // 成功分配
        assert!(pool.allocate_resource(0, holder.clone()).await.is_ok());
        {
            let resource = pool.resources[0].read().await;
            assert_eq!(resource.status, ResourceStatus::InUse);
            assert_eq!(resource.holder_id, Some(holder.clone()));
        }
        // 分配失败
        assert!(pool.allocate_resource(15, holder.clone()).await.is_err());
        {
            let resource = pool.resources[15].read().await;
            assert_eq!(resource.status, ResourceStatus::Idle);
        }

        // 释放失败
        assert!(pool.release_resource(0).await.is_err());
    }
}