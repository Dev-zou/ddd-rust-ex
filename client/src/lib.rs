use futures::stream::{SplitSink, SplitStream};
use tokio::task::JoinHandle;
use tracing_subscriber::EnvFilter;
use std::net::SocketAddr;
use tokio_tungstenite::{connect_async, tungstenite, WebSocketStream};
use tungstenite::http::{Method, Request};
use tungstenite::protocol::Message as StreamMessage;
use url::Url;
use tokio::time::{sleep, Duration};
use std::sync::{Arc, Mutex};
// use std::collections::HashMap;
use std::ptr;
use std::os::raw::c_char;
use std::ffi::CStr;
use tokio::sync::mpsc;
use futures::{SinkExt, StreamExt};
use tokio::runtime::Runtime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

// 使用common库中的消息定义
use common::{RequestMessage, ResponseMessage, WsMessage};

// 定义主动推送消息类型
pub enum PushMessage {
    TimeoutResource(Vec<u16>),
    AsyncAllocateResult { success: Vec<u16>, failed: Vec<(u16, String)> },
    AsyncReleaseResult { success: Vec<u16>, failed: Vec<(u16, String)> },
    AllocateResp { success: Vec<u16>, failed: Vec<(u16, String)> },
    ReleaseResp { success: Vec<u16>, failed: Vec<(u16, String)> },
}

pub enum SyncMessage {
    AllocateResp { success: Vec<u16>, failed: Vec<(u16, String)> },
    ReleaseResp { success: Vec<u16>, failed: Vec<(u16, String)> },
    QueryResp { resources: Vec<u16> },
    ExitResp,
}

pub struct MessageRx {
    // 用于接收消息的通道
    session_id_rx: Arc<Mutex<mpsc::Receiver<String>>>,
    timeout_message_rx: Arc<Mutex<mpsc::Receiver<PushMessage>>>,
    alloc_rx: Arc<Mutex<mpsc::Receiver<SyncMessage>>>,
    release_rx: Arc<Mutex<mpsc::Receiver<SyncMessage>>>,
    query_rx: Arc<Mutex<mpsc::Receiver<SyncMessage>>>,
    exit_rx: Arc<Mutex<mpsc::Receiver<SyncMessage>>>,
}

pub struct MessageTx {
    // 用于接收消息的通道
    session_id_tx: mpsc::Sender<String>,
    timeout_message_tx: mpsc::Sender<PushMessage>,
    alloc_tx: mpsc::Sender<SyncMessage>,
    release_tx: mpsc::Sender<SyncMessage>,
    query_tx: mpsc::Sender<SyncMessage>,
    exit_tx: mpsc::Sender<SyncMessage>,
}

// 子任务类型别名（简化代码）
type SendTaskHandle = JoinHandle<()>;
type RecvTaskHandle = JoinHandle<()>;
// 客户端内部状态
struct ClientInner {
    session_id: Option<String>,
    // 发送端
    requeset_tx: Option<mpsc::Sender<StreamMessage>>,
    // 运行时
    runtime: Arc<Runtime>,
    // 消息接收端
    resp_rx: Arc<MessageRx>,

}

// 客户端结构体
pub struct MessageTask {
    send_task: Option<SendTaskHandle>,
    recv_task: Option<RecvTaskHandle>,
}


// 客户端结构体
pub struct Client {
    // 保存运行时，确保后台任务持续运行
    inner: ClientInner,
    message_task: Arc<Mutex<MessageTask>>,
}

pub fn logger_init() {
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else( |_| EnvFilter::new("info")))
        .with(
            tracing_subscriber::fmt::layer()
                .with_file(true)
                .with_line_number(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_target(false)
        )
        .init();
}

pub async fn get_websocket_request(addr: SocketAddr) -> Result<Request<()>, String> {
    // 构建WebSocket URL
    let ws_url = format!("ws://{}:{}/ws", addr.ip(), addr.port());
    let url = match Url::parse(&ws_url) {
        Ok(url) => url,
        Err(e) => { return Err(e.to_string()); }
    };

    // 创建WebSocket请求
    let request = match Request::builder()
        .method(Method::GET)
        .uri(url.as_str())
        .header("Host", format!("{}:{}", addr.ip(), addr.port()))
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
        .header("Sec-WebSocket-Version", "13")
        .body(())
    {
        Ok(request) => request,
        Err(e) => { return Err(e.to_string()); }
    };
    Ok(request)
}

