use std::sync::Arc;

use serde::Deserialize;

use super::super::{error::ApiError, message::{RequestMessage, ResponseMessage, WsMessage}};
use super::MessageHandler;
use crate::app::{resource_app::ResourceAppService, session_app::SessionAppService};

/// 退出消息处理器
pub struct ExitMessageHandler {
    session_app: Arc<SessionAppService>,
    resource_app: Arc<ResourceAppService>,
}

impl ExitMessageHandler {
    /// 创建新的退出消息处理器
    pub fn new(
        session_app: Arc<SessionAppService>,
        resource_app: Arc<ResourceAppService>,
    ) -> Self {
        Self {
            session_app,
            resource_app,
        }
    }
}

impl MessageHandler for ExitMessageHandler {
    async fn handle(&self, session_id: String, data: &[u8], message_id: Option<String>) -> Result<String, ApiError> {
        // 解析请求数据
        let ws_message = serde_json::from_slice::<WsMessage<RequestMessage>>(data).unwrap();
        let request_session_id = match &ws_message.data {
            RequestMessage::Exit { session_id } => session_id,
            _ => return Err(ApiError::InvalidRequest),
        };

        // 校验session_id
        self.intercept(&session_id, request_session_id)?;

        tracing::info!("session {:?} exit received", session_id);
        // 移除会话
        if let Err(e) = self.session_app.handle_remove_session(&session_id, self.resource_app.clone()).await {
            tracing::warn!("session {:?} exit, fail to release resources {:?}", session_id, e);
        }

        // 构建响应
        let response = ResponseMessage::ExitResp {
            session_id: session_id.clone(),
        };
        let response_ws_message = WsMessage::new(message_id, response);

        Ok(serde_json::to_string(&response_ws_message).unwrap())
    }
}