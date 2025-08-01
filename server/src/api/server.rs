use std::net::SocketAddr;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::time::{interval, Duration};

use crate::api::middlewares::logger;
use crate::{api::{error::ApiError, message::{RequestMessage, ResponseMessage, WsMessage}}, app::{resource_app::ResourceAppService, session_app::SessionAppService, config}}; 
use crate::domain::{resource_pool::ResourcePool, user_sessions::UserSessions};
use crate::infra::resource_clib::CLibResourceProvider;
use crate::api::message_handlers::*;

use axum::{
    extract::{ws::{Message, WebSocket}, WebSocketUpgrade}, response::Response, routing::get, Extension, Router
};
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;
use futures::{SinkExt, StreamExt};

/// `WebSocket协议升级处理器`
async fn handle_websocket(
    ws: WebSocketUpgrade,
    Extension(api_server): Extension<Arc<ApiServer>>,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        api_server.handle_connection(socket).await;
    })
}

/// API服务
pub struct ApiServer {
    /// 会话应用服务
    session_app: Arc<SessionAppService>,
    /// 资源应用服务
    resource_app: Arc<ResourceAppService>,
    /// WebSocket连接映射
    connections: Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>,
    /// 资源分配消息处理器
    allocate_handler: Arc<AllocateMessageHandler>,
    /// 资源释放消息处理器
    release_handler: Arc<ReleaseMessageHandler>,
    /// 资源查询消息处理器
    query_handler: Arc<QueryMessageHandler>,
    /// 心跳消息处理器
    heartbeat_handler: Arc<HeartbeatMessageHandler>,
    /// 退出消息处理器
    exit_handler: Arc<ExitMessageHandler>,
}

