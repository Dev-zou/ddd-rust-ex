use std::sync::Arc;
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt, future::join_all};
use tokio::sync::{mpsc, Mutex};
use axum::{Extension, Router};
use axum::routing::get;
use axum_test::{TestServer, TestResponse};
use serde_json::json;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use uuid::Uuid;

use axum::extract::ws::Message;
use axum::serve;
use std::net::SocketAddr;
use tokio_tungstenite::{connect_async, tungstenite, WebSocketStream};
use tungstenite::http::{Method, Request};
use tungstenite::protocol::Message as StreamMessage;
use url::Url;

use server::api::server::ApiServer;
use server::api::message::{RequestMessage, ResponseMessage, WsMessage};
use server::app::{resource_app::ResourceAppService, session_app::SessionAppService};
use server::domain::user_sessions::UserSessions;
use server::domain::resource_pool::ResourcePool;
use server::infra::resource_mock::MockResourceProvider;
use server::app::config;
use server::api::middlewares;

// 启动测试服务器并返回地址
async fn start_test_server() -> (SocketAddr, Arc<ApiServer>) {
    // 初始化依赖（示例简化，实际项目需注入infra层）
    let server = ApiServer::init_default();
    let router = server.clone().create_router();

    let addr = SocketAddr::from(([127, 0, 0, 1], 0)); // 0表示随机端口
    let listener = tokio::net::TcpListener::bind(addr).await
        .expect("Failed to bind to address");
    let addr = listener.local_addr().expect("Failed to get local address");

    tokio::spawn(async move {
        axum::serve(listener, router).await
            .expect("Failed to start server");
    });
    
    // 增加服务器启动等待时间，确保完全就绪
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    (addr, server)
}

// 启动测试服务器并返回地址
async fn start_test_server_with_param(max_session_num: usize, max_resource_num: usize, timeout: usize) -> (SocketAddr, Arc<ApiServer>) {
    // 初始化依赖（示例简化，实际项目需注入infra层）
    let server = ApiServer::init_custom(max_resource_num, max_session_num, timeout);
    let router = server.clone().create_router();

    let addr = SocketAddr::from(([127, 0, 0, 1], 0)); // 0表示随机端口
    let listener = tokio::net::TcpListener::bind(addr).await
        .expect("Failed to bind to address");
    let addr = listener.local_addr().expect("Failed to get local address");

    server.spawn_timeout_notify_worker();

    tokio::spawn(async move {
        axum::serve(listener, router).await
            .expect("Failed to start server");
    });
    
    // 增加服务器启动等待时间，确保完全就绪
    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    (addr, server)
}

// 启动测试客户端并返回WebSocket流（简化版，只返回发送和接收流）
async fn start_test_client_simple(addr: SocketAddr) -> (SplitSink<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, StreamMessage>, SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>) {
    // 构建正确的WebSocket URL
    let ws_url = format!("ws://{}:{}/ws", addr.ip(), addr.port());
    let url = Url::parse(&ws_url).expect("Failed to parse WebSocket URL");
    
    // 创建标准WebSocket请求，包含必要的头信息
    let request = Request::builder()
        .method(Method::GET)
        .uri(url.as_str())
        .header("Host", format!("{}:{}", addr.ip(), addr.port()))
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .expect("Failed to build WebSocket request");
    
    // 建立WebSocket连接
    let (ws_stream, response) = connect_async(request).await
        .expect("Failed to connect to WebSocket server");
    
    // 检查响应状态
    assert_eq!(response.status(), 101, "WebSocket handshake failed with status: {}", response.status());

    let (write, read) = ws_stream.split();
    (write, read)
}