// 处理服务器推送的消息
async fn receive_task(
    mut read: SplitStream<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>,
    msg_tx: MessageTx,
) {
    while let Some(Ok(message)) = read.next().await {
        if let StreamMessage::Text(text) = message {
            if let Ok(ws_message) = serde_json::from_str::<WsMessage<ResponseMessage>>(&text) {
                match ws_message.data {
                    ResponseMessage::SessionIdResp { session_id } => {
                        // 发送超时资源信息到消息通道
                        tracing::info!("收到session_id: {:?}", session_id);
                        let _ = msg_tx.session_id_tx.send(session_id).await;
                    },
                    ResponseMessage::TimeoutResourceInfo { session_id: _, timeout_resources } => {
                        // 发送超时资源信息到消息通道
                        tracing::info!("收到超时资源信息: {:?}", timeout_resources);
                        let _ = msg_tx.timeout_message_tx.send(PushMessage::TimeoutResource(timeout_resources)).await;
                    },
                    ResponseMessage::AllocateResp { session_id: _, success_resources, failed_resources, .. } => {
                        // 收到申请资源响应
                        tracing::info!("收到申请资源成功: {:?}", success_resources);
                        let result = SyncMessage::AllocateResp {
                            success: success_resources.clone(),
                            failed: failed_resources,
                        };
                        let _ = msg_tx.alloc_tx.send(result).await;
                    },
                    ResponseMessage::ReleaseResp { session_id: _, success_resources, failed_resources, .. } => {
                        // 收到释放资源响应
                        tracing::info!("收到释放资源成功: {:?}", success_resources);
                        let result = SyncMessage::ReleaseResp {
                            success: success_resources.clone(),
                            failed: failed_resources,
                        };
                        let _ = msg_tx.release_tx.send(result).await;
                    },
                    ResponseMessage::QueryResp { session_id: _, resources, .. } => {
                        // 收到查询资源响应
                        tracing::info!("收到查询资源成功: {:?}", resources);
                        let result = SyncMessage::QueryResp {
                            resources,
                        };
                        let _ = msg_tx.query_tx.send(result).await;
                    },
                    ResponseMessage::ExitResp { session_id: _ } => {
                        // 收到退出响应
                        tracing::info!("收到退出响应");
                        let result = SyncMessage::ExitResp;
                        let _ = msg_tx.exit_tx.send(result).await;
                    },
                    _ => {}
                }
            }
        }
    }
}

// 发送消息的代理任务
async fn send_proxy_task(
    mut rx: tokio::sync::mpsc::Receiver<StreamMessage>,
    mut write: SplitSink<WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, StreamMessage>,
) {
    while let Some(msg) = rx.recv().await {
        if let Err(e) = write.send(msg).await {
            tracing::error!("发送消息失败: {}", e);
            break;
        }
    }
}


