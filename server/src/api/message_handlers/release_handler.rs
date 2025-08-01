use std::sync::Arc;

use serde::Deserialize;

use super::super::{error::ApiError, message::{RequestMessage, ResponseMessage, WsMessage}};
use super::MessageHandler;
use crate::app::resource_app::ResourceAppService;

/// 资源释放消息处理器
pub struct ReleaseMessageHandler {
    resource_app: Arc<ResourceAppService>,
}

impl ReleaseMessageHandler {
    /// 创建新的资源释放消息处理器
    pub fn new(resource_app: Arc<ResourceAppService>) -> Self {
        Self {
            resource_app,
        }
    }
}

impl MessageHandler for ReleaseMessageHandler {
    async fn handle(&self, session_id: String, data: &[u8], message_id: Option<String>) -> Result<String, ApiError> {
        // 解析请求数据
        let ws_message = serde_json::from_slice::<WsMessage<RequestMessage>>(data).unwrap();
        let (request_session_id, resources) = match &ws_message.data {
            RequestMessage::Release { session_id, resources } => (session_id, resources.clone()),
            _ => return Err(ApiError::InvalidRequest),
        };

        // 校验session_id
        self.intercept(&session_id, request_session_id)?;

        tracing::info!("session {:?} release resource {:?}", session_id, resources);
        self.resource_app.handle_release(&session_id, resources).await?;

        // 构建响应
        let response = ResponseMessage::ReleaseResp {
            session_id: session_id.clone(),
        };
        let response_ws_message = WsMessage::new(message_id, response);

        Ok(serde_json::to_string(&response_ws_message).unwrap())
    }
}