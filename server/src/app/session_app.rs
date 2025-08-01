use crate::app::error::AppError;
use crate::app::resource_app::ResourceAppService;
use crate::domain::user_sessions::UserSessions;
use std::sync::Arc;
use tracing::warn;

pub struct SessionAppService {
    pub user_sessions: Arc<UserSessions>,
}

impl SessionAppService {
    pub const fn new(user_sessions: Arc<UserSessions>) -> Self {
        Self { user_sessions }
    }

    /// 处理添加/更新会话请求
    pub async fn handle_add_session(&self, session_id: String) -> Result<(), AppError> {
        self.user_sessions.add_session(session_id).await?;
        Ok(())
    }

    /// 处理心跳请求
    pub async fn handle_heartbeat(&self, session_id: &str) -> Result<(), AppError> {
        self.user_sessions.update_session_heartbeat(session_id).await?;
        Ok(())
    }

    /// 处理删除会话请求（先释放资源，再删除会话）
    pub async fn handle_remove_session(
        &self,
        session_id: &str,
        resource_app: Arc<ResourceAppService>, // 注入ResourceAppService以释放资源
    ) -> Result<(), AppError> {
        // 1. 检查会话是否存在
        self.user_sessions.session_exists(session_id).await?;

        // 2. 检查并释放会话持有的资源（忽略错误）
        if let Ok(resources) = self.user_sessions.user_get_resources(session_id).await {
            if !resources.is_empty() {
                if let Err(e) = resource_app.handle_release(session_id, resources.clone()).await {
                    tracing::warn!("Failed to release resources for session {}: {:?}", session_id, e);
                }
            }
        } else {
            tracing::warn!("Failed to get resources for session {}", session_id);
        }

        // 3. 删除会话（确保执行）
        self.user_sessions.remove_session(session_id).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::resource_pool::ResourcePool;
    use crate::app::config;
    use crate::infra::resource_mock::MockResourceProvider;

    #[tokio::test]
    async fn test_add_session_success() {
        let mock_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let session_app = SessionAppService::new(mock_sessions.clone());

        // 调用添加会话
        assert!(session_app.handle_add_session("session_123".to_string()).await.is_ok());

        // 验证会话已添加
        assert!(mock_sessions.session_exists("session_123").await.is_ok());
    }

    #[tokio::test]
    async fn test_remove_session_with_resources() {
        let mock_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let mock_resource = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider)));
        let resource_app = ResourceAppService::new(mock_sessions.clone(), mock_resource.clone());
        let resource_app = Arc::new(resource_app);
        let session_app = SessionAppService::new(mock_sessions.clone());

        // 设置测试数据：添加会话和资源
        // 添加session
        let session_id = "session1".to_string();
        let resource_ids = vec![1];
        session_app.handle_add_session(session_id.clone()).await.unwrap();
        // 测试资源申请
        resource_app.handle_allocate(&session_id, resource_ids).await.unwrap();

        // 调用删除会话
        let result = session_app.handle_remove_session(&session_id, resource_app.clone()).await;
        assert!(result.is_ok());

        // 验证会话和资源已删除
        assert!(mock_sessions.session_exists(&session_id).await.is_err());
    }

    #[tokio::test]
    async fn test_remove_session_not_exists() {
        let mock_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let mock_resource = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider)));
        let resource_app = ResourceAppService::new(mock_sessions.clone(), mock_resource.clone());
        let resource_app = Arc::new(resource_app);
        let session_app = SessionAppService::new(mock_sessions.clone());

        // 调用删除不存在的会话
        let result = session_app.handle_remove_session("invalid_session", resource_app).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_concurrent_add_sessions() {
        let mock_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let session_app = Arc::new(SessionAppService::new(mock_sessions.clone()));

        let mut handles = vec![];
        for i in 0..10 {
            let service_clone = session_app.clone();
            handles.push(tokio::spawn(async move {
                service_clone.handle_add_session(format!("session_{}", i)).await
            }));
        }

        // 等待所有任务完成
        for handle in handles {
            assert!(handle.await.unwrap().is_ok());
        }

        // 验证所有会话已添加
        for i in 0..10 {
            assert!(mock_sessions.session_exists(&format!("session_{}", i)).await.is_ok());
        }
    }

    #[tokio::test]
    async fn test_concurrent_remove_sessions() {
        let mock_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let mock_resource = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider)));
        let resource_app = ResourceAppService::new(mock_sessions.clone(), mock_resource.clone());
        let resource_app = Arc::new(resource_app);
        let session_app = Arc::new(SessionAppService::new(mock_sessions.clone()));

        // 模拟10个会话并发申请不同资源
        let session_ids: Vec<String> = (0..10)
            .map(|i| format!("session{}", i))
            .collect();

        for i in 0..10 {
            let session_app_clone = session_app.clone();
            let resource_app_clone = resource_app.clone();
            let resource_ids = vec![i.try_into().unwrap()];
            session_app_clone.handle_add_session(session_ids[i].clone()).await.unwrap();
            resource_app_clone.handle_allocate(&session_ids[i], resource_ids).await.unwrap();
        }

        // 删除10个session
        let mut handles = vec![];
        for i in 0..10 {
            let session_app_clone = session_app.clone();
            let resource_app_clone = resource_app.clone();
            let session_id = session_ids[i].clone();
            handles.push(tokio::spawn(async move {
                session_app_clone.handle_remove_session(&session_id, resource_app_clone).await
            }));
        }

        // 验证所有删除操作成功
        for handle in handles {
            assert!(handle.await.unwrap().is_ok());
        }

        // 验证所有会话已删除
        for i in 0..10 {
            assert!(mock_sessions.session_exists(&session_ids[i]).await.is_err());
        }
    }

    #[tokio::test]
    async fn test_race_condition_on_same_session() {
        let mock_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let mock_resource = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider)));
        let resource_app = ResourceAppService::new(mock_sessions.clone(), mock_resource.clone());
        let resource_app = Arc::new(resource_app);
        let session_app = Arc::new(SessionAppService::new(mock_sessions.clone()));

        // 设置测试数据：添加会话和资源
        // 添加session
        let session_id = "session_race".to_string();
        let resource_ids = vec![1];
        session_app.handle_add_session(session_id.clone()).await.unwrap();
        // 测试资源申请
        resource_app.handle_allocate(&session_id, resource_ids).await.unwrap();

        // 启动两个并发删除任务
        let handle1 = tokio::spawn({
            let session_app_clone = session_app.clone();
            let resource_app_clone = resource_app.clone();
            let session_id_clone = session_id.clone();
            async move {
                session_app_clone.handle_remove_session(&session_id_clone, resource_app_clone).await
            }
        });

        let handle2 = tokio::spawn({
            let session_app_clone = session_app.clone();
            let resource_app_clone = resource_app.clone();
            let session_id_clone = session_id.clone();
            async move {
                session_app_clone.handle_remove_session(&session_id_clone, resource_app_clone).await
            }
        });

        // 至少一个成功，另一个可能因会话已删除而失败
        let results = futures::future::join_all(vec![handle1, handle2]).await;
        assert!(results.iter().any(|r| r.as_ref().unwrap().is_ok()));
        assert!(mock_sessions.session_exists(&session_id).await.is_err());
    }
}