impl Client {
    // 创建新客户端
    pub fn create_client(addr: SocketAddr) -> Result<Self, String> {
        // 创建运行时
        let rt = match Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("创建运行时失败: {}", e);
                return Err(e.to_string());
            }
        };

        // 创建通道用于发送消息
        let (tx, rx) = tokio::sync::mpsc::channel(100);

        let (session_id_tx, session_id_rx) = mpsc::channel(100);
        let session_id_rx_arc = Arc::new(Mutex::new(session_id_rx));

        let (timeout_message_tx, timeout_message_rx) = mpsc::channel(100);
        let timeout_message_rx_arc = Arc::new(Mutex::new(timeout_message_rx));
        // 创建消息通道
        let (alloc_tx, alloc_rx) = mpsc::channel(100);
        let alloc_rx_arc = Arc::new(Mutex::new(alloc_rx));
        // 创建消息通道
        let (release_tx, release_rx) = mpsc::channel(100);
        let release_rx_arc = Arc::new(Mutex::new(release_rx));
        // 创建消息通道
        let (query_tx, query_rx) = mpsc::channel(100);
        let query_rx_arc = Arc::new(Mutex::new(query_rx));
        // 创建消息通道
        let (exit_tx, exit_rx) = mpsc::channel(100);
        let exit_rx_arc = Arc::new(Mutex::new(exit_rx));

        // 创建客户端内部状态
        let inner = ClientInner {
            session_id: None,
            requeset_tx: Some(tx),
            runtime: Arc::new(rt),
            resp_rx: Arc::new(MessageRx {
                session_id_rx: session_id_rx_arc,
                timeout_message_rx: timeout_message_rx_arc,
                alloc_rx: alloc_rx_arc,
                release_rx: release_rx_arc,
                query_rx: query_rx_arc,
                exit_rx: exit_rx_arc,
            }),
        };

        let msg_tx = MessageTx {
            session_id_tx,
            timeout_message_tx,
            alloc_tx,
            release_tx,
            query_tx,
            exit_tx,
        };
        let message_task = Arc::new(Mutex::new(MessageTask {
            send_task: None,
            recv_task: None,
        }));
        let message_task_clone = message_task.clone();

        // 关键优化：用代码块限制外部锁的持有时间 
        let rt = inner.runtime.clone();
        let rt_clone = rt.clone();
        rt_clone.spawn(async move {
            // 建立WebSocket连接
            let (ws_stream, response) = match connect_async(get_websocket_request(addr).await.unwrap()).await {
                Ok(res) => res,
                Err(e) => {
                    tracing::error!("连接失败: {}", e);
                    return;
                }
            };

            tracing::info!("连接成功，状态码: {}", response.status());
            
            // 拆分流
            let (write_sink, read_stream) = ws_stream.split();
        
            // 启动发送代理任务
            let rt =  rt.clone();
            // 启动发送代理任务并保存句柄
            let send_handle = rt.spawn(send_proxy_task(rx, write_sink));

            // 启动接收任务并保存句柄
            let recv_handle = rt.spawn(receive_task(read_stream, msg_tx));
            // 4. 单独的代码块：仅在保存句柄时短暂锁定，完成后立即释放
            {
                // 只在赋值时锁定，操作完成后自动释放锁（通过代码块作用域）
                let mut message_task_clone = message_task_clone.lock().unwrap();
                message_task_clone.send_task = Some(send_handle);
                message_task_clone.recv_task = Some(recv_handle);
            } // 此处锁自动释放
        });
        Ok(Client { inner, message_task })
    }

    // 申请资源
    pub async fn receive_and_set_session_id(&mut self) {
        loop {
            match self.inner.resp_rx.session_id_rx.lock().unwrap().recv().await {
                Some(message) => {
                    self.inner.session_id = Some(message.clone());
                    break;
                },
                None => {
                    tracing::warn!("Failed to receive session_id response: channel closed");
                    return;
                }
            }
        }
    }

    // 申请资源
    pub async fn allocate_resource(&mut self, resources: Vec<u16>) -> Result<Vec<u16>, Box<dyn std::error::Error>> {
        let request = RequestMessage::Allocate {
            session_id: self.inner.session_id.clone().unwrap_or_default(),
            resources: resources,
        };
        let ws_message = WsMessage {
            id: None,
            data: request,
        };
        let json_str = serde_json::to_string(&ws_message)?;
        self.inner.requeset_tx.as_mut().unwrap().send(StreamMessage::Text(json_str.into())).await?;
        loop {
            if let Some(message) = self.inner.resp_rx.alloc_rx.lock().unwrap().recv().await {
                match message {
                    SyncMessage::AllocateResp { success, failed: _ } => {
                        return Ok(success);
                    },
                    _ => {
                        tracing::warn!("Failed to receive release response");
                        return Err("Failed to receive release response".into());
                    }
                }
            }
        }
    }

    // 释放资源
    pub async fn release_resource(&mut self, resources: Vec<u16>) -> Result<Vec<u16>, Box<dyn std::error::Error>> {
        let request = RequestMessage::Release {
            session_id: self.inner.session_id.clone().unwrap_or_default(),
            resources: resources.clone(),
        };
        let ws_message = WsMessage {
            id: None,
            data: request,
        };
        tracing::info!("发送释放资源请求: {:?}", resources);
        let json_str = serde_json::to_string(&ws_message)?;
        self.inner.requeset_tx.as_mut().unwrap().send(StreamMessage::Text(json_str.into())).await?;
        loop {
            if let Some(message) = self.inner.resp_rx.release_rx.lock().unwrap().recv().await {
                match message {
                    SyncMessage::ReleaseResp { success, failed:_ } => {
                        return Ok(success);
                    },
                    _ => {
                        tracing::warn!("Failed to receive release response");
                        return Err("Failed to receive release response".into());
                    }
                }
            }
        }
    }

    // 查询资源
    pub async fn query_resources(&mut self) -> Result<Vec<u16>, Box<dyn std::error::Error>> {
        let request = RequestMessage::Query {
            session_id: self.inner.session_id.clone().unwrap_or_default(),
        };
        let ws_message = WsMessage {
            id: None,
            data: request,
        };
        let json_str = serde_json::to_string(&ws_message)?;
        self.inner.requeset_tx.as_mut().unwrap().send(StreamMessage::Text(json_str.into())).await?;

        loop {
            if let Some(message) = self.inner.resp_rx.query_rx.lock().unwrap().recv().await {
                match message {
                    SyncMessage::QueryResp { resources } => {
                        return Ok(resources);
                    },
                    _ => {
                        tracing::warn!("Failed to receive query response");
                        return Err("Failed to receive query response".into());
                    }
                }
            }
        }
    }

    // 发送心跳
    pub async fn heartbeat(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let request = RequestMessage::Heartbeat {
            session_id: self.inner.session_id.clone().unwrap_or_default(),
        };
        let ws_message = WsMessage {
            id: None,
            data: request,
        };
        let json_str = serde_json::to_string(&ws_message)?;
        self.inner.requeset_tx.as_mut().unwrap().send(StreamMessage::Text(json_str.into())).await?;
        Ok(())
    }


    // // 异步申请资源
    // pub async fn async_allocate_resource(&mut self, resource_id: u16) -> Result<String, Box<dyn std::error::Error>> {
    //     if let Some(session_id) = &self.session_id {
    //         if let Some(ref mut write) = self.write {
    //             // 生成请求ID
    //             let request_id = Uuid::new_v4().to_string();
    //             let request = RequestMessage::AsyncAllocate {
    //                 session_id: session_id.clone(),
    //                 resources: vec![resource_id],
    //             };
    //             let ws_message = WsMessage {
    //                 id: Some(request_id.clone()),
    //                 data: request,
    //             };
    //             let json_str = serde_json::to_string(&ws_message)?;
    //             write.send(StreamMessage::Text(json_str.into())).await?;
    //             Ok(request_id)
    //         } else {
    //             Err("WebSocket connection closed".into())
    //         }
    //     } else {
    //         Err("No session_id available".into())
    //     }
    // }

    // // 异步释放资源
    // pub async fn async_release_resource(&mut self, resource_id: u16) -> Result<String, Box<dyn std::error::Error>> {
    //     if let Some(session_id) = &self.session_id {
    //         if let Some(ref mut write) = self.write {
    //             // 生成请求ID
    //             let request_id = Uuid::new_v4().to_string();
    //             let request = RequestMessage::AsyncRelease {
    //                 session_id: session_id.clone(),
    //                 resources: vec![resource_id],
    //             };
    //             let ws_message = WsMessage {
    //                 id: Some(request_id.clone()),
    //                 data: request,
    //             };
    //             let json_str = serde_json::to_string(&ws_message)?;
    //             write.send(StreamMessage::Text(json_str.into())).await?;
    //             Ok(request_id)
    //         } else {
    //             Err("WebSocket connection closed".into())
    //         }
    //     } else {
    //         Err("No session_id available".into())
    //     }
    // }

    // 查询主动推送消息
    // pub async fn get_push_message(&self) -> Option<PushMessage> {
    //     let mut rx = self.inner.resp_rx.timeout_message_rx.lock().unwrap();
    //     rx.recv().await.ok()
    // }

    // // 根据请求ID查询异步操作结果
    // pub fn get_async_result(&self, request_id: &str) -> Option<PushMessage> {
    //     let mut results = self.async_results.lock().unwrap();
    //     results.remove(request_id)
    // }

    // 断开连接
    pub async fn disconnect(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // 发送Exit消息并等待响应
        tracing::info!("断开连接 {:?}", self.inner.session_id);
        let request = RequestMessage::Exit {
            session_id: self.inner.session_id.clone().unwrap_or_default(),
        };
        let ws_message = WsMessage {
            id: None,
            data: request,
        };
        let json_str = serde_json::to_string(&ws_message)?;

        if let Err(e) = self.inner.requeset_tx.as_mut().unwrap().send(StreamMessage::Text(json_str.into())).await {
            tracing::error!("断开连接发送消息失败: {:?}", e);
        }
        

        // 设置超时，最多等待3秒
        let timeout = sleep(Duration::from_secs(3));
        tokio::pin!(timeout);

        loop {
            let mut exit_rx_lock = self.inner.resp_rx.exit_rx.lock().unwrap();
            tokio::select! {
                _ = &mut timeout => {
                    tracing::warn!("Timeout waiting for ExitResp");
                    break;
                },
                message = exit_rx_lock.recv() => {
                    match message {
                        Some(_) => {
                            tracing::info!("Received ExitResp from server");
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

        tracing::info!("WebSocket connection closed gracefully");
        Ok(())
    }

    pub fn shutdown(mut self) {
        self.inner.requeset_tx.take(); // 取出并 drop 发送器，通道关闭
        {
            // 只在赋值时锁定，操作完成后自动释放锁（通过代码块作用域）
            let mut message_task_lock = self.message_task.lock().unwrap();
            message_task_lock.send_task.take();
            message_task_lock.recv_task.take();
        } // 此处锁自动释放
        match Arc::try_unwrap (self.inner.runtime) {
            Ok (rt) => {
                // rt.shutdown_timeout(Duration::from_secs(2));
                rt.shutdown_background();
            }
            Err (_arc) => {
                tracing::warn!("警告：运行时仍被其他引用持有，强制关闭{:?}", _arc);
            }
        }
    }
    pub fn session_id(&self) -> Option<String> {
        self.inner.session_id.clone()
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
    logger_init();
    let ip_str = unsafe { CStr::from_ptr(ip) }.to_str().unwrap_or("127.0.0.1");
    let addr = SocketAddr::new(ip_str.parse().unwrap_or([127, 0, 0, 1].into()), port);

    let rt = tokio::runtime::Runtime::new().unwrap();

    // 直接调用同步的Client::new方法
    if let Ok(client) = Client::create_client(addr) {
        let arc = std::sync::Arc::new(std::sync::Mutex::new(client));
        let client_copy = arc.clone();
        rt.block_on(async move {
            client_copy.lock().unwrap().receive_and_set_session_id().await;
        });
        let ptr = Box::into_raw(Box::new(arc));
        return ClientHandle { ptr };
    }
    tracing::error!("client_init fail");
    return ClientHandle { ptr: ptr::null_mut() };
}

// 申请资源
#[no_mangle]
pub extern "C" fn client_allocate_resource(
    handle: ClientHandle, 
    resource_ids: *const u16, 
    count: usize,
    output_buffer: *mut u16,
    actual_count: *mut usize
) -> bool {
    if handle.ptr.is_null() || resource_ids.is_null() || count == 0 || output_buffer.is_null() || actual_count.is_null() {
        return false;
    }

    let arc = unsafe { &*handle.ptr };
    let mut client = arc.lock().unwrap();

    // 将C数组转换为Rust向量
    let resources = unsafe { std::slice::from_raw_parts(resource_ids, count) };
    let resources_vec: Vec<u16> = resources.to_vec();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async move {
        client.allocate_resource(resources_vec).await
    });

    match result {
        Ok(success_vec) => {
            let len = success_vec.len();
            let copy_len = len.min(count); // 使用count作为缓冲区大小限制
            
            // 将结果复制到输出缓冲区
            unsafe {
                std::ptr::copy_nonoverlapping(success_vec.as_ptr(), output_buffer, copy_len);
                *actual_count = len; // 设置实际元素数量
            }
            
            true
        },
        Err(_) => false
    }
}

// 释放资源
#[no_mangle]
pub extern "C" fn client_release_resource(
    handle: ClientHandle, 
    resource_ids: *const u16, 
    count: usize,
    output_buffer: *mut u16,
    actual_count: *mut usize
) -> bool {
    if handle.ptr.is_null() || resource_ids.is_null() || count == 0 || output_buffer.is_null() || actual_count.is_null() {
        return false;
    }

    let arc = unsafe { &*handle.ptr };
    let mut client = arc.lock().unwrap();

    // 将C数组转换为Rust向量
    let resources = unsafe { std::slice::from_raw_parts(resource_ids, count) };
    let resources_vec: Vec<u16> = resources.to_vec();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async move {
        client.release_resource(resources_vec).await
    });

    match result {
        Ok(success_vec) => {
            let len = success_vec.len();
            let copy_len = len.min(count); // 使用count作为缓冲区大小限制
            
            // 将结果复制到输出缓冲区
            unsafe {
                std::ptr::copy_nonoverlapping(success_vec.as_ptr(), output_buffer, copy_len);
                *actual_count = len; // 设置实际元素数量
            }
            
            true
        },
        Err(_) => false
    }
}

// // 异步申请资源
// #[no_mangle]
// pub extern "C" fn client_async_allocate_resource(handle: ClientHandle, resource_id: u16, request_id: *mut c_char, buffer_size: usize) -> bool {
//     if handle.ptr.is_null() || request_id.is_null() {
//         return false;
//     }

//     let arc = unsafe { &*handle.ptr };
//     let mut client = arc.lock().unwrap();

//     let rt = tokio::runtime::Runtime::new().unwrap();
//     let result = rt.block_on(async move {
//         client.async_allocate_resource(resource_id).await
//     });

//     match result {
//         Ok(id) => {
//             let c_str = std::ffi::CString::new(id).unwrap_or_default();
//             let bytes = c_str.as_bytes_with_nul();
//             let len = bytes.len().min(buffer_size);
//             unsafe {
//                 std::ptr::copy_nonoverlapping(bytes.as_ptr(), request_id as *mut u8, len);
//             }
//             true
//         },
//         Err(_) => false
//     }
// }

// // 异步释放资源
// #[no_mangle]
// pub extern "C" fn client_async_release_resource(handle: ClientHandle, resource_id: u16, request_id: *mut c_char, buffer_size: usize) -> bool {
//     if handle.ptr.is_null() || request_id.is_null() {
//         return false;
//     }

//     let arc = unsafe { &*handle.ptr };
//     let mut client = arc.lock().unwrap();

//     let rt = tokio::runtime::Runtime::new().unwrap();
//     let result = rt.block_on(async move {
//         client.async_release_resource(resource_id).await
//     });

//     match result {
//         Ok(id) => {
//             let c_str = std::ffi::CString::new(id).unwrap_or_default();
//             let bytes = c_str.as_bytes_with_nul();
//             let len = bytes.len().min(buffer_size);
//             unsafe {
//                 std::ptr::copy_nonoverlapping(bytes.as_ptr(), request_id as *mut u8, len);
//             }
//             true
//         },
//         Err(_) => false
//     }
// }

// // 查询主动推送消息
// // 注意：这个函数在C端需要单独的线程来调用，避免阻塞主线程
// #[no_mangle]
// pub extern "C" fn client_get_push_message(handle: ClientHandle) -> *mut c_char {
//     if handle.ptr.is_null() {
//         return ptr::null_mut();
//     }

//     let arc = unsafe { &*handle.ptr };
//     let client = arc.lock().unwrap();

//     let rt = tokio::runtime::Runtime::new().unwrap();
//     let result = rt.block_on(async move {
//         client.get_push_message().await
//     });

//     match result {
//         Some(message) => {
//             let message_str = match message {
//                 PushMessage::TimeoutResource(resources) => {
//                     format!("TimeoutResource:{:?}", resources)
//                 },
//                 PushMessage::AsyncAllocateResult { success, failed } => {
//                     format!("AsyncAllocateResult:success={:?},failed={:?}", success, failed)
//                 },
//                 PushMessage::AsyncReleaseResult { success, failed } => {
//                     format!("AsyncReleaseResult:success={:?},failed={:?}", success, failed)
//                 },
//             };
//             let c_str = std::ffi::CString::new(message_str).unwrap_or_default();
//             c_str.into_raw()
//         },
//         None => ptr::null_mut()
//     }
// }

// // 根据请求ID查询异步操作结果
// #[no_mangle]
// pub extern "C" fn client_get_async_result(handle: ClientHandle, request_id: *const c_char) -> *mut c_char {
//     if handle.ptr.is_null() || request_id.is_null() {
//         return ptr::null_mut();
//     }

//     let request_id_str = unsafe { CStr::from_ptr(request_id) }.to_str().unwrap_or("");
//     let arc = unsafe { &*handle.ptr };
//     let client = arc.lock().unwrap();

//     match client.get_async_result(request_id_str) {
//         Some(result) => {
//             let result_str = match result {
//                 PushMessage::TimeoutResource(resources) => {
//                     format!("TimeoutResource:{:?}", resources)
//                 },
//                 PushMessage::AsyncAllocateResult { success, failed } => {
//                     format!("AsyncAllocateResult:success={:?},failed={:?}", success, failed)
//                 },
//                 PushMessage::AsyncReleaseResult { success, failed } => {
//                     format!("AsyncReleaseResult:success={:?},failed={:?}", success, failed)
//                 },
//             };
//             let c_str = std::ffi::CString::new(result_str).unwrap_or_default();
//             c_str.into_raw()
//         },
//         None => ptr::null_mut()
//     }
// }

// 断开连接
#[no_mangle]
pub extern "C" fn client_disconnect(handle: ClientHandle) -> bool {
    if handle.ptr.is_null() {
        tracing::warn!("client_disconnect handle is null");
        return false;
    }

    let arc = unsafe { &*handle.ptr };
    let mut client = arc.lock().unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let result = rt.block_on(async move {
        client.disconnect().await.is_ok()
    });
    tracing::info!("client_disconnect result: {:?}", result);
    
    result
}

// 释放客户端
#[no_mangle]
pub extern "C" fn client_shutdown(handle: ClientHandle) {
    if handle.ptr.is_null() {
        return;
    }

    // 1. 将原始指针转换为Box<Arc<Mutex<Client>>>，获取Arc的所有权
    // 注意：这里必须用Box::from_raw，因为指针应该是之前用Box::into_raw创建的
    let arc_box = unsafe { Box::from_raw(handle.ptr) };
    let arc = *arc_box; // 取出Arc（此时arc_box被消费）

    // 2. 尝试解开Arc（确保没有其他引用，否则会返回Err）
    let mutex = match Arc::try_unwrap(arc) {
        Ok(mutex) => mutex,
        Err(_) => {
            // 还有其他线程持有引用，无法获取所有权，此处根据需求处理（如返回错误）
            return;
        }
    };

    // 3. 消费锁，获取Client的所有权
    let client = match mutex.into_inner() {
        Ok(client) => client, // 成功获取Client所有权
        Err(e) => {
            // 处理锁被污染的情况（如持有锁的线程panic）
            e.into_inner() // 即使锁被污染，也可以强行取出内部数据
        }
    };

    // 5. 调用需要所有权的shutdown方法（消费client）
    client.shutdown();

    // unsafe {
    //     let _ = Box::from_raw(handle.ptr);
    // }
}
