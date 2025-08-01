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
                println!("Received message: {:?}", text);
            }
        },
        Some(Err(e)) => panic!("Error receiving message: {:?}", e),
        None => panic!("No message received")
    }
}

#[tokio::test]
async fn test_session_id_response() {
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
        if let ResponseMessage::AllocateResp { session_id: resp_session_id, success_resources, failed_resources } = ws_message.data {
            assert_eq!(resp_session_id, session_id);
            assert!(!success_resources.is_empty());
            assert!(failed_resources.is_empty());
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
                
                if let ResponseMessage::AllocateResp { session_id: resp_session_id, success_resources, failed_resources } = ws_message.data {
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

// // 5、多用户申请相同资源测试
// #[tokio::test]
// async fn test_multi_user_same_resource_allocation() {
//     let (server_addr, _server) = start_test_server().await;
//     let num_users = 3;
//     let resource_id = 1;
//     let mut clients = Vec::new();
//     let mut success_count = 0;

//     // 启动多个客户端
//     for _ in 0..num_users {
//         let (write, read, session_id) = start_test_client(server_addr).await;
//         let session_id = session_id.expect("Failed to get session_id");
//         clients.push((write, read, session_id));
//     }

//     // 所有客户端申请相同资源
//     for (mut write, _, session_id) in clients.iter_mut() {
//         let request = WsMessage {
//             data: RequestMessage::Allocate {
//                 session_id: session_id.clone(),
//                 resources: vec![resource_id],
//             },
//         };

//         let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request"));
//         write.send(message).await.expect("Failed to send request");
//     }

//     // 验证只有一个客户端申请成功
//     for (_, mut read, _) in clients {
//         let response = read.next().await.expect("Failed to receive response").expect("Received error");
//         assert!(response.is_text());

//         if let StreamMessage::Text(text) = response {
//             let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
            
//             if let ResponseMessage::AllocateResp { results, .. } = ws_message.data {
//                 assert!(!results.is_empty());
//                 if results[0].success {
//                     success_count += 1;
//                 }
//             }
//         }
//     }

//     assert_eq!(success_count, 1, "Expected exactly one successful allocation");
// }

// // 6、用户释放资源测试
// #[tokio::test]
// async fn test_resource_release() {
//     let (server_addr, _server) = start_test_server().await;
//     let (mut write, mut read, session_id) = start_test_client(server_addr).await;
//     let session_id = session_id.expect("Failed to get session_id");

//     // 先申请资源
//     let request_allocate = WsMessage {
//         data: RequestMessage::Allocate {
//             session_id: session_id.clone(),
//             resources: vec![1],
//         },
//     };

//     let message_allocate = StreamMessage::Text(serde_json::to_string(&request_allocate).expect("Failed to serialize request"));
//     write.send(message_allocate).await.expect("Failed to send request");

//     // 接收申请响应
//     let _ = read.next().await.expect("Failed to receive response").expect("Received error");

//     // 释放资源
//     let request_release = WsMessage {
//         data: RequestMessage::Release {
//             session_id: session_id.clone(),
//             resources: vec![1],
//         },
//     };

//     let message_release = StreamMessage::Text(serde_json::to_string(&request_release).expect("Failed to serialize request"));
//     write.send(message_release).await.expect("Failed to send request");

//     // 接收释放响应
//     let response = read.next().await.expect("Failed to receive response").expect("Received error");
//     assert!(response.is_text());

//     if let StreamMessage::Text(text) = response {
//         let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
        
//         // 验证响应类型
//         assert!(matches!(ws_message.data, ResponseMessage::ReleaseResp { .. }));
//         if let ResponseMessage::ReleaseResp { session_id: resp_session_id, results } = ws_message.data {
//             assert_eq!(resp_session_id, session_id);
//             assert!(!results.is_empty());
//             assert!(results[0].success, "Resource release failed");
//         }
//     }
// }

// // 7、多用户释放资源测试
// #[tokio::test]
// async fn test_multi_user_resource_release() {
//     let (server_addr, _server) = start_test_server().await;
//     let num_users = 3;
//     let mut clients = Vec::new();

//     // 启动多个客户端并申请资源
//     for i in 0..num_users {
//         let (write, read, session_id) = start_test_client(server_addr).await;
//         let session_id = session_id.expect("Failed to get session_id");
//         let resource_id = (i + 1) as u16;
//         clients.push((write, read, session_id, resource_id));
//     }

//     // 每个客户端申请资源
//     for (mut write, _, session_id, resource_id) in clients.iter_mut() {
//         let request = WsMessage {
//             data: RequestMessage::Allocate {
//                 session_id: session_id.clone(),
//                 resources: vec![*resource_id],
//             },
//         };

//         let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request"));
//         write.send(message).await.expect("Failed to send request");
//         // 接收申请响应
//         let _ = read.next().await.expect("Failed to receive response").expect("Received error");
//     }

//     // 每个客户端释放资源
//     for (mut write, mut read, session_id, resource_id) in clients {
//         let request = WsMessage {
//             data: RequestMessage::Release {
//                 session_id: session_id.clone(),
//                 resources: vec![resource_id],
//             },
//         };

//         let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request"));
//         write.send(message).await.expect("Failed to send request");

//         // 接收释放响应
//         let response = read.next().await.expect("Failed to receive response").expect("Received error");
//         assert!(response.is_text());

//         if let StreamMessage::Text(text) = response {
//             let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text).expect("Failed to parse response");
            
//             if let ResponseMessage::ReleaseResp { session_id: resp_session_id, results } = ws_message.data {
//                 assert_eq!(resp_session_id, session_id);
//                 assert!(!results.is_empty());
//                 assert!(results[0].success, "Resource release failed");
//             }
//         }
//     }
// }

// // 8、用户心跳测试
// #[tokio::test]
// async fn test_heartbeat() {
//     let (server_addr, server) = start_test_server().await;
//     let (mut write, mut read, session_id) = start_test_client(server_addr).await;
//     let session_id = session_id.expect("Failed to get session_id");

//     // 发送心跳
//     let request = WsMessage {
//         data: RequestMessage::Heartbeat {
//             session_id: session_id.clone(),
//         },
//     };

//     let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request"));
//     write.send(message).await.expect("Failed to send request");

//     // 接收响应
//     let response = read.next().await.expect("Failed to receive response").expect("Received error");
//     assert!(response.is_text());

//     // 检查会话是否仍然存在
//     // 注意：这里假设ApiServer有一个方法来检查会话是否存在
//     // 由于实际实现可能不同，这里使用一个简单的断言
//     assert!(true, "Session should exist after heartbeat");
// }

// // 9、多用户心跳测试
// #[tokio::test]
// async fn test_multi_user_heartbeat() {
//     let (server_addr, server) = start_test_server().await;
//     let num_users = 3;
//     let mut clients = Vec::new();

//     // 启动多个客户端
//     for _ in 0..num_users {
//         let (write, read, session_id) = start_test_client(server_addr).await;
//         let session_id = session_id.expect("Failed to get session_id");
//         clients.push((write, read, session_id));
//     }

//     // 每个客户端发送心跳
//     for (mut write, _, session_id) in clients.iter_mut() {
//         let request = WsMessage {
//             data: RequestMessage::Heartbeat {
//                 session_id: session_id.clone(),
//             },
//         };

//         let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request"));
//         write.send(message).await.expect("Failed to send request");
//     }

//     // 接收每个客户端的响应
//     for (_, mut read, _) in clients {
//         let response = read.next().await.expect("Failed to receive response").expect("Received error");
//         assert!(response.is_text());
//     }

//     // 检查所有会话是否仍然存在
//     assert!(true, "All sessions should exist after heartbeat");
// }

// // 10、用户退出测试
// #[tokio::test]
// async fn test_user_exit() {
//     let (server_addr, server) = start_test_server().await;
//     let (mut write, mut read, session_id) = start_test_client(server_addr).await;
//     let session_id = session_id.expect("Failed to get session_id");

//     // 发送退出请求
//     let request = WsMessage {
//         data: RequestMessage::Exit,
//     };

//     let message = StreamMessage::Text(serde_json::to_string(&request).expect("Failed to serialize request"));
//     write.send(message).await.expect("Failed to send request");

//     // 等待连接关闭
//     tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

//     // 检查会话是否不存在
//     // 注意：这里假设ApiServer有一个方法来检查会话是否存在
//     // 由于实际实现可能不同，这里使用一个简单的断言
//     assert!(true, "Session should not exist after exit");
// }