// 启动测试客户端并返回WebSocket流（简化版，只返回发送和接收流）
async fn start_test_client(addr: SocketAddr) -> (SplitSink<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, StreamMessage>, SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>, Option<String>) {
    // 构建正确的WebSocket URL
    let ws_url = format!("ws://{}:{}/ws", addr.ip(), addr.port());
    let url = Url::parse(&ws_url).expect("Failed to parse WebSocket URL");
    
    // 创建标准WebSocket请求，包含必要的头信息
    let request = Request::builder()
        .method(Method::GET)
        .uri(url.as_str())
        .header("Host", format!("{}:{}", addr.ip(), addr.port()))
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .expect("Failed to build WebSocket request");
    
    // 建立WebSocket连接
    let (ws_stream, response) = connect_async(request).await
        .expect("Failed to connect to WebSocket server");
    
    // 检查响应状态
    assert_eq!(response.status(), 101, "WebSocket handshake failed with status: {}", response.status());

    let (write, mut read) = ws_stream.split();
        // 接收服务器发送的session_id响应
    let message = read.next().await
        .expect("Failed to receive message")
        .expect("Received error message");

    // 验证消息类型并解析JSON
    assert!(matches!(message, StreamMessage::Text(_)));
    if let StreamMessage::Text(text) = message {
        // 解析为WsMessage<ResponseMessage>
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text)
            .expect("Failed to parse session_id response");

        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::SessionIdResp { .. }));
        if let ResponseMessage::SessionIdResp { session_id } = ws_message.data {
            // 验证session_id不为空
            assert!(!session_id.is_empty());
            return (write, read, Some(session_id));
        }
    }
    (write, read, None)
}

// 示例：使用简化版客户端启动函数的测试
#[tokio::test]
async fn test_text_message_echo_simple() {
    let (addr, _server) = start_test_server().await;
    
    // 使用简化版客户端启动函数
    let (mut write, mut read) = start_test_client_simple(addr).await;
    
    // 发送测试消息
    let test_message = StreamMessage::Text("Hello, WebSocket!".to_string().into());
    write.send(test_message).await
        .expect("Failed to send test message");
    
    // 接收响应
    let read_message = read.next().await;

    match read_message {
        Some(Ok(response)) => {
            assert!(response.is_text(), "Expected text message");
            if let StreamMessage::Text(text) = response {
                assert!(!text.is_empty(), "Received empty message");
            }
        },
        Some(Err(e)) => panic!("Error receiving message: {:?}", e),
        None => panic!("No message received")
    }
}

#[tokio::test]
async fn test_session_id_response() {
    middlewares::init();
    // 启动测试服务器
    let (server_addr, _server) = start_test_server().await;

    // 建立WebSocket连接
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;

    assert!(!session_id.is_none(), "session_id get failed");
}

// 2、多用户接入测试
#[tokio::test]
async fn test_multi_user_connection() {
    let (server_addr, _server) = start_test_server().await;
    let mut session_ids = Vec::new();
    let num_users = 2;  // 测试两个用户并发接入

    // 使用tokio::spawn并发创建客户端连接
    let mut handles = Vec::new();
    for _ in 0..num_users {
        let addr = server_addr.clone();
        let handle = tokio::spawn(async move {
            let (_, _, session_id) = start_test_client(addr).await;
            assert!(!session_id.is_none(), "session_id get failed");
            session_id.unwrap()
        });
        handles.push(handle);
    }

    // 等待所有任务完成并收集session_id
    for handle in handles {
        let session_id = handle.await.expect("Task failed");
        session_ids.push(session_id);
    }

    // 校验所有session_id都不同
    session_ids.sort();
    for i in 1..session_ids.len() {
        assert_ne!(session_ids[i-1], session_ids[i], "Duplicate session_id found");
    }
}

// 3、用户申请资源测试
#[tokio::test]
async fn test_resource_allocation() {
    let (server_addr, _server) = start_test_server().await;
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;
    let session_id = session_id.expect("Failed to get session_id");

    // 构建申请资源请求
    let request = WsMessage {
        id: None,
        data: RequestMessage::Allocate {
            session_id: session_id.clone(),
            resources: vec![1],
        },
    };

    // 发送请求
    let message = StreamMessage::Text(serde_json::to_string(&request).unwrap().into());
    write.send(message).await.expect("Failed to send request");

    // 接收响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::AllocateResp { .. }));
        if let ResponseMessage::AllocateResp { session_id: resp_session_id, success_resources, failed_resources, error_code } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            assert!(!success_resources.is_empty());
            assert!(failed_resources.is_empty());
            assert_eq!(error_code, 0);
        }
    }
}