impl ApiServer {
    /// 初始化API服务
    #[must_use] pub fn new(
        session_app: Arc<SessionAppService>,
        resource_app: Arc<ResourceAppService>,
    ) -> Self {
        Self {
            session_app: session_app.clone(),
            resource_app: resource_app.clone(),
            connections: Arc::new(Mutex::new(HashMap::new())),
            allocate_handler: Arc::new(AllocateMessageHandler::new(resource_app.clone())),
            release_handler: Arc::new(ReleaseMessageHandler::new(resource_app.clone())),
            query_handler: Arc::new(QueryMessageHandler::new(resource_app.clone())),
            heartbeat_handler: Arc::new(HeartbeatMessageHandler::new(session_app.clone())),
            exit_handler: Arc::new(ExitMessageHandler::new(session_app.clone(), resource_app.clone())),
        }
    }

    /// 创建默认配置的API服务器实例
    pub fn init_default() -> Arc<Self> {
        // 初始化依赖
        let user_sessions = Arc::new(UserSessions::new(config::MAX_SESSION_NUM));
        let resource_pool = Arc::new(ResourcePool::new(config::MAX_RESOURCE_NUM, Arc::new(CLibResourceProvider)));
        
        let session_app = Arc::new(SessionAppService::new(user_sessions.clone()));
        let resource_app= ResourceAppService::new(user_sessions, resource_pool);
        let resource_app = Arc::new(resource_app);

        Arc::new(Self::new(session_app, resource_app))
    }

    /// 创建自定义配置的API服务器实例
    pub fn init_custom(max_session_num: usize, max_resource_num: usize) -> Arc<Self> {
        // 初始化依赖
        let user_sessions = Arc::new(UserSessions::new(max_session_num));
        let resource_pool = Arc::new(ResourcePool::new(max_resource_num, Arc::new(CLibResourceProvider)));
        
        let session_app = Arc::new(SessionAppService::new(user_sessions.clone()));
        let resource_app = ResourceAppService::new(user_sessions, resource_pool);
        let resource_app = Arc::new(resource_app);

        Arc::new(Self::new(session_app, resource_app))
    }

    /// `创建路由`
    pub fn create_router(self: Arc<Self>) -> Router {
        Router::new()
                .route("/ws", get(handle_websocket))
                .layer(Extension(self.clone()))
    }

    /// `启动HTTP服务并挂载WebSocket路由`
    pub async fn run(&self, addr: SocketAddr, route: Router) {
        let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
        axum::serve(listener, route).await.unwrap();
    }

    /// 连接生命周期管理
    async fn handle_connection(self: Arc<Self>, socket: WebSocket) {
        let session_id: Uuid = Uuid::new_v4();
        let session_id_str = session_id.to_string();
        

        // 1. 创建统一消息通道（合并command和response）
        let (message_tx, mut message_rx) = mpsc::channel(100); // 统一发送/响应通道
        let (mut ws_sender, mut ws_receiver) = socket.split(); // WebSocket通道保留

        // 注册到全局连接映射
        {
            let mut connections = self.connections.lock().await;
            connections.insert(session_id_str.clone(), message_tx.clone());
        }

        // 添加会话记录
        self.session_app
            .handle_add_session(session_id_str.clone())
            .await
            .unwrap();

        // 发送会话ID，封装为JSON格式
        let session_id_resp = ResponseMessage::SessionIdResp { session_id: session_id_str.clone() };
        let ws_message = WsMessage::new(None, session_id_resp);
        let json_str = serde_json::to_string(&ws_message).expect("Failed to serialize session_id response");
        ws_sender.send(Message::Text(json_str.into())).await.unwrap();

        // 启动独立任务处理接收消息
        let recv_task = {
            let api_server = self.clone();
            let message_tx = message_tx.clone();
            tokio::spawn(async move {
                loop {
                    // 显式存储next()结果，避免临时值生命周期问题
                    let next_result = ws_receiver.next().await;
                    match next_result {
                        Some(Ok(msg)) => {
                            // 显式存储into_text()结果
                            let text_result = msg.into_text();
                            if let Ok(text) = text_result {
                                // 显式存储process_incoming()结果
                                let process_result = api_server.process_incoming(&session_id_str, &text, message_tx.clone()).await;
                                if let Err(e) = process_result {
                                    tracing::info!("消息处理错误: {:?}", e);
                                }
                            }
                        },
                        Some(Err(e)) => {
                            tracing::warn!("WebSocket接收错误: {:?}", e);
                            break;
                        },
                        None => break,
                    }
                }
            })
        };

        // 启动独立任务处理发送消息
        let send_task = tokio::spawn(async move {
            while let Some(msg) = message_rx.recv().await {
                if ws_sender.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
        });

        // 连接结束时清理
        let session_app = self.session_app.clone();
        let resource_app = self.resource_app.clone();
        let connections = self.connections.clone();
        tokio::spawn(async move {
            let session_id_copy = session_id.to_string();
            // 等待连接结束
            tokio::select! {
                _ = recv_task => {},
                _ = send_task => {},
            }
            
            // 删除会话记录
            match session_app.handle_remove_session(&session_id_copy, resource_app).await {
                Ok(_) => {
                    tracing::info!("Successfully handle exit for session {}", session_id_copy);
                },
                Err(e) =>  {
                    tracing::warn!("Failed to handle exit for session {}: {:?}", session_id_copy, e);
                }
            }
            // 移除会话映射
            connections.lock().await.remove(&session_id_copy);
        });
    }

    /// 处理消息
    async fn process_incoming(
        &self,
        session_id: &str,
        text: &str,
        message_tx: mpsc::Sender<String> // 使用统一消息通道
    ) -> Result<(), ApiError> {
        // 先解析为WsMessage<RequestMessage>
        match serde_json::from_str::<WsMessage<RequestMessage>>(text) {
            Ok(ws_message) => {
                // 从WsMessage中提取RequestMessage并调用相应的处理器
                let response = match ws_message.data {
                    RequestMessage::Allocate { .. } => {
                        self.allocate_handler.handle(session_id.to_string(), text.as_bytes(), ws_message.id).await?
                    }
                    RequestMessage::Release { .. } => {
                        self.release_handler.handle(session_id.to_string(), text.as_bytes(), ws_message.id).await?
                    }
                    RequestMessage::Query { .. } => {
                        self.query_handler.handle(session_id.to_string(), text.as_bytes(), ws_message.id).await?
                    }
                    RequestMessage::Heartbeat { .. } => {
                        self.heartbeat_handler.handle(session_id.to_string(), text.as_bytes(), ws_message.id).await?
                    }
                    RequestMessage::Exit { .. } => {
                        self.exit_handler.handle(session_id.to_string(), text.as_bytes(), ws_message.id).await?
                    }
                };

                // 发送响应
                message_tx.send(response).await.map_err(|e| ApiError::SendMessageFailed(e.to_string()))?;
            }
            Err(e) => {
                tracing::error!("Invalid message: {}, error: {:?}", text, e);
                // 可以考虑发送错误响应
            }
        }
        Ok(())
    }

    // 消息处理方法已迁移到message_handlers模块中的处理器实现
    // 具体请查看api/message_handlers目录下的各个文件
    // AllocateMessageHandler, ReleaseMessageHandler, QueryMessageHandler, HeartbeatMessageHandler, ExitMessageHandler

    /// 处理超时通知
    pub fn spawn_timeout_notify_worker(&self) {
        let resource_app = self.resource_app.clone();
        let connections = self.connections.clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                // 创建释放通知消息
                let timeout_resources = resource_app.get_and_release_timeout_resources().await;
                if timeout_resources.is_empty() {
                    continue;
                }
                for (session_id, resource_ids) in timeout_resources {
                    let release_resp = ResponseMessage::TimeoutResourceInfo {
                        session_id: session_id.clone(),
                        timeout_resources: resource_ids,
                    };
                    let ws_message = WsMessage::new(None, release_resp);
                    let json_str = match serde_json::to_string(&ws_message) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("Failed to serialize timeout release notification: {:?}", e);
                            continue;
                        }
                    };
                    // 发送通知到对应会话
                    let connections = connections.lock().await;
                    if let Some(tx) = connections.get(&session_id) {
                        if tx.send(json_str).await.is_err() {
                            tracing::warn!("Failed to send timeout release notification to session {}", session_id);
                        }
                    } else {
                        tracing::warn!("Session {} not found for timeout release notification", session_id);
                    }
                }
            }
        });
    }
}


        // tokio::spawn(async move {
        //     let mut interval = interval(Duration::from_secs(1));
        //     loop {
        //         interval.tick().await;
                
        //         // 检测超时资源（10秒超时）
        //         let timeouts = pool.get_timeout_resources(10).await;
        //         for (resource_id, session_id) in timeouts {
        //             // 双重验证防止状态变更
        //             if pool.is_held_by_session(resource_id, &session_id).await {
        //                 // 释放资源
        //                 if let Err(e) = pool.release_resource(resource_id).await {
        //                     tracing::info!("超时释放失败: {:?}", e);
        //                     continue;
        //                 }
                        
        //                 // 清理会话
        //                 if let Err(e) = sessions.user_release_resources(&session_id, vec![resource_id]).await {
        //                     tracing::info!("会话清理失败: {:?}", e);
        //                 }
                        
        //                 // 发送通知
        //                 if let Err(e) = tx.send((session_id, resource_id)).await {
        //                     tracing::info!("通知发送失败: {:?}", e);
        //                 }
        //             }
        //         }
        //     }
        // });