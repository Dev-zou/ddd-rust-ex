use crate::app::error::AppError;
use crate::app::resource_app::ResourceAppService;
use crate::domain::user_sessions::UserSessions;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tracing::warn;

pub struct SessionAppService {
    pub user_sessions: Arc<UserSessions>,
}

impl SessionAppService {
    #[must_use]
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

    pub async fn get_session_num(&self) -> usize {
        self.user_sessions.get_session_num().await
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
        let mock_resource = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS));
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
        let mock_resource = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS));
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
        let mock_resource = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS));
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
        let mock_resource = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS));
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

    #[tokio::test]
    async fn test_handle_heartbeat() {
        let mock_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let session_app = SessionAppService::new(mock_sessions.clone());
        let session_id = "session_heartbeat".to_string();

        // 测试1: 更新存在的会话的心跳时间
        session_app.handle_add_session(session_id.clone()).await.unwrap();

        // 验证会话存在
        assert!(mock_sessions.session_exists(&session_id).await.is_ok());

        // 更新心跳
        let result = session_app.handle_heartbeat(&session_id).await;
        assert!(result.is_ok());

        // 测试2: 尝试更新不存在的会话的心跳时间
        let invalid_session_id = "invalid_session_heartbeat".to_string();
        let result = session_app.handle_heartbeat(&invalid_session_id).await;
        assert!(result.is_err());
    }

    // 异常场景测试: 会话数量超限
    #[tokio::test]
    async fn test_exceed_max_sessions() {
        let mock_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let session_app = SessionAppService::new(mock_sessions.clone());

        // 添加MAX_SESSION_NUM个会话
        for i in 0..config::MAX_SESSION_NUM {
            let session_id = format!("session_{}", i);
            session_app.handle_add_session(session_id).await.unwrap();
        }

        // 尝试添加第MAX_SESSION_NUM+1个会话
        let session_id = format!("session_{}", config::MAX_SESSION_NUM);
        let result = session_app.handle_add_session(session_id).await;

        // 应该失败
        assert!(result.is_err());
    }

    // 异常场景测试: 重复添加相同会话
    #[tokio::test]
    async fn test_duplicate_session_addition() {
        let mock_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let session_app = SessionAppService::new(mock_sessions.clone());

        let session_id = "duplicate_session".to_string();

        // 第一次添加成功
        session_app.handle_add_session(session_id.clone()).await.unwrap();

        // 第二次添加应该失败或成功更新
        let result = session_app.handle_add_session(session_id.clone()).await;

        // 根据实现，可能是成功(更新)或失败(已存在)
        // 这里假设实现允许重复添加(相当于更新)
        assert!(result.is_ok());
    }

    // 异常场景测试: 会话超时
    // #[tokio::test]
    // async fn test_session_timeout() {
    //     // 需要假设UserSessions有超时清理功能
    //     // 这里仅作示例，实际实现可能不同
    //     let mock_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
    //     let session_app = SessionAppService::new(mock_sessions.clone());

    //     let session_id = "timeout_session".to_string();
    //     session_app.handle_add_session(session_id.clone()).await.unwrap();

    //     // 验证会话存在
    //     assert!(mock_sessions.session_exists(&session_id).await.is_ok());

    //     // 手动设置会话为超时状态
    //     // 调用UserSessions的update_session_heartbeat方法，传入一个过去的时间
    //     // 注意：这里假设update_session_heartbeat方法允许设置任意时间
    //     // 实际实现可能需要修改该方法或添加新方法
    //     // mock_sessions.update_session_heartbeat_with_time(&session_id, SystemTime::now() - Duration::from_secs(config::SESSION_TIMEOUT_SECS as u64 + 1)).await.unwrap();

    //     // 等待超时清理
    //     tokio::time::sleep(Duration::from_secs(1)).await;

    //     // 验证会话已被清理
    //     assert!(mock_sessions.session_exists(&session_id).await.is_err());
    // }

    // 并发场景测试: 会话添加和删除并发
    #[tokio::test(flavor = "multi_thread")]
    async fn test_concurrent_add_remove() {
        let mock_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let mock_resource = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(MockResourceProvider), config::RESOURCE_TIMEOUT_SECS));
        let resource_app = ResourceAppService::new(mock_sessions.clone(), mock_resource.clone());
        let resource_app = Arc::new(resource_app);
        let session_app = Arc::new(SessionAppService::new(mock_sessions.clone()));

        // 添加一些初始会话
        let initial_sessions = 5;
        for i in 0..initial_sessions {
            let session_id = format!("session_{}", i);
            session_app.handle_add_session(session_id).await.unwrap();
        }

        // 启动10个并发任务，每个任务交替添加和删除会话
        let mut handles = vec![];
        for i in 0..10 {
            let session_app_clone = session_app.clone();
            let resource_app_clone = resource_app.clone();
            handles.push(tokio::spawn(async move {
                let session_id = format!("concurrent_session_{}", i);
                // 添加会话
                session_app_clone.handle_add_session(session_id.clone()).await.unwrap();
                // 立即删除会话
                session_app_clone.handle_remove_session(&session_id, resource_app_clone).await
            }));
        }

        // 等待所有任务完成
        let results = futures::future::join_all(handles).await;

        // 统计成功和失败的任务数
        let success_count = results.iter().filter(|r| r.as_ref().unwrap().is_ok()).count();
        let fail_count = results.len() - success_count;

        // 输出结果，帮助调试
        // println!("Concurrent add/remove: {} success, {} failed", success_count, fail_count);

        // 验证最终会话数量
        let final_count = mock_sessions.get_session_num().await;
        assert_eq!(final_count, initial_sessions);
    }

    // 并发场景测试: 心跳更新并发
    #[tokio::test(flavor = "multi_thread")]
    async fn test_concurrent_heartbeat_updates() {
        let mock_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let session_app = Arc::new(SessionAppService::new(mock_sessions.clone()));

        let session_id = "concurrent_heartbeat_session".to_string();
        session_app.handle_add_session(session_id.clone()).await.unwrap();

        // 启动20个并发任务更新同一个会话的心跳
        let mut handles = vec![];
        for _ in 0..20 {
            let session_app_clone = session_app.clone();
            let session_id_clone = session_id.clone();
            handles.push(tokio::spawn(async move {
                session_app_clone.handle_heartbeat(&session_id_clone).await
            }));
        }

        // 等待所有任务完成
        let results = futures::future::join_all(handles).await;

        // 所有心跳更新都应该成功
        for result in results {
            assert!(result.unwrap().is_ok());
        }

        // 验证会话仍然存在
        assert!(mock_sessions.session_exists(&session_id).await.is_ok());
    }

    // 并发场景测试: 会话数量接近上限时的并发添加
    #[tokio::test(flavor = "multi_thread")]
    async fn test_concurrent_add_near_limit() {
        let max_sessions = 10; // 使用较小的限制便于测试
        let mock_sessions = Arc::new(UserSessions::new(max_sessions));
        let session_app = Arc::new(SessionAppService::new(mock_sessions.clone()));

        // 先添加max_sessions - 2个会话
        for i in 0..max_sessions - 2 {
            let session_id = format!("session_{}", i);
            session_app.handle_add_session(session_id).await.unwrap();
        }

        // 启动10个并发任务尝试添加会话
        let mut handles = vec![];
        for i in 0..10 {
            let session_app_clone = session_app.clone();
            handles.push(tokio::spawn(async move {
                let session_id = format!("concurrent_near_limit_{}", i);
                session_app_clone.handle_add_session(session_id).await
            }));
        }

        // 等待所有任务完成
        let results = futures::future::join_all(handles).await;

        // 统计成功和失败的任务数
        let success_count = results.iter().filter(|r| r.as_ref().unwrap().is_ok()).count();
        let fail_count = results.len() - success_count;

        // 成功的任务数应该是2，失败的应该是8
        assert_eq!(success_count, 2);
        assert_eq!(fail_count, 8);

        // 验证最终会话数量
        let final_count = mock_sessions.get_session_num().await;
        assert_eq!(final_count, max_sessions);
    }
}