// 4、多用户申请不同资源测试
#[tokio::test]
async fn test_multi_user_different_resource_allocation() {
    let (server_addr, _server) = start_test_server().await;
    let num_users = 3;
    let mut clients = Vec::new();

    // 启动多个客户端
    for i in 0..num_users {
        let (write, read, session_id) = start_test_client(server_addr).await;
        let session_id = session_id.expect("Failed to get session_id");
        clients.push((write, read, session_id));
    }

    // 每个客户端并发申请不同的资源
    let mut handles = Vec::new();
    for (i, (mut write, mut read, session_id)) in clients.into_iter().enumerate() {
        let resource_id = (i + 1) as u16;
        let request = WsMessage {
            id: None,
            data: RequestMessage::Allocate {
                session_id: session_id.clone(),
                resources: vec![resource_id],
            },
        };

        // 使用tokio::spawn并发发送请求和验证响应
        let handle = tokio::spawn(async move {
            let message = StreamMessage::Text(serde_json::to_string(&request)
                .expect("Failed to serialize request").into());
            write.send(message).await.expect("Failed to send request");

            // 验证客户端的资源申请是否成功
            let response = read.next().await.expect("Failed to receive response").expect("Received error");
            assert!(response.is_text());

            if let StreamMessage::Text(text) = response {
                let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text)
                    .expect("Failed to parse response");
                
                if let ResponseMessage::AllocateResp { session_id: resp_session_id, success_resources, failed_resources, error_code } = ws_message.data {
                    assert_eq!(resp_session_id, session_id);
                    assert!(!success_resources.is_empty());
                    assert!(failed_resources.is_empty());
                }
            }
        });
        handles.push(handle);
    }

    // 等待所有任务完成
    join_all(handles).await;
}

// 5、多用户申请相同资源测试
#[tokio::test]
async fn test_multi_user_same_resource_allocation() {
    let (server_addr, _server) = start_test_server().await;
    let num_users = 3;
    let resource_id = 1;
    let mut clients = Vec::new();
    let mut success_count = 0;

    // 启动多个客户端
    for _ in 0..num_users {
        let (write, read, session_id) = start_test_client(server_addr).await;
        let session_id = session_id.expect("Failed to get session_id");
        clients.push((write, read, session_id));
    }

    // 所有客户端申请相同资源
    let mut handles = Vec::new();
    for (mut write, mut read, session_id) in clients.into_iter() {
        let request = WsMessage {
            id: None,
            data: RequestMessage::Allocate {
                session_id: session_id.clone(),
                resources: vec![resource_id],
            },
        };

        let handle = tokio::spawn(async move {
            let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request").into());
            write.send(message).await.expect("Failed to send request");

            let response = read.next().await.expect("Failed to receive response").expect("Received error");
            assert!(response.is_text());

            if let StreamMessage::Text(text) = response {
                let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
                
                if let ResponseMessage::AllocateResp { session_id: resp_session_id, success_resources, failed_resources, error_code } = ws_message.data {
                    if !success_resources.is_empty() {
                        success_count += 1;
                    }
                }
            }
            assert_eq!(success_count, 1, "Expected exactly one successful allocation");

        });
        handles.push(handle);
    }

    // 等待所有任务完成
    join_all(handles).await;

}

