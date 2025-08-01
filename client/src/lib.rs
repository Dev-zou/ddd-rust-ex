use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio_tungstenite::{connect_async, tungstenite, WebSocketStream};
use tungstenite::http::{Method, Request};
use tungstenite::protocol::Message as StreamMessage;
use url::Url;
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};
// 导入tracing库
use tracing::{error, info, warn};
// use std::sync::Arc;
use std::ptr;
use std::os::raw::c_char;
use std::ffi::CStr;

// 使用common库中的消息定义
use common::{RequestMessage, ResponseMessage, WsMessage};

// 客户端结构体
pub struct Client {
    session_id: Option<String>,
    write: Option<SplitSink<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, StreamMessage>>,
    read: Option<SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>>,
}

impl Client {
    // 获取会话ID
    pub fn session_id(&self) -> Option<&String> {
        self.session_id.as_ref()
    }

    // 创建新客户端
    pub async fn new(addr: SocketAddr) -> Result<Self, Box<dyn std::error::Error>> {
        // 构建WebSocket URL
        let ws_url = format!("ws://{}:{}/ws", addr.ip(), addr.port());
        let url = Url::parse(&ws_url)?;

        // 创建WebSocket请求
        let request = Request::builder()
            .method(Method::GET)
            .uri(url.as_str())
            .header("Host", format!("{}:{}", addr.ip(), addr.port()))
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
            .header("Sec-WebSocket-Version", "13")
            .body(())?;

        // 建立连接
        let (ws_stream, response) = connect_async(request).await?;

        if response.status() != 101 {
            return Err(format!("WebSocket handshake failed with status: {}", response.status()).into());
        }

        let (write, mut read) = ws_stream.split();

        // 接收session_id
        let session_id = if let Some(Ok(message)) = read.next().await {
            if let StreamMessage::Text(text) = message {
                let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text)?;
                if let ResponseMessage::SessionIdResp { session_id } = ws_message.data {
                    Some(session_id)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            session_id,
            write: Some(write),
            read: Some(read),
        })
    }

