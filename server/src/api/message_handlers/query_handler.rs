use std::sync::Arc;

use serde::Deserialize;

use super::super::{error::ApiError, message::{RequestMessage, ResponseMessage, WsMessage}};
use super::MessageHandler;
use crate::app::resource_app::ResourceAppService;

/// 资源查询消息处理器
pub struct QueryMessageHandler {
    resource_app: Arc<ResourceAppService>,
}

impl QueryMessageHandler {
    /// 创建新的资源查询消息处理器
    pub fn new(resource_app: Arc<ResourceAppService>) -> Self {
        Self {
            resource_app,
        }
    }
}

impl MessageHandler for QueryMessageHandler {
    async fn handle(&self, session_id: String, data: &[u8], message_id: Option<String>) -> Result<String, ApiError> {
        // 解析请求数据
        let ws_message = serde_json::from_slice::<WsMessage<RequestMessage>>(data).unwrap();
        let request_session_id = match &ws_message.data {
            RequestMessage::Query { session_id } => session_id,
            _ => return Err(ApiError::InvalidRequest),
        };

        // 校验session_id
        self.intercept(&session_id, request_session_id)?;

        tracing::info!("session {:?} query resource", session_id);
        // let resource_info = self.resource_app.handle_query(&session_id, resource_id).await?;
        let resource_info = vec![0];

        // 构建响应
        let response = ResponseMessage::QueryResp {
            session_id: session_id.clone(),
            resources: resource_info,
        };
        let response_ws_message = WsMessage::new(message_id, response);

        Ok(serde_json::to_string(&response_ws_message).unwrap())
    }
}