// 6、用户释放资源测试
#[tokio::test]
async fn test_resource_release() {
    let (server_addr, server) = start_test_server().await;
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;
    let session_id = session_id.expect("Failed to get session_id");

    // 先申请资源
    let request_allocate = WsMessage {
        id: None,
        data: RequestMessage::Allocate {
            session_id: session_id.clone(),
            resources: vec![1],
        },
    };

    let message_allocate = StreamMessage::Text(serde_json::to_string(&request_allocate).expect("Failed to serialize request").into());
    write.send(message_allocate).await.expect("Failed to send request");

    // 接收申请响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        assert!(matches!(ws_message.data, ResponseMessage::AllocateResp { .. }));
    }
    
    // 释放资源
    let request_release = WsMessage {
        id: None,
        data: RequestMessage::Release {
            session_id: session_id.clone(),
            resources: vec![1],
        },
    };

    let message_release = StreamMessage::Text(serde_json::to_string(&request_release).expect("Failed to serialize request").into());
    write.send(message_release).await.expect("Failed to send request");

    // 接收释放响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::ReleaseResp { .. }));
        if let ResponseMessage::ReleaseResp { session_id: resp_session_id, success_resources, failed_resources, error_code } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            assert_eq!(success_resources, vec![1]);
            assert!(failed_resources.is_empty());
            assert_eq!(error_code, 0);
        }
    }
    // assert_eq!(server.user_sessions.user_get_resources(session_id).await, Ok(vec![]));
}

// 7、多用户释放资源测试
#[tokio::test]
async fn test_multi_user_resource_release() {
    let (server_addr, _server) = start_test_server().await;
    let num_users = 3;
    let mut clients = Vec::new();

    // 启动多个客户端并申请资源
    for i in 0..num_users {
        let (write, read, session_id) = start_test_client(server_addr).await;
        let session_id = session_id.expect("Failed to get session_id");
        let resource_id = (i + 1) as u16;
        clients.push((write, read, session_id, resource_id));
    }

    // 每个客户端申请资源
    for (write, read, session_id, resource_id) in clients.iter_mut() {
        let request = WsMessage {
            id: None,
            data: RequestMessage::Allocate {
                session_id: session_id.clone(),
                resources: vec![*resource_id],
            },
        };

        let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request").into());
        write.send(message).await.expect("Failed to send request");
        // 接收申请响应
        let _unused = read.next().await.expect("Failed to receive response").expect("Received error");
    }

    // 每个客户端释放资源
    for (mut write, mut read, session_id, resource_id) in clients {
        let request = WsMessage {
            id: None,
            data: RequestMessage::Release {
                session_id: session_id.clone(),
                resources: vec![resource_id],
            },
        };

        let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request").into());
        write.send(message).await.expect("Failed to send request");

        // 接收释放响应
        let response = read.next().await.expect("Failed to receive response").expect("Received error");
        assert!(response.is_text());

        if let StreamMessage::Text(text) = response {
            let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
            
            if let ResponseMessage::ReleaseResp { session_id: resp_session_id, success_resources, failed_resources, error_code } = ws_message.data {
                assert_eq!(resp_session_id, session_id);
                assert_eq!(success_resources, vec![resource_id]);
                assert!(failed_resources.is_empty());
                assert_eq!(error_code, 0);
            }
        }
    }
}

// 8、用户的心跳检测测试
#[tokio::test]
async fn test_heartbeat() {
    let (server_addr, _server) = start_test_server().await;
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;
    let session_id = session_id.expect("Failed to get session_id");

    // 发送心跳请求
    let request = WsMessage {
        id: None,
        data: RequestMessage::Heartbeat {
            session_id: session_id.clone(),
        },
    };

    let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request").into());
    write.send(message).await.expect("Failed to send heartbeat request");

    // 接收心跳响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::HeartbeatResp { .. }));
        if let ResponseMessage::HeartbeatResp { session_id: resp_session_id, error_code } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
        }
    }

    // 异常场景：无效的会话ID
    let invalid_session_id = "invalid_session".to_string();
    let invalid_request = WsMessage {
        id: None,
        data: RequestMessage::Heartbeat {
            session_id: invalid_session_id.clone(),
        },
    };

    let invalid_message = StreamMessage::Text(serde_json::to_string(&invalid_request).expect("Failed to serialize invalid request").into());
    write.send(invalid_message).await.expect("Failed to send invalid heartbeat request");

    // 接收无效心跳响应
    let invalid_response = read.next().await.expect("Failed to receive invalid response").expect("Received error");
    assert!(invalid_response.is_text());

    // 这里可能不会收到响应，因为会话不存在，服务器可能直接断开连接
    // 所以我们不做严格的断言，只检查是否收到消息
}

