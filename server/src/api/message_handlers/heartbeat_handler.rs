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
    async fn handle(&self, base_session_id: String, data: &[u8], message_id: Option<String>) -> Result<String, ApiError> {
        // 解析请求数据
        let ws_message = serde_json::from_slice::<WsMessage<RequestMessage>>(data).unwrap();
        let request_session_id = match &ws_message.data {
            RequestMessage::Heartbeat { session_id } => session_id,
            _ => {
                return Ok(self.new_default_resp(base_session_id, 1));
            }
        };

        // 校验session_id
        let err_code = self.intercept(&base_session_id, request_session_id);
        if err_code != 0 {
            return Ok(self.new_default_resp(request_session_id.clone(), err_code));
        }
        tracing::info!("session {:?} heartbeat received", base_session_id);
        self.session_app.handle_heartbeat(&base_session_id).await?;

        Ok(self.new_default_resp(base_session_id, 0))
    }

    fn new_default_resp(&self, session_id: String, error_code: u32) -> String {
        let response = ResponseMessage::HeartbeatResp {
            session_id: session_id.clone(),
            error_code,
        };
        let response_ws_message = WsMessage::new(None, response);

        serde_json::to_string(&response_ws_message).unwrap()
    }
}