//! 消息处理器模块
//! 包含各种WebSocket消息类型的处理逻辑

mod allocate_handler;
mod release_handler;
mod query_handler;
mod heartbeat_handler;
mod exit_handler;

pub use allocate_handler::AllocateMessageHandler;
pub use release_handler::ReleaseMessageHandler;
pub use query_handler::QueryMessageHandler;
pub use heartbeat_handler::HeartbeatMessageHandler;
pub use exit_handler::ExitMessageHandler;

/// 消息处理器 trait
pub trait MessageHandler {
    /// 处理消息并返回响应
    fn handle(&self, session_id: String, data: &[u8], message_id: Option<String>) -> impl std::future::Future<Output = Result<String, super::error::ApiError>> + Send;

    /// 拦截异常请求
    fn intercept(&self, server_saved_session_id: &str, request_session_id: &str) -> Result<(), super::error::ApiError> {
        // 校验请求中的session_id与服务器保存的session_id是否一致
        if request_session_id != server_saved_session_id {
            return Err(super::error::ApiError::InvalidSessionId);
        }

        // 可以添加其他校验条件
        // 例如: 校验session是否过期、校验权限等

        Ok(())
    }
}

/// 消息处理器 trait
pub trait AsyncMessageHandler {
    /// 处理消息并返回响应
    fn handle(&self, session_id: String, data: &[u8], message_id: Option<String>) -> impl std::future::Future<Output = Result<String, super::error::ApiError>> + Send;

    /// 拦截异常请求
    fn intercept(&self, server_saved_session_id: &str, request_session_id: &str) -> Result<(), super::error::ApiError> {
        // 校验请求中的session_id与服务器保存的session_id是否一致
        if request_session_id != server_saved_session_id {
            return Err(super::error::ApiError::InvalidSessionId);
        }

        // 可以添加其他校验条件
        // 例如: 校验session是否过期、校验权限等

        Ok(())
    }
}
