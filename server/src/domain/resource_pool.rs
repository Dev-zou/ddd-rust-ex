use std::sync::Arc;
use std::time::SystemTime;
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
}

impl ResourcePool {
    pub fn new(max_resource_num: usize, provider: Arc<dyn ResourceProvider>) -> Self {
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
            provider
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
            // {
                let mut guard = resource.write().await;
                if matches!(guard.status, ResourceStatus::InUse | ResourceStatus::Applying) {
                    guard.status = ResourceStatus::Releasing;
                    drop(guard);
                } else {
                    return Err(ResourcePoolError::ResourceIdle(resource_id));
                }
            // }

            // 调用外部接口
            let _release_result = self.provider.release(resource_id.into());

            {
                let mut guard = resource.write().await;
                guard.status = ResourceStatus::Idle;
                guard.holder_id = None;
                guard.allocation_time = None;
            }
            return Ok(());
        }
        Err(ResourcePoolError::ResourceNotFound(resource_id))
    }

    // 获取超时资源列表（持有超过10秒）
    pub async fn get_timeout_resources(&self, timeout_secs: u64) -> Vec<(u16, String)> {
        let mut timeouts = Vec::new();
        for resource in &self.resources {
            let guard = resource.read().await;
            if guard.status == ResourceStatus::InUse {
                if let (Some(holder), Some(allocation_time)) = (&guard.holder_id, guard.allocation_time) {
                    if allocation_time.elapsed().unwrap().as_secs() >= timeout_secs {
                        timeouts.push((guard.id, holder.clone()));
                    }
                }
            }
        }
        timeouts
    }

    // 检查资源是否被特定会话持有
    pub async fn is_held_by_session(&self, resource_id: u16, session_id: &str) -> bool {
        if let Some(resource) = self.resources.get(resource_id as usize) {
            let guard = resource.read().await;
            guard.status == ResourceStatus::InUse && 
            guard.holder_id.as_ref() == Some(&session_id.to_string())
        } else {
            false
        }
    }

    // 辅助方法：打印资源状态
    pub async fn status_report(&self) -> Vec<String> {
        let mut statuss = Vec::with_capacity(self.resources.len());
        for id in 0..self.resources.len() {
            let resource = self.resources.get(id).unwrap().read().await;
            statuss.push(
                format!(
                    "Resource {}: Status={:?}, Holder={:?}",
                    resource.id,
                    resource.status,
                    resource.holder_id
                )
            );
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
}


#[cfg(test)]
mod tests {
    use tokio::sync::Barrier;
    use crate::{app::config, infra::resource_mock::MockResourceProvider};

    use super::*;
    // use mockall::automock;

    // #[automock]
    // impl Allocator for MockAllocator {} // 自动生成 MockAllocator

    // #[test]
    // fn test_allocate_returns_true() {
    //     let mut mock = MockAllocator::new();
    //     // 固定返回 true
    //     mock.expect_allocate_res()
    //         .with(eq(42)) // 可指定参数匹配（如 id=42）
    //         .returning(|_| true); // 无论输入如何，均返回 true

    //     assert!(allocate_resource(&mock, 42)); // 测试通过
    // }

    #[tokio::test]
    async fn test_resource_pool_init() {
        let pool = ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider));
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
        let pool = ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider));
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

    // #[tokio::test]
    // async fn test_resource_pool_allocate_resource_failure() {
    //     let pool = ResourcePool::new(config::MAX_RESOURCE_NUM);
    //     // 强制模拟ID=9的资源分配失败
    //     assert!(pool.allocate_resource(9, "holder".to_string()).await.is_err());
    //     {
    //         let resource = pool.resources[9].read().await;
    //         assert_eq!(resource.status, ResourceStatus::Idle); // 失败后状态回滚
    //     }
    // }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_resource_pool_concurrent_allocation() {
        let pool = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider)));
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
            if i < 10 { // 排除强制失败的ID=9
                assert!(line.contains("InUse"), "Resource {} should be InUse", i);
            }

            // if i < 10 && i != 9 { // 排除强制失败的ID=9
            //     assert!(line.contains("InUse"), "Resource {} should be InUse", i);
            // } else if i == 9 {
            //     assert!(line.contains("Idle"), "Resource 9 should fail and remain Idle");
            // }
        }
    }
}