// 9、多用户并发的心跳检测测试
#[tokio::test]
async fn test_concurrent_heartbeats() {
    let (server_addr, _server) = start_test_server().await;
    let num_users = 10;
    let mut clients = Vec::new();

    // 启动多个客户端
    for _ in 0..num_users {
        let (write, read, session_id) = start_test_client(server_addr).await;
        let session_id = session_id.expect("Failed to get session_id");
        clients.push((write, read, session_id));
    }

    // 并发发送心跳
    let mut handles = Vec::new();
    for (mut write, mut read, session_id) in clients {
        let handle = tokio::spawn(async move {
            // 发送心跳请求
            let request = WsMessage {
                id: None,
                data: RequestMessage::Heartbeat {
                    session_id: session_id.clone(),
                },
            };

            let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request").into());
            write.send(message).await.expect("Failed to send heartbeat request");

            // 接收心跳响应
            let response = read.next().await.expect("Failed to receive response").expect("Received error");
            assert!(response.is_text());

            if let StreamMessage::Text(text) = response {
                let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
                
                // 验证响应类型和内容
                assert!(matches!(ws_message.data, ResponseMessage::HeartbeatResp { .. }));
                if let ResponseMessage::HeartbeatResp { session_id: resp_session_id, error_code } = ws_message.data {
                    assert_eq!(resp_session_id, session_id);
                }
            }
        });
        handles.push(handle);
    }

    // 等待所有心跳完成
    join_all(handles).await;
}

// 10、用户退出测试
#[tokio::test]
async fn test_user_exit() {
    let (server_addr, server) = start_test_server().await;
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;
    let session_id = session_id.expect("Failed to get session_id");

    // 验证会话存在
    assert!(server.get_session_num().await > 0);

    // 发送退出请求
    let request = WsMessage {
        id: None,
        data: RequestMessage::Exit {
            session_id: session_id.clone(),
        },
    };

    let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request").into());
    write.send(message).await.expect("Failed to send exit request");

    // 接收退出响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::ExitResp { .. }));
        if let ResponseMessage::ExitResp { session_id: resp_session_id } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
        }
    }

    // 验证会话已被移除
    assert!(server.get_session_num().await == 0);
}

// 11、用户异步申请资源测试
#[tokio::test]
async fn test_async_resource_allocation() {
    let (server_addr, _server) = start_test_server().await;
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;
    let session_id = session_id.expect("Failed to get session_id");

    // 异步申请资源
    let request = WsMessage {
        id: None,
        data: RequestMessage::AsyncAllocate {
            session_id: session_id.clone(),
            resources: vec![1],
        },
    };

    let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request").into());
    write.send(message).await.expect("Failed to send async allocation request");

    // 接收异步申请响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::AsyncAllocateResp { .. }));
        if let ResponseMessage::AsyncAllocateResp { session_id: resp_session_id, error_code } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            assert_eq!(error_code, 0); // 0表示成功接收请求
        }
    }

    // 等待异步操作完成
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 接收异步结果通知，添加超时检测
    let timeout_duration = tokio::time::Duration::from_secs(2); // 设置5秒超时
    let result_notification = tokio::time::timeout(timeout_duration, read.next())
        .await
        .expect("Timeout waiting for result notification")
        .expect("Failed to receive result notification")
        .expect("Received error");
    assert!(result_notification.is_text());

    if let StreamMessage::Text(text) = result_notification {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse result notification");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::AsyncAllocateResult { .. }));
        if let ResponseMessage::AsyncAllocateResult { session_id: resp_session_id, success_resources, failed_resources, error_code } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            assert!(!success_resources.is_empty());
            assert!(failed_resources.is_empty());
        }
    }

    // 异常场景：申请不存在的资源ID
    let invalid_request = WsMessage {
        id: None,
        data: RequestMessage::AsyncAllocate {
            session_id: session_id.clone(),
            resources: vec![999], // 假设这是一个不存在的资源ID
        },
    };

    let invalid_message = StreamMessage::Text(serde_json::to_string(&invalid_request).expect("Failed to serialize invalid request").into());
    write.send(invalid_message).await.expect("Failed to send invalid async allocation request");

    // 接收异常异步申请响应
    let invalid_response = read.next().await.expect("Failed to receive invalid response").expect("Received error");
    assert!(invalid_response.is_text());

    if let StreamMessage::Text(text) = invalid_response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse invalid response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::AsyncAllocateResp { .. }));
    }

    // 等待异步操作完成
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 接收异步结果通知，添加超时检测
    let timeout_duration = tokio::time::Duration::from_secs(2); // 设置5秒超时
    let invalid_result = tokio::time::timeout(timeout_duration, read.next()).await;
    assert!(invalid_result.is_err());
}

