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
    #[must_use]
    pub const fn new(resource_app: Arc<ResourceAppService>) -> Self {
        Self {
            resource_app,
        }
    }
}

impl MessageHandler for ReleaseMessageHandler {
    async fn handle(&self, base_session_id: String, data: &[u8], message_id: Option<String>) -> Result<String, ApiError> {
        // 解析请求数据
        let ws_message = serde_json::from_slice::<WsMessage<RequestMessage>>(data).unwrap();
        let (request_session_id, resources) = match &ws_message.data {
            RequestMessage::Release { session_id, resources } => (session_id, resources.clone()),
            _ => {
                return Ok(self.new_default_resp(base_session_id, 1));
            }
        };

        // 校验session_id
        let err_code = self.intercept(&base_session_id, request_session_id);
        if err_code != 0 {
            return Ok(self.new_default_resp(request_session_id.clone(), err_code));
        }
        tracing::info!("session {:?} release resource {:?}", base_session_id, resources);
        if let Ok((success_resources, failed_resources)) = self.resource_app.handle_release(&base_session_id, resources).await {
            tracing::info!("session {:?} release resource, success_resources: {:?}, failed_resources: {:?}", base_session_id, success_resources, failed_resources);
            // 构建响应
            let response = ResponseMessage::ReleaseResp {
                session_id: base_session_id.clone(),
                success_resources,
                failed_resources,
                error_code: 0,
            };
            let response_ws_message = WsMessage::new(message_id, response);

            return Ok(serde_json::to_string(&response_ws_message).unwrap());
        }

        Ok(self.new_default_resp(base_session_id, 2))
    }

    fn new_default_resp(&self, session_id: String, error_code: u32) -> String {
        let response = ResponseMessage::ReleaseResp {
            session_id,
            success_resources: Vec::new(),
            failed_resources: Vec::new(),
            error_code,
        };
        let response_ws_message = WsMessage::new(None, response);

        serde_json::to_string(&response_ws_message).unwrap()
    }
}