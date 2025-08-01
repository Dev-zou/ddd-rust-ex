use std::sync::Arc;

use serde::Deserialize;

use super::super::{error::ApiError, message::{RequestMessage, ResponseMessage, WsMessage}};
use super::MessageHandler;
use crate::app::resource_app::ResourceAppService;

/// 资源分配消息处理器
pub struct AllocateMessageHandler {
    resource_app: Arc<ResourceAppService>,
}

impl AllocateMessageHandler {
    /// 创建新的资源分配消息处理器
    pub fn new(resource_app: Arc<ResourceAppService>) -> Self {
        Self {
            resource_app,
        }
    }
}

impl MessageHandler for AllocateMessageHandler {
    async fn handle(&self, session_id: String, data: &[u8], message_id: Option<String>) -> Result<String, ApiError> {
        // 解析请求数据
        let ws_message = serde_json::from_slice::<WsMessage<RequestMessage>>(data).unwrap();
        let (request_session_id, resources) = match &ws_message.data {
            RequestMessage::Allocate { session_id, resources } => (session_id, resources.clone()),
            _ => return Err(ApiError::InvalidRequest),
        };

        // 校验session_id
        self.intercept(&session_id, request_session_id)?;

        tracing::info!("session {:?} allocate resource {:?}", session_id, resources);
        let (success_resources, failed_resources) = self.resource_app.handle_allocate(&session_id, resources).await?;

        // 构建响应
        let response = ResponseMessage::AllocateResp {
            session_id: session_id.clone(),
            success_resources,
            failed_resources,
        };
        let response_ws_message = WsMessage::new(message_id, response);

        Ok(serde_json::to_string(&response_ws_message).unwrap())
    }
}