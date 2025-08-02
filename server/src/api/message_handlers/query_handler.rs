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
    #[must_use]
    pub const fn new(resource_app: Arc<ResourceAppService>) -> Self {
        Self {
            resource_app,
        }
    }
}

impl MessageHandler for QueryMessageHandler {
    async fn handle(&self, base_session_id: String, data: &[u8], message_id: Option<String>) -> Result<String, ApiError> {
        // 解析请求数据
        let ws_message = serde_json::from_slice::<WsMessage<RequestMessage>>(data).unwrap();
        let RequestMessage::Query { session_id: request_session_id } = &ws_message.data else {
            return Ok(self.new_default_resp(base_session_id, 1));
        };

        // 校验session_id
        let err_code = self.intercept(&base_session_id, request_session_id);
        if err_code != 0 {
            return Ok(self.new_default_resp(request_session_id.clone(), err_code));
        }
        tracing::info!("session {:?} query resource", base_session_id);
        if let Ok(resource_info) = self.resource_app.handle_query(&base_session_id).await {
            // 构建响应
            let response = ResponseMessage::QueryResp {
                session_id: base_session_id.clone(),
                resources: resource_info,
                error_code: 0,
            };
            let response_ws_message = WsMessage::new(message_id, response);

            return Ok(serde_json::to_string(&response_ws_message).unwrap());
        }

        Ok(self.new_default_resp(base_session_id, 2))
    }

    fn new_default_resp(&self, session_id: String, error_code: u32) -> String {
        let response = ResponseMessage::QueryResp {
            session_id,
            resources: Vec::new(),
            error_code,
        };
        let response_ws_message = WsMessage::new(None, response);

        serde_json::to_string(&response_ws_message).unwrap()
    }
}