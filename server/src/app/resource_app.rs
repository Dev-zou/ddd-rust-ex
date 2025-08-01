use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

use crate::app::error::AppError;
use crate::domain::user_sessions::{UserSessions};
use crate::domain::resource_pool::ResourcePool;

use std::sync::Arc;

pub struct ResourceAppService {
    user_sessions: Arc<UserSessions>,
    resource_pool: Arc<ResourcePool>,
    notification_tx: mpsc::Sender<(String, u16)>, // (session_id, resource_id)

}

impl ResourceAppService {
    pub fn new(
        user_sessions: Arc<UserSessions>,
        resource_pool: Arc<ResourcePool>,
    ) -> (Self, mpsc::Receiver<(String, u16)>) {
        let (tx, rx) = mpsc::channel(100);
        (
            Self { 
                user_sessions, 
                resource_pool,
                notification_tx: tx,
            },
            rx
        )
    }

    /// 处理资源申请请求
    pub async fn handle_allocate(
        &self,
        session_id: &str,
        resource_ids: Vec<u16>,
    ) -> Result<(Vec<u16>, Vec<(u16, String)>), AppError> {
        // 检查会话是否已持有资源
        if self.user_sessions.user_has_resources(session_id).await? {
            let mut failed_resources = Vec::new();
            for res_id in resource_ids {
                failed_resources.push((res_id, "Session already holding resources".to_string()));
            }
            return Ok((Vec::new(), failed_resources));
        }

        // 申请资源并更新会话状态
        let mut success_res = Vec::new();
        let mut failed_resources = Vec::new();

        for res_id in resource_ids {
            match self.resource_pool.allocate_resource(res_id, session_id.to_string()).await {
                Ok(_) => success_res.push(res_id),
                Err(e) => failed_resources.push((res_id, e.to_string())),
            }
        }

        if !success_res.is_empty() {
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
        resource_id: Vec<u16>,
    ) -> Result<(), AppError> {
        self.resource_pool.release_resource(resource_id[0]).await?;
        self.user_sessions.user_release_resources(session_id, resource_id).await?;
        Ok(())
    }

    /// 启动超时检测任务
    pub fn spawn_timeout_task(&self) {
        let pool = self.resource_pool.clone();
        let sessions = self.user_sessions.clone();
        let tx = self.notification_tx.clone();
        
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                
                // 检测超时资源（10秒超时）
                let timeouts = pool.get_timeout_resources(10).await;
                for (resource_id, session_id) in timeouts {
                    // 双重验证防止状态变更
                    if pool.is_held_by_session(resource_id, &session_id).await {
                        // 释放资源
                        if let Err(e) = pool.release_resource(resource_id).await {
                            tracing::info!("超时释放失败: {:?}", e);
                            continue;
                        }
                        
                        // 清理会话
                        if let Err(e) = sessions.user_release_resources(&session_id, vec![resource_id]).await {
                            tracing::info!("会话清理失败: {:?}", e);
                        }
                        
                        // 发送通知
                        if let Err(e) = tx.send((session_id, resource_id)).await {
                            tracing::info!("通知发送失败: {:?}", e);
                        }
                    }
                }
            }
        });
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
        let resource_pool = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider)));
        let session_service = SessionAppService::new(user_sessions.clone());

        // 添加session
        for session_id in session_ids {
            session_service.handle_add_session(session_id.clone()).await.unwrap();
        }
        let (app, _rx) = ResourceAppService::new(user_sessions.clone(), resource_pool.clone());
        app
    }

    #[tokio::test]
    async fn test_allocate_and_release() {
        // 初始化依赖
        let user_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let resource_pool = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider)));
        let (resource_service, _rx) = ResourceAppService::new(user_sessions.clone(), resource_pool.clone());
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
        let result = service.handle_allocate(&session_ids[0], resource_ids.clone()).await.unwrap();
        assert!(result.0.is_empty());
        assert_eq!(result.1.len(), 1);
        assert_eq!(result.1[0].0, 1);
        assert!(result.1[0].1.contains("already holding resources"));

        // 释放未持有的资源应失败
        let invalid_resource_id = vec![2];
        let result = service.handle_release(&session_ids[0], invalid_resource_id).await;
        assert!(result.is_err());
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
        let resource_pool = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider)));
        let (resource_service, _rx) = ResourceAppService::new(user_sessions.clone(), resource_pool.clone());
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
}