    // 申请资源
    pub async fn allocate_resource(&mut self, resource_id: u16) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(session_id) = &self.session_id {
            if let Some(ref mut write) = self.write {
                let request = RequestMessage::Allocate {
                    session_id: session_id.clone(),
                    resources: vec![resource_id],
                };
                let ws_message = WsMessage {
                    id: None,
                    data: request,
                };
                let json_str = serde_json::to_string(&ws_message)?;
                write.send(StreamMessage::Text(json_str.into())).await?;
                Ok(())
            } else {
                Err("WebSocket connection closed".into())
            }
        } else {
            Err("No session_id available".into())
        }
    }

    // 释放资源
    pub async fn release_resource(&mut self, resource_id: u16) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(session_id) = &self.session_id {
            if let Some(ref mut write) = self.write {
                let request = RequestMessage::Release {
                    session_id: session_id.clone(),
                    resources: vec![resource_id],
                };
                let ws_message = WsMessage {
                    id: None,
                    data: request,
                };
                let json_str = serde_json::to_string(&ws_message)?;
                write.send(StreamMessage::Text(json_str.into())).await?;
                Ok(())
            } else {
                Err("WebSocket connection closed".into())
            }
        } else {
            Err("No session_id available".into())
        }
    }

    // 查询资源
    pub async fn query_resources(&mut self) -> Result<Vec<u16>, Box<dyn std::error::Error>> {
        if let Some(session_id) = &self.session_id {
            if let Some(ref mut write) = self.write {
                let request = RequestMessage::Query {
                    session_id: session_id.clone(),
                };
                let ws_message = WsMessage {
                    id: None,
                    data: request,
                };
                let json_str = serde_json::to_string(&ws_message)?;
                write.send(StreamMessage::Text(json_str.into())).await?;

                // 等待响应
                if let Some(mut read) = self.read.as_mut() {
                    if let Some(Ok(message)) = read.next().await {
                        if let StreamMessage::Text(text) = message {
                            let ws_message: WsMessage<ResponseMessage> = serde_json::from_str(&text)?;
                            if let ResponseMessage::QueryResp { resources, .. } = ws_message.data {
                                return Ok(resources);
                            }
                        }
                    }
                }
                Err("Failed to receive query response".into())
            } else {
                Err("WebSocket connection closed".into())
            }
        } else {
            Err("No session_id available".into())
        }
    }

    // 发送心跳
    pub async fn heartbeat(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(session_id) = &self.session_id {
            if let Some(ref mut write) = self.write {
                let request = RequestMessage::Heartbeat {
                    session_id: session_id.clone(),
                };
                let ws_message = WsMessage {
                    id: None,
                    data: request,
                };
                let json_str = serde_json::to_string(&ws_message)?;
                write.send(StreamMessage::Text(json_str.into())).await?;
                Ok(())
            } else {
                Err("WebSocket connection closed".into())
            }
        } else {
            Err("No session_id available".into())
        }
    }

    // 断开连接
    pub async fn disconnect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 发送Exit消息并等待响应
        if let Some(session_id) = &self.session_id {
            if let Some(ref mut write) = self.write {
                let request = RequestMessage::Exit {
                    session_id: session_id.clone(),
                };
                let ws_message = WsMessage {
                    id: None,
                    data: request,
                };
                let json_str = serde_json::to_string(&ws_message)?;
                write.send(StreamMessage::Text(json_str.into())).await?;
            }

            // 等待服务端响应ExitResp
            if let Some(read) = self.read.as_mut() {
                // 设置超时，最多等待3秒
                let timeout = sleep(Duration::from_secs(3));
                tokio::pin!(timeout);

                loop {
                    tokio::select! {
                        _ = &mut timeout => {
                            tracing::warn!("Timeout waiting for ExitResp");
                            break;
                        },
                        message = read.next() => {
                            match message {
                                Some(Ok(StreamMessage::Text(text))) => {
                                    if let Ok(ws_message) = serde_json::from_str::<WsMessage<ResponseMessage>>(&text) {
                                        if let ResponseMessage::ExitResp { session_id: resp_session_id } = ws_message.data {
                                            if resp_session_id == *session_id {
                                                tracing::info!("Received ExitResp from server");
                                                break;
                                            }
                                        }
                                    }
                                },
                                Some(Ok(_)) => {}
                                Some(Err(e)) => {
                                    tracing::error!("Error reading message: {:?}", e);
                                    break;
                                },
                                None => {
                                    tracing::warn!("Connection closed before receiving ExitResp");
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        // 优雅关闭连接
        if let Some(mut write) = self.write.take() {
            // 确保所有数据都已发送
            write.flush().await?;
            // 发送关闭帧
            write.close().await?;
            tracing::info!("WebSocket connection closed gracefully");
        }
        // 释放读取流
        self.read.take();

        Ok(())
    }
}

// C SDK 实现
// 定义C兼容的客户端句柄
#[repr(C)]
pub struct ClientHandle {
    // 内部使用Arc<Mutex<Client>>来保证线程安全
    ptr: *mut std::sync::Arc<std::sync::Mutex<Client>>,
}

// 初始化客户端
#[no_mangle]
pub extern "C" fn client_init(ip: *const c_char, port: u16) -> ClientHandle {
    let ip_str = unsafe { CStr::from_ptr(ip) }.to_str().unwrap_or("127.0.0.1");
    let addr = SocketAddr::new(ip_str.parse().unwrap_or([127, 0, 0, 1].into()), port);

    // 在异步运行时中创建客户端
    let rt = tokio::runtime::Runtime::new().unwrap();
    let client = rt.block_on(async move {
        Client::new(addr).await
    });

    match client {
        Ok(c) => {
            let arc = std::sync::Arc::new(std::sync::Mutex::new(c));
            let ptr = Box::into_raw(Box::new(arc));
            ClientHandle { ptr }
        },
        Err(_) => {
            ClientHandle { ptr: ptr::null_mut() }
        }
    }
}

// 申请资源
#[no_mangle]
pub extern "C" fn client_allocate_resource(handle: ClientHandle, resource_id: u16) -> bool {
    if handle.ptr.is_null() {
        return false;
    }

    let arc = unsafe { &*handle.ptr };
    let mut client = arc.lock().unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        client.allocate_resource(resource_id).await.is_ok()
    })
}

// 释放资源
#[no_mangle]
pub extern "C" fn client_release_resource(handle: ClientHandle, resource_id: u16) -> bool {
    if handle.ptr.is_null() {
        return false;
    }

    let arc = unsafe { &*handle.ptr };
    let mut client = arc.lock().unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        client.release_resource(resource_id).await.is_ok()
    })
}

// 断开连接
#[no_mangle]
pub extern "C" fn client_disconnect(handle: ClientHandle) -> bool {
    if handle.ptr.is_null() {
        return false;
    }

    let arc = unsafe { &*handle.ptr };
    let mut client = arc.lock().unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async move {
        client.disconnect().await.is_ok()
    });

    // 释放资源
    unsafe {
        let _ = Box::from_raw(handle.ptr);
    }

    result
}

// 主函数已移除，因为这是一个库文件
// 若要运行示例，请参考C示例代码或创建单独的二进制文件

// 编译时需要添加的依赖提示
// 在Cargo.toml中添加:
// [dependencies]
// tokio = { version = "1.0", features = ["full"] }
// tokio-tungstenite = "0.18"
// tungstenite = "0.18"
// futures = "0.3"
// serde = { version = "1.0", features = ["derive"] }
// serde_json = "1.0"
// url = "2.3"
// tracing = "0.1"