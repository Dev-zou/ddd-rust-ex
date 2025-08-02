use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

use crate::app::error::AppError;
use crate::domain::user_sessions::{UserSessions};
use crate::domain::resource_pool::ResourcePool;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

pub struct ResourceAppService {
    user_sessions: Arc<UserSessions>,
    resource_pool: Arc<ResourcePool>,
}

impl ResourceAppService {
    pub fn new(
        user_sessions: Arc<UserSessions>,
        resource_pool: Arc<ResourcePool>,
    ) -> Self {
        Self { 
            user_sessions, 
            resource_pool,
        }
    }

    /// 处理资源申请请求
    pub async fn handle_allocate(
        &self,
        session_id: &str,
        resource_ids: Vec<u16>,
    ) -> Result<(Vec<u16>, Vec<(u16, String)>), AppError> {
        // 检查会话是否已持有资源
        self.user_sessions.validate_and_start_allocate(session_id).await?;

        // 申请资源并更新会话状态
        let mut success_res = Vec::new();
        let mut failed_resources = Vec::new();

        for res_id in resource_ids {
            match self.resource_pool.allocate_resource(res_id, session_id.to_string()).await {
                Ok(_) => success_res.push(res_id),
                Err(e) => failed_resources.push((res_id, e.to_string())),
            }
        }

        if success_res.is_empty() {
            self.user_sessions.rollback_applying_session(session_id).await?;
        } else {
            self.user_sessions.user_add_resources(session_id, success_res.clone()).await?;
        }

        Ok((success_res, failed_resources))
    }

    /// 处理资源查询请求
    pub async fn handle_query(
        &self,
        session_id: &str,
    ) -> Result<Vec<u16>, AppError> {
        self.user_sessions.user_get_resources(session_id)
            .await
            .map_err(|e| AppError::Session(e))
    }

    /// 处理资源释放请求
    pub async fn handle_release(
        &self,
        session_id: &str,
        resource_ids: Vec<u16>,
    ) -> Result<(Vec<u16>, Vec<(u16, String)>), AppError> {
        let mut success_res = Vec::new();
        let mut failed_resources = Vec::new();
        for res_id in resource_ids {
            match self.resource_pool.release_resource(res_id).await {
                Ok(_) => success_res.push(res_id),
                Err(e) => failed_resources.push((res_id, e.to_string())),
            }
        }
        
        self.user_sessions.user_release_resources(session_id, success_res.clone()).await?;
        Ok((success_res, failed_resources))
    }

    /// 启动超时检测任务
    pub async fn get_and_release_timeout_resources(&self) -> HashMap<String, Vec<u16>> {
        let pool = self.resource_pool.clone();
        let sessions = self.user_sessions.clone();
        
        let mut timeouts_resources = HashMap::new();
        // 检测超时资源（10秒超时）
        let timeouts = pool.get_timeout_resources().await;
        for (resource_id, session_id) in timeouts {
            // 释放资源
            if let Err(e) = pool.release_resource(resource_id).await {
                tracing::info!("超时释放失败: {:?}", e);
                continue;
            }

            // 清理会话
            if let Err(e) = sessions.user_release_resources(&session_id, vec![resource_id]).await {
                tracing::info!("会话清理失败: {:?}", e);
            }
            
            if(timeouts_resources.get(&session_id).is_none()){
                timeouts_resources.insert(session_id, vec![resource_id]);
            } else {
                timeouts_resources.get_mut(&session_id).unwrap().push(resource_id);
            }
        }
        timeouts_resources
    }
}

#[cfg(test)]
mod tests {
    use crate::app::session_app::{SessionAppService};
    use crate::app::config;
    use crate::infra::resource_mock::MockResourceProvider;

    use super::*;
    // use tokio::sync::RwLock;