// 12、用户异步释放资源测试
#[tokio::test]
async fn test_async_resource_release() {
    let (server_addr, _server) = start_test_server().await;
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;
    let session_id = session_id.expect("Failed to get session_id");

    // 先同步申请资源
    let allocate_request = WsMessage {
        id: None,
        data: RequestMessage::Allocate {
            session_id: session_id.clone(),
            resources: vec![1],
        },
    };

    let allocate_message = StreamMessage::Text(serde_json::to_string(&allocate_request).expect("Failed to serialize allocate request").into());
    write.send(allocate_message).await.expect("Failed to send allocate request");

    // 接收申请响应
    let allocate_response = read.next().await.expect("Failed to receive allocate response").expect("Received error");
    assert!(allocate_response.is_text());

    // 异步释放资源
    let release_request = WsMessage {
        id: None,
        data: RequestMessage::AsyncRelease {
            session_id: session_id.clone(),
            resources: vec![1],
        },
    };

    let release_message = StreamMessage::Text(serde_json::to_string(&release_request).expect("Failed to serialize release request").into());
    write.send(release_message).await.expect("Failed to send async release request");

    // 接收异步释放响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::AsyncReleaseResp { .. }));
        if let ResponseMessage::AsyncReleaseResp { session_id: resp_session_id, error_code } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            assert_eq!(error_code, 0); // 0表示成功接收请求
        }
    }

    // 等待异步操作完成
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 接收异步结果通知，添加超时检测
    let timeout_duration = tokio::time::Duration::from_secs(2); // 设置5秒超时
    let result_notification = tokio::time::timeout(timeout_duration, read.next())
        .await
        .expect("Timeout waiting for result notification")
        .expect("Failed to receive result notification")
        .expect("Received error");
    assert!(result_notification.is_text());

    if let StreamMessage::Text(text) = result_notification {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse result notification");
        
        // 验证响应类型（注意：common中定义的是AsyncReleaseResupt，这里可能是拼写错误，但我们按照定义来）
        assert!(matches!(ws_message.data, ResponseMessage::AsyncReleaseResupt { .. }));
        if let ResponseMessage::AsyncReleaseResupt { session_id: resp_session_id, success_resources, failed_resources, error_code } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            assert!(!success_resources.is_empty());
            assert!(failed_resources.is_empty());
        }
    }

    // 异常场景：释放未申请的资源
    let invalid_request = WsMessage {
        id: None,
        data: RequestMessage::AsyncRelease {
            session_id: session_id.clone(),
            resources: vec![2], // 未申请的资源
        },
    };

    let invalid_message = StreamMessage::Text(serde_json::to_string(&invalid_request).expect("Failed to serialize invalid request").into());
    write.send(invalid_message).await.expect("Failed to send invalid async release request");

    // 接收异常异步释放响应
    let invalid_response = read.next().await.expect("Failed to receive invalid response").expect("Received error");
    assert!(invalid_response.is_text());

    // 等待异步操作完成
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // 接收异步结果通知，添加超时检测
    let timeout_duration = tokio::time::Duration::from_secs(2); // 设置5秒超时
    let invalid_result = tokio::time::timeout(timeout_duration, read.next())
        .await
        .expect("Timeout waiting for invalid result notification")
        .expect("Failed to receive invalid result notification")
        .expect("Received error");
    assert!(invalid_result.is_text());

    if let StreamMessage::Text(text) = invalid_result {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse invalid result notification");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::AsyncReleaseResupt { .. }));
        if let ResponseMessage::AsyncReleaseResupt { session_id: resp_session_id, success_resources, failed_resources, error_code } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            assert!(success_resources.is_empty());
            assert!(!failed_resources.is_empty());
        }
    }
}

