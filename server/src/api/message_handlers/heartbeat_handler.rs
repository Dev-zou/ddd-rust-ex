use std::sync::Arc;

use serde::Deserialize;

use super::super::{error::ApiError, message::{RequestMessage, ResponseMessage, WsMessage}};
use super::MessageHandler;
use crate::app::session_app::SessionAppService;

/// 心跳消息处理器
pub struct HeartbeatMessageHandler {
    session_app: Arc<SessionAppService>,
}

impl HeartbeatMessageHandler {
    /// 创建新的心跳消息处理器
    pub fn new(session_app: Arc<SessionAppService>) -> Self {
        Self {
            session_app,
        }
    }
}

impl MessageHandler for HeartbeatMessageHandler {
    async fn handle(&self, session_id: String, data: &[u8], message_id: Option<String>) -> Result<String, ApiError> {
        // 解析请求数据
        let ws_message = serde_json::from_slice::<WsMessage<RequestMessage>>(data).unwrap();
        let request_session_id = match &ws_message.data {
            RequestMessage::Heartbeat { session_id } => session_id,
            _ => return Err(ApiError::InvalidRequest),
        };

        // 校验session_id
        self.intercept(&session_id, request_session_id)?;

        tracing::info!("session {:?} heartbeat received", session_id);
        self.session_app.handle_heartbeat(&session_id).await?;

        // 构建响应
        let response = ResponseMessage::HeartbeatResp {
            session_id: session_id.clone(),
        };
        let response_ws_message = WsMessage::new(message_id, response);

        Ok(serde_json::to_string(&response_ws_message).unwrap())
    }
}