use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, Mutex};
use serde::Deserialize;

use super::super::{error::ApiError, message::{RequestMessage, ResponseMessage, WsMessage}};
use super::{MessageHandler, AsyncMessageHandler};
use crate::app::resource_app::ResourceAppService;

/// 资源分配消息处理器
pub struct AsyncAllocateMessageHandler {
    resource_app: Arc<ResourceAppService>,
}

impl AsyncAllocateMessageHandler {
    /// 创建新的资源分配消息处理器
    pub fn new(resource_app: Arc<ResourceAppService>) -> Self {
        Self {
            resource_app,
        }
    }
}

impl AsyncMessageHandler for AsyncAllocateMessageHandler {
    async fn handle(&self, base_session_id: String, data: &[u8], message_id: Option<String>, connections: Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>) -> Result<String, ApiError> {
        // 解析请求数据
        let ws_message = serde_json::from_slice::<WsMessage<RequestMessage>>(data).unwrap();
        let (request_session_id, resources) = match &ws_message.data {
            RequestMessage::AsyncAllocate { session_id, resources } => (session_id, resources.clone()),
            _ => {
                return Ok(self.new_default_resp(base_session_id, 1));
            }
        };

        // 校验session_id
        let err_code = self.intercept(&base_session_id, request_session_id);
        if err_code != 0 {
            return Ok(self.new_default_resp(request_session_id.clone(), err_code));
        }

        let resource_app = self.resource_app.clone();
        let connections = connections.clone();
        let session_id_copy = base_session_id.clone();
        let resources_copy = resources.clone();

        tokio::spawn(async move {
            // 创建释放通知消息
            tracing::info!("session {:?} async allocate resource {:?}", session_id_copy, resources_copy);

            if let Ok((success_resources, failed_resources)) = resource_app.handle_allocate(&session_id_copy, resources_copy).await {
                let allocate_resp = ResponseMessage::AsyncAllocateResult {
                    session_id: session_id_copy.clone(),
                    success_resources,
                    failed_resources,
                    error_code: 0,
                };
                let ws_message = WsMessage::new(None, allocate_resp);
                let json_str = match serde_json::to_string(&ws_message) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("Failed to serialize allocate response: {:?}", e);
                        return;
                    }
                };
                // 发送通知到对应会话
                let connections = connections.lock().await;
                if let Some(tx) = connections.get(&session_id_copy) {
                    if tx.send(json_str).await.is_err() {
                        tracing::warn!("Failed to send allocate response to session {}", session_id_copy);
                    }
                } else {
                    tracing::warn!("Session {} not found for allocate response", session_id_copy);
                }
            }
        });

        Ok(self.new_default_resp(base_session_id, 0))
    }

    fn new_default_resp(&self, session_id: String, error_code: u32) -> String {
        let response = ResponseMessage::AsyncAllocateResp {
            session_id: session_id.clone(),
            error_code,
        };
        let response_ws_message = WsMessage::new(None, response);

        serde_json::to_string(&response_ws_message).unwrap()
    }
}