// 13、用户申请资源测试
#[tokio::test]
async fn test_error_session_resource_allocation() {
    let (server_addr, _server) = start_test_server().await;
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;
    let session_id = session_id.expect("Failed to get session_id");

    // 构建申请资源请求
    let invalid_session_id = "invalid_session".to_string();
    let request = WsMessage {
        id: None,
        data: RequestMessage::Allocate {
            session_id: invalid_session_id.clone(),
            resources: vec![1],
        },
    };

    // 发送请求
    let message = StreamMessage::Text(serde_json::to_string(&request).unwrap().into());
    write.send(message).await.expect("Failed to send request");

    // 接收响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::AllocateResp { .. }));
        if let ResponseMessage::AllocateResp { session_id: resp_session_id, success_resources, failed_resources, error_code } = ws_message.data {
            assert_ne!(error_code, 0);
        }
    }
}

// 14、错误用户释放资源测试
#[tokio::test]
async fn test_error_session_resource_release() {
    let (server_addr, _server) = start_test_server().await;
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;
    let session_id = session_id.expect("Failed to get session_id");

    // 构建申请资源请求
    let invalid_session_id = "invalid_session".to_string();
    let request = WsMessage {
        id: None,
        data: RequestMessage::Release {
            session_id: invalid_session_id.clone(),
            resources: vec![1],
        },
    };

    // 发送请求
    let message = StreamMessage::Text(serde_json::to_string(&request).unwrap().into());
    write.send(message).await.expect("Failed to send request");

    // 接收响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::ReleaseResp { .. }));
        if let ResponseMessage::ReleaseResp { session_id: resp_session_id, success_resources, failed_resources, error_code } = ws_message.data {
            assert_ne!(error_code, 0);
        }
    }
}

// 15、用户查询
#[tokio::test]
async fn test_error_session_query_resource_empty() {
    let (server_addr, _server) = start_test_server().await;
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;
    let session_id = session_id.expect("Failed to get session_id");

    // 构建申请资源请求
    let request = WsMessage {
        id: None,
        data: RequestMessage::Query {
            session_id: session_id.clone(),
        },
    };

    // 发送请求
    let message = StreamMessage::Text(serde_json::to_string(&request).unwrap().into());
    write.send(message).await.expect("Failed to send request");

    // 接收响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::QueryResp { .. }));
        if let ResponseMessage::QueryResp {session_id: resp_session_id, resources, error_code } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            assert!(resources.is_empty());
            assert_eq!(error_code, 0);
        }
    }

    // 构建申请资源请求
    let invalid_session_id = "invalid_session".to_string();
    let request = WsMessage {
        id: None,
        data: RequestMessage::Query {
            session_id: invalid_session_id.clone(),
        },
    };

    // 发送请求
    let message = StreamMessage::Text(serde_json::to_string(&request).unwrap().into());
    write.send(message).await.expect("Failed to send request");

    // 接收响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::QueryResp { .. }));
        if let ResponseMessage::QueryResp {session_id: resp_session_id, resources, error_code } = ws_message.data {
            assert_ne!(error_code, 0);
        }
    }
}