    async fn mock_setup_service(session_ids: Vec<String>) -> ResourceAppService {
        let user_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let resource_pool = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS));
        let session_service = SessionAppService::new(user_sessions.clone());

        // 添加session
        for session_id in session_ids {
            if let Err(e) = session_service.handle_add_session(session_id.clone()).await {
                break;
            }
        }
        let app = ResourceAppService::new(user_sessions.clone(), resource_pool.clone());
        app
    }

    #[tokio::test]
    async fn test_allocate_and_release() {
        // 初始化依赖
        let user_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let resource_pool = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS));
        let resource_service = ResourceAppService::new(user_sessions.clone(), resource_pool.clone());
        let session_service = SessionAppService::new(user_sessions.clone());

        // 添加session
        let session_id = "session1".to_string();
        session_service.handle_add_session(session_id.clone()).await.unwrap();
        let resource_ids = vec![1];

        // 测试资源申请
        let alloc_success = resource_service.handle_allocate(&session_id, resource_ids.clone()).await.unwrap();
        assert_eq!(alloc_success.0, resource_ids);
        assert!(resource_service.user_sessions.user_has_resources(&session_id).await.unwrap());
        assert_eq!(resource_service.resource_pool.get_holder(1).await.unwrap(), Some(session_id.clone()));

        // 测试资源释放
        resource_service.handle_release(&session_id, resource_ids).await.unwrap();
        assert!(!resource_service.user_sessions.user_has_resources(&session_id).await.unwrap());
        assert!(resource_service.resource_pool.get_holder(1).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_duplicate_allocation_and_invalid_release() {
        let session_ids = vec!["session1".to_string()];
        let service = Arc::new(mock_setup_service(session_ids.clone()).await);
        let resource_ids = vec![1];

        // 重复申请应返回空成功资源和失败资源
        service.handle_allocate(&session_ids[0], resource_ids.clone()).await.unwrap();
        assert!(service.handle_allocate(&session_ids[0], resource_ids.clone()).await.is_err());

        // 释放未持有的资源应失败
        let invalid_resource_id = vec![2];
        let (success_res, failed_resources) = service.handle_release(&session_ids[0], invalid_resource_id).await.unwrap();
        assert!(success_res.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_allocation() {
        let session_ids: Vec<String> = (0..10)
            .map(|i| format!("session{}", i))
            .collect();
        let service = Arc::new(mock_setup_service(session_ids.clone()).await);
        let mut handles = vec![];

        // 模拟10个会话并发申请不同资源
        for i in 0..10 {
            let service_clone = service.clone();
            let session_id = session_ids[i].clone();
            let resource_ids: Vec<u16> = vec![i.try_into().unwrap()];
            handles.push(tokio::spawn(async move {
                service_clone.handle_allocate(&session_id, resource_ids).await
            }));
        }

        // 检查10个任务的返回结果
        let results = futures::future::join_all(handles).await;
        let success_count = results.iter().filter(|r| r.as_ref().unwrap().is_ok()).count();
        assert_eq!(success_count, 10);
        // 验证所有申请成功且资源归属正确
        for (index, session_id) in session_ids.iter().enumerate() {
            assert_eq!(service.resource_pool.get_holder(index as u16).await.unwrap(), Some(session_id.clone()));
        }

    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_resource_contention() {
        let session_ids = vec!["session1".to_string(), "session2".to_string()];
        let service = Arc::new(mock_setup_service(session_ids.clone()).await);
        let resource_ids = vec![1];

        // 保存会话ID的克隆用于断言
        let session_id1_assert = session_ids[0].clone();
        let session_id2_assert = session_ids[1].clone();

        // 启动两个任务竞争同一资源，只有一个任务成功
        let session_id1 = session_ids[0].clone();
        let task1 = tokio::spawn({
            let service = service.clone();
            let resource_clone = resource_ids.clone();
            async move { service.handle_allocate(&session_id1, resource_clone).await }
        });

        let session_id2 = session_ids[1].clone();
        let task2 = tokio::spawn({
            let service = service.clone();
            let resource_clone = resource_ids.clone();
            async move { service.handle_allocate(&session_id2, resource_clone).await }
        });

        // 检查两个任务的返回结果
        let results = futures::future::join_all(vec![task1, task2]).await;

        // 统计成功和失败的任务数
        let mut success_count = 0;
        let mut failed_count = 0;
        for result in results {
            match result.unwrap() {
                Ok((success_res, failed_res)) => {
                    if !success_res.is_empty() {
                        success_count += 1;
                    } else if !failed_res.is_empty() {
                        failed_count += 1;
                    }
                },
                Err(_) => failed_count += 1,
            }
        }

        // 应该有一个任务成功，一个任务失败
        assert_eq!(success_count, 1);
        assert_eq!(failed_count, 1);

        // 验证资源确实被其中一个会话持有
        let holder = service.resource_pool.get_holder(1).await.unwrap();
        assert!(holder.is_some());
        assert!(holder == Some(session_id1_assert) || holder == Some(session_id2_assert));
    }

    #[tokio::test]
    async fn test_handle_query() {
        // 初始化依赖
        let user_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let resource_pool = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS));
        let resource_service = ResourceAppService::new(user_sessions.clone(), resource_pool.clone());
        let session_service = SessionAppService::new(user_sessions.clone());

        // 添加session
        let session_id = "session1".to_string();
        session_service.handle_add_session(session_id.clone()).await.unwrap();
        let resource_ids = vec![1, 2, 3];

        // 申请资源
        resource_service.handle_allocate(&session_id, resource_ids.clone()).await.unwrap();

        // 查询资源
        let queried_resources = resource_service.handle_query(&session_id).await.unwrap();
        assert_eq!(queried_resources, resource_ids);

        // 释放部分资源
        resource_service.handle_release(&session_id, vec![2]).await.unwrap();

        // 再次查询资源
        let queried_resources = resource_service.handle_query(&session_id).await.unwrap();
        assert_eq!(queried_resources, vec![1, 3]);

        // 查询不存在的会话
        let result = resource_service.handle_query("non_existent_session").await;
        assert!(result.is_err());
    }

    // 异常场景测试: 申请超过最大数量的资源
    #[tokio::test]
    async fn test_allocate_exceed_max_resources() {
        let session_ids = vec!["session1".to_string()];
        let service = mock_setup_service(session_ids.clone()).await;
        
        // 尝试申请超过最大数量的资源
        let resource_ids: Vec<u16> = (0..config::MAX_RESOURCE_NUM + 10).map(|i| i as u16).collect();
        let result = service.handle_allocate(&session_ids[0], resource_ids).await;
        
        // 应该成功申请部分资源，失败部分资源
        assert!(result.is_ok());
        let (success_res, failed_resources) = result.unwrap();
        assert_eq!(success_res.len(), config::MAX_RESOURCE_NUM as usize);
        assert_eq!(failed_resources.len(), 10);
    }

    // 异常场景测试: 申请无效的资源ID
    #[tokio::test]
    async fn test_allocate_invalid_resource_ids() {
        let session_ids = vec!["session1".to_string()];
        let service = mock_setup_service(session_ids.clone()).await;
        
        // 包含无效资源ID (负数和超出范围的ID)
        let resource_ids = vec![1, 2, 3, 1000, 65535];
        let result = service.handle_allocate(&session_ids[0], resource_ids.clone()).await;
        
        // 应该只成功申请有效的资源
        assert!(result.is_ok());
        let (success_res, failed_resources) = result.unwrap();
        assert_eq!(success_res, vec![1, 2, 3]);
        assert_eq!(failed_resources.len(), 2);
    }

    // 异常场景测试: 使用无效的会话ID申请资源
    #[tokio::test]
    async fn test_allocate_with_invalid_session_id() {
        let user_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let resource_pool = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS));
        let service = ResourceAppService::new(user_sessions, resource_pool);
        
        // 使用无效会话ID申请资源
        let resource_ids = vec![1];
        let result = service.handle_allocate("invalid_session_id", resource_ids).await;
        
        // 应该失败
        assert!(result.is_err());
    }

    // 异常场景测试: 释放已经释放的资源
    #[tokio::test]
    async fn test_release_already_released_resource() {
        let session_ids = vec!["session1".to_string()];
        let service = mock_setup_service(session_ids.clone()).await;
        let resource_ids = vec![1];
        
        // 先申请资源
        service.handle_allocate(&session_ids[0], resource_ids.clone()).await.unwrap();
        
        // 释放资源
        service.handle_release(&session_ids[0], resource_ids.clone()).await.unwrap();
        
        // 再次释放相同资源
        let (success_res, failed_resources) = service.handle_release(&session_ids[0], resource_ids.clone()).await.unwrap();
        
        // 第二次释放应该失败
        assert!(success_res.is_empty());
        assert_eq!(failed_resources.len(), 1);
    }

    // 异常场景测试: 资源超时释放
    #[tokio::test]
    async fn test_timeout_resource_release() {
        let user_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let resource_pool = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS));
        let service = ResourceAppService::new(user_sessions.clone(), resource_pool.clone());
        let session_service = SessionAppService::new(user_sessions.clone());
        
        // 添加session
        let session_id = "session_timeout".to_string();
        session_service.handle_add_session(session_id.clone()).await.unwrap();
        let resource_ids = vec![1];
        
        // 申请资源
        service.handle_allocate(&session_id, resource_ids.clone()).await.unwrap();
        assert!(service.user_sessions.user_has_resources(&session_id).await.unwrap());
        
        // 手动设置资源为超时状态
        resource_pool.set_resource_allocation_time(1, Some(SystemTime::now() - Duration::from_secs(config::RESOURCE_TIMEOUT_SECS as u64 + 1))).await;

        // 调用超时检测和释放
        let timeout_resources = service.get_and_release_timeout_resources().await;
        
        // 验证资源已被释放
        assert!(timeout_resources.contains_key(&session_id));
        assert_eq!(timeout_resources.get(&session_id).unwrap(), &resource_ids);
        assert!(!service.user_sessions.user_has_resources(&session_id).await.unwrap());
        assert!(resource_pool.get_holder(1).await.unwrap().is_none());
    }

    // 并发场景测试: 资源池耗尽场景
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_resource_pool_exhaustion() {
        let session_ids: Vec<String> = (0..config::MAX_SESSION_NUM * 2)
            .map(|i| format!("session{}", i))
            .collect();
        let service = Arc::new(mock_setup_service(session_ids.clone()).await);
        let mut handles = vec![];
        
        // 每个会话申请1个资源，直到资源池耗尽
        for i in 0..session_ids.len() {
            let service_clone = service.clone();
            let session_id = session_ids[i].clone();
            let resource_id = i as u16 % config::MAX_RESOURCE_NUM as u16;
            handles.push(tokio::spawn(async move {
                service_clone.handle_allocate(&session_id, vec![resource_id]).await
            }));
        }
        
        // 等待所有任务完成
        let results = futures::future::join_all(handles).await;
        
        // 统计成功和失败的任务数
        let mut success_count = 0;
        let mut failed_count = 0;
        for result in results {
            match result.unwrap() {
                Ok((success_res, _)) => {
                    if !success_res.is_empty() {
                        success_count += 1;
                    } else {
                        failed_count += 1;
                    }
                },
                Err(_) => failed_count += 1,
            }
        }
        
        // 成功数量应该等于资源池大小，其他应该失败
        assert_eq!(success_count, config::MAX_RESOURCE_NUM as usize);
        assert_eq!(failed_count, session_ids.len() - config::MAX_RESOURCE_NUM as usize);
    }

    // 并发场景测试: 资源快速释放后立即被其他会话申请
    #[tokio::test(flavor = "multi_thread")]
    async fn test_resource_recycling() {
        let session_ids: Vec<String> = (0..20)
            .map(|i| format!("session{}", i))
            .collect();
        let service = Arc::new(mock_setup_service(session_ids.clone()).await);
        
        // 先让5个会话占用资源
        let mut initial_handles = vec![];
        for i in 0..5 {
            let service_clone = service.clone();
            let session_id = session_ids[i].clone();
            let resource_id = i as u16;
            initial_handles.push(tokio::spawn(async move {
                service_clone.handle_allocate(&session_id, vec![resource_id]).await
            }));
        }
        futures::future::join_all(initial_handles).await;
        
        // 启动10个任务释放资源
        let mut release_handles = vec![];
        for i in 0..5 {
            let service_clone = service.clone();
            let session_id = session_ids[i].clone();
            let resource_id = i as u16;
            release_handles.push(tokio::spawn(async move {
                service_clone.handle_release(&session_id, vec![resource_id]).await
            }));
        }
        
        // 同时启动10个任务申请资源
        let mut allocate_handles = vec![];
        for i in 5..15 {
            let service_clone = service.clone();
            let session_id = session_ids[i].clone();
            let resource_id = i as u16 % 5;
            allocate_handles.push(tokio::spawn(async move {
                service_clone.handle_allocate(&session_id, vec![resource_id]).await
            }));
        }
        
        // 等待所有任务完成
        futures::future::join_all(release_handles).await;
        let allocate_results = futures::future::join_all(allocate_handles).await;
        
        // 统计成功申请的任务数
        let success_count = allocate_results.iter()
            .filter(|r| r.as_ref().unwrap().is_ok() && !r.as_ref().unwrap().as_ref().unwrap().0.is_empty())
            .count();
        
        // 应该有5个资源被释放并重新分配
        assert_eq!(success_count, 5);
    }

    // 并发场景测试: 并发调用超时检测功能
    #[tokio::test(flavor = "multi_thread")]
    async fn test_concurrent_timeout_detection() {
        let user_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let resource_pool = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS));
        let service = Arc::new(ResourceAppService::new(user_sessions.clone(), resource_pool.clone()));
        let session_service = SessionAppService::new(user_sessions.clone());
        
        // 添加多个会话并申请资源
        let session_count = 5;
        for i in 0..session_count {
            let session_id = format!("session_timeout_{}", i);
            session_service.handle_add_session(session_id.clone()).await.unwrap();
            service.handle_allocate(&session_id, vec![i as u16]).await.unwrap();
            
            // 设置资源为超时状态
            // 注意：这里假设ResourcePool有一个方法来获取资源的可写锁
            resource_pool.set_resource_allocation_time(i as u16, Some(SystemTime::now() - Duration::from_secs(config::RESOURCE_TIMEOUT_SECS as u64 + 1))).await;

        }
        
        // 启动多个并发任务调用超时检测
        let mut handles = vec![];
        for _ in 0..5 {
            let service_clone = service.clone();
            handles.push(tokio::spawn(async move {
                service_clone.get_and_release_timeout_resources().await
            }));
        }
        
        // 等待所有任务完成
        let results = futures::future::join_all(handles).await;
        
        // 验证所有超时资源被释放
        let mut total_released = 0;
        for result in results {
            let released = result.unwrap();
            for (_, resources) in released {
                total_released += resources.len();
            }
        }
        
        // 虽然有5个任务并发调用，但实际释放的资源总数应该是5
        assert_eq!(total_released, session_count);
        
        // 验证所有资源确实被释放
        for i in 0..session_count {
            assert!(resource_pool.get_holder(i as u16).await.unwrap().is_none());
        }
    }
}