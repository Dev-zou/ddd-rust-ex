use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

use crate::domain::error::UserSessionsError;


#[derive(Debug)]
#[allow(dead_code)]
pub enum UserStatus {
    Free,
    Applying,
    Helding,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct UserSession {
    session_id: String,
    last_time: SystemTime,
    status: UserStatus,
    resources: Vec<u16>,

}

#[derive(Debug)]
pub struct UserSessions {
    sessions: Arc<RwLock<HashMap<String, UserSession>>>,
    max_session_num: usize,
}

impl UserSessions {
    #[must_use]
    pub fn new(max_session_num: usize) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::with_capacity(10))),
            max_session_num
        }
    }

    pub async fn add_session(&self, session_id: String) -> Result<(), UserSessionsError> {
        let mut sessions = self.sessions.write().await;
        if self.max_session_num <= sessions.len() {
            return Err(UserSessionsError::QuotaExceeded(self.max_session_num.try_into().unwrap()));
        }
        sessions.insert(
            session_id.clone(),
            UserSession {
                session_id,
                last_time: SystemTime::now(),
                status: UserStatus::Free,
                resources: Vec::new(),
            },
        );
        drop(sessions);
        Ok(())
    }

    pub async fn remove_session(&self, session_id: &str) -> Result<(), UserSessionsError> {
        let mut sessions = self.sessions.write().await;
        if sessions.remove(session_id).is_none() {
            return Err(UserSessionsError::InvalidSession(session_id.to_owned()))
        }
        drop(sessions);
        Ok(())
    }

    pub async fn user_add_resources(
        &self,
        session_id: &str,
        resources: Vec<u16>,
    ) -> Result<(), UserSessionsError> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.resources = resources;
            session.last_time = SystemTime::now();
            session.status = UserStatus::Helding;
            Ok(())
        } else {
            Err(UserSessionsError::InvalidSession(session_id.to_owned()))
        }
    }

    pub async fn user_release_resources(
        &self,
        session_id: &str,
        user_release_resources: Vec<u16>,
    ) -> Result<(), UserSessionsError> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.resources.retain(|r| !user_release_resources.contains(r));
            session.last_time = SystemTime::now();
            if session.resources.is_empty() {
                session.status = UserStatus::Free;
            }
            Ok(())
        } else {
            Err(UserSessionsError::InvalidSession(session_id.to_owned()))
        }
    }

    /// 校验用户是否可以申请资源
    /// 只有Free状态的用户才能申请资源，并将状态置为Applying
    pub async fn validate_and_start_allocate(&self, session_id: &str) -> Result<(), UserSessionsError> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            match session.status {
                UserStatus::Free => {
                    session.status = UserStatus::Applying;
                    session.last_time = SystemTime::now();
                    Ok(())
                },
                _ => {
                    Err(UserSessionsError::InvalidSessionStatus(
                        session_id.to_owned(),
                        format!("{:?}", session.status),
                    ))
                },
            }
        } else {
            Err(UserSessionsError::InvalidSession(session_id.to_owned()))
        }
    }

    /// 完成资源申请，没有申请到资源，将状态从Applying置为Free
    pub async fn rollback_applying_session(&self, session_id: &str) -> Result<(), UserSessionsError> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.last_time = SystemTime::now();
            session.status = UserStatus::Free;
            Ok(())
        } else {
            Err(UserSessionsError::InvalidSession(session_id.to_owned()))
        }
    }

    /// 判断指定session是否持有资源
    pub async fn user_has_resources(&self, session_id: &str) -> Result<bool, UserSessionsError> {
        let sessions = self.sessions.read().await; // 获取读锁（共享访问）
        sessions.get(session_id).map_or_else(|| Err(UserSessionsError::InvalidSession(session_id.to_owned())), |session| Ok(matches!(session.status, UserStatus::Helding)))
    }

    /// 获取session持有的资源
    pub async fn user_get_resources(&self, session_id: &str) -> Result<Vec<u16>, UserSessionsError> {
        let sessions = self.sessions.read().await; // 获取读锁（共享访问）
        sessions.get(session_id).map_or_else(|| Err(UserSessionsError::InvalidSession(session_id.to_owned())), |session| Ok(session.resources.clone()))
    }

    /// 生成所有会话的状态报告（按会话ID排序）
    pub async fn status_report(&self) -> Vec<String> {
        let mut report: Vec<_> = self.sessions.read().await
            .iter()
            .map(|(id, session)| {
                format!(
                    "Session {}: Status={:?}, LastActive={:?}, Resources={:?}",
                    id,
                    session.status,
                    session.last_time,
                    session.resources
                )
            })
            .collect();
        report.sort(); // 按会话ID排序
        report
    }

    /// 判断指定session是否持有资源
    pub async fn status_report_resource_num(&self, session_id: &str) -> Option<usize> {
        let sessions = self.sessions.read().await; // 获取读锁（共享访问）
        sessions.get(session_id).map(|session| session.resources.len())
    }
    /// 判断指定会话是否存在（线程安全）
    /// 返回：
    ///   - `Ok(())`: 会话存在
    ///   - `Err(UserSessionsError::InvalidSession)`: 会话不存在
    pub async fn session_exists(&self, session_id: &str) -> Result<(), UserSessionsError> {
        let sessions = self.sessions.read().await; // 获取读锁（共享访问）
        if sessions.contains_key(session_id) {
            Ok(())
        } else {
            Err(UserSessionsError::InvalidSession(session_id.to_owned()))
        }
    }

    /// 更新会话心跳时间
    pub async fn update_session_heartbeat(&self, session_id: &str) -> Result<(), UserSessionsError> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.last_time = SystemTime::now();
            Ok(())
        } else {
            Err(UserSessionsError::InvalidSession(session_id.to_owned()))
        }
    }
    
    pub async fn get_session_num(&self) -> usize {
        self.sessions.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::config;

    #[tokio::test]
    async fn test_new_user_sessions() {
        let sessions = UserSessions::new(config::MAX_SESSION_NUM);
        let report = sessions.status_report().await;
        assert!(report.is_empty()); // 初始应为空
    }

    #[tokio::test]
    async fn test_add_and_remove_session() {
        let sessions = UserSessions::new(config::MAX_SESSION_NUM);
        // 添加会话
        sessions.add_session("session_1".to_string()).await.unwrap();
        assert!(sessions.user_has_resources("session_1").await.unwrap() == false); // 初始状态为Free
        // 注销会话
        let removed = sessions.remove_session("session_1").await;
        assert!(removed.is_ok());
        assert!(sessions.remove_session("invalid_id").await.is_err()); // 无效ID测试
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_concurrent_add_sessions() {
        let sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let mut handles = vec![];
        for i in 0..100 {
            let sessions = sessions.clone();
            handles.push(tokio::spawn(async move {
                sessions.add_session(format!("session_{}", i)).await.unwrap();
            }));
        }
        // futures::future::join_all(handles).await;
        for handle in handles {
            handle.await.unwrap();
        }

        assert_eq!(sessions.status_report().await.len(), 100); // 确保全部添加成功
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_concurrent_release_resources() {
        let sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        sessions.add_session("session_1".to_string()).await.unwrap();
        sessions.user_add_resources("session_1", (0..100).collect()).await.unwrap();
        let mut handles = vec![];
        for i in 0..100 {
            let sessions = sessions.clone();
            handles.push(tokio::spawn(async move {
                sessions.user_release_resources("session_1", vec![i]).await.unwrap();
            }));
        }
        // futures::future::join_all(handles).await;
        for handle in handles {
            handle.await.unwrap();
        }
        assert!(!sessions.user_has_resources("session_1").await.unwrap()); // 所有资源释放后状态回Free
    }

    #[tokio::test]
    async fn test_invalid_session_operations() {
        let sessions = UserSessions::new(config::MAX_SESSION_NUM);
        // 对不存在的Session操作资源
        assert!(sessions.user_add_resources("invalid_id", vec![1]).await.is_err());
        assert!(sessions.user_release_resources("invalid_id", vec![1]).await.is_err());
    }

    #[tokio::test]
    async fn test_resource_state_conflict() {
        let sessions = UserSessions::new(config::MAX_SESSION_NUM);
        sessions.add_session("session_1".to_string()).await.unwrap();
        // 重复添加相同资源
        sessions.user_add_resources("session_1", vec![1]).await.unwrap();
        sessions.user_add_resources("session_1", vec![1]).await.unwrap(); // 应去重或忽略
        assert_eq!(sessions.status_report_resource_num("session_1").await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_update_session_heartbeat() {
        let sessions = UserSessions::new(config::MAX_SESSION_NUM);
        let session_id = "session_1".to_string();
        
        // 测试1: 更新存在的会话的心跳时间
        sessions.add_session(session_id.clone()).await.unwrap();
        
        // 记录更新前的时间
        let before_update = SystemTime::now();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        // 更新心跳
        let result = sessions.update_session_heartbeat(&session_id).await;
        assert!(result.is_ok());
        
        // 获取更新后的会话信息
        let sessions_read = sessions.sessions.read().await;
        let session = sessions_read.get(&session_id).unwrap();
        assert!(session.last_time > before_update);
        drop(sessions_read);
        
        // 测试2: 尝试更新不存在的会话的心跳时间
        let invalid_session_id = "invalid_session".to_string();
        let result = sessions.update_session_heartbeat(&invalid_session_id).await;
        assert!(result.is_err());
        assert!(matches!(result, Err(UserSessionsError::InvalidSession(_))));
    }
}