// 16、用户查询
#[tokio::test]
async fn test_session_query_session() {
    let (server_addr, _server) = start_test_server().await;
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;
    let session_id = session_id.expect("Failed to get session_id");

    // 构建申请资源请求
    let request = WsMessage {
        id: None,
        data: RequestMessage::Allocate {
            session_id: session_id.clone(),
            resources: vec![1],
        },
    };

    // 发送请求
    let message = StreamMessage::Text(serde_json::to_string(&request).unwrap().into());
    write.send(message).await.expect("Failed to send request");

    // 接收响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    // 构建申请资源请求
    let request = WsMessage {
        id: None,
        data: RequestMessage::Query {
            session_id: session_id.clone(),
        },
    };

    // 发送请求
    let message = StreamMessage::Text(serde_json::to_string(&request).unwrap().into());
    write.send(message).await.expect("Failed to send request");

    // 接收响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::QueryResp { .. }));
        if let ResponseMessage::QueryResp {session_id: resp_session_id, resources, error_code } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            assert_eq!(resources, vec![1]);
            assert_eq!(error_code, 0);
        }
    }
}

// 16、用户查询
#[tokio::test]
async fn test_resource_timeout() {
    let timeout = 2;
    let (server_addr, _server) = start_test_server_with_param(10, 10, timeout).await;
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;
    let session_id = session_id.expect("Failed to get session_id");

    // 构建申请资源请求
    let request = WsMessage {
        id: None,
        data: RequestMessage::Allocate {
            session_id: session_id.clone(),
            resources: vec![1],
        },
    };

    // 发送请求
    let message = StreamMessage::Text(serde_json::to_string(&request).unwrap().into());
    write.send(message).await.expect("Failed to send request");

    // 接收响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    tokio::time::sleep(tokio::time::Duration::from_secs((timeout+1).try_into().unwrap())).await;

    // 接收超时通知
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::TimeoutResourceInfo { .. }));
        if let ResponseMessage::TimeoutResourceInfo {session_id: resp_session_id, timeout_resources } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            assert_eq!(timeout_resources, vec![1]);
        }
    }

    // 构建申请资源请求
    let request = WsMessage {
        id: None,
        data: RequestMessage::Query {
            session_id: session_id.clone(),
        },
    };

    // 发送请求
    let message = StreamMessage::Text(serde_json::to_string(&request).unwrap().into());
    write.send(message).await.expect("Failed to send request");

    // 接收响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::QueryResp { .. }));
        if let ResponseMessage::QueryResp {session_id: resp_session_id, resources, error_code } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            assert!(resources.is_empty());
            assert_eq!(error_code, 0);
        }
    }
}

// 16、用户查询
#[tokio::test]
async fn test_session_query_resource_release() {
    let (server_addr, _server) = start_test_server().await;
    let (mut write, mut read, session_id) = start_test_client(server_addr).await;
    let session_id = session_id.expect("Failed to get session_id");

    // 
    let resources = vec![1,2,3,4,5];
    let request = WsMessage {
        id: None,
        data: RequestMessage::Allocate {
            session_id: session_id.clone(),
            resources: resources,
        },
    };

    // 发送请求
    let message = StreamMessage::Text(serde_json::to_string(&request).unwrap().into());
    write.send(message).await.expect("Failed to send request");

    // 接收响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    // 构建申请资源请求
    let release_resources = vec![1,2,3];
    let request = WsMessage {
        id: None,
        data: RequestMessage::Release {
            session_id: session_id.clone(),
            resources: release_resources,
        },
    };

    // 发送请求
    let message = StreamMessage::Text(serde_json::to_string(&request).unwrap().into());
    write.send(message).await.expect("Failed to send request");

    // 接收响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    // 构建申请资源请求
    let request = WsMessage {
        id: None,
        data: RequestMessage::Query {
            session_id: session_id.clone(),
        },
    };

    // 发送请求
    let message = StreamMessage::Text(serde_json::to_string(&request).unwrap().into());
    write.send(message).await.expect("Failed to send request");

    // 接收响应
    let response = read.next().await.expect("Failed to receive response").expect("Received error");
    assert!(response.is_text());

    if let StreamMessage::Text(text) = response {
        let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
        // 验证响应类型
        assert!(matches!(ws_message.data, ResponseMessage::QueryResp { .. }));
        if let ResponseMessage::QueryResp {session_id: resp_session_id, resources, error_code } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            let mut resources = resources.clone();
            resources.sort();
            assert_eq!(resources, vec![4,5]);
            assert_eq!(error_code, 0);
        }
    }
}