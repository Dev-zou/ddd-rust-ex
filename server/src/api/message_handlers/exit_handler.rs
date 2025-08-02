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
    #[must_use]
    pub const fn new(
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
    async fn handle(&self, base_session_id: String, data: &[u8], message_id: Option<String>) -> Result<String, ApiError> {
        // 解析请求数据
        let ws_message = serde_json::from_slice::<WsMessage<RequestMessage>>(data).unwrap();
        let RequestMessage::Exit { session_id: request_session_id } = &ws_message.data else {
            return Ok(self.new_default_resp(base_session_id, 1));
        };

        // 校验session_id
        let err_code = self.intercept(&base_session_id, request_session_id);
        if err_code != 0 {
            return Ok(self.new_default_resp(request_session_id.clone(), err_code));
        }

        tracing::info!("session {:?} exit received", base_session_id);
        // 移除会话
        if let Err(e) = self.session_app.handle_remove_session(&base_session_id, self.resource_app.clone()).await {
            tracing::warn!("session {:?} exit, fail to release resources {:?}", base_session_id, e);
        }

        Ok(self.new_default_resp(base_session_id, 0))
    }

    fn new_default_resp(&self, session_id: String, error_code: u32) -> String {
        let response = ResponseMessage::ExitResp {
            session_id,
        };
        let response_ws_message = WsMessage::new(None, response);

        serde_json::to_string(&response_ws_message).unwrap()
    }
}