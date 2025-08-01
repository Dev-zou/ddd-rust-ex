mod api;
mod app;
mod infra;
mod domain;

use std::net::SocketAddr;
use crate::api::middlewares::logger;
use crate::api::server::ApiServer;
use crate::app::config;

// src/main.rs
#[tokio::main]
async fn main() {
    logger::init();
    
    // 使用默认配置初始化服务器
    let server = ApiServer::init_default();
    
    // 或者使用自定义配置初始化服务器
    // let server = ApiServer::init_custom(100, 200);
    
    let port = 3099;
    tracing::info!("Listening on http://0.0.0.0:{port}");
    
    let router = server.clone().create_router();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    server.spawn_timeout_notify_worker();
    
    server.run(addr, router).await;
}

// 测试用JSON消息示例
/*
请求消息格式示例:
{
    "type": "Allocate",
    "data": {
        "session_id": "143a2d1c-aa64-4474-a9be-04210ebf72d9",
        "resources": [1, 2]
    }
}

{
    "type": "Release",
    "data": {
        "session_id": "143a2d1c-aa64-4474-a9be-04210ebf72d9",
        "resources": [1]
    }
}

{
    "type": "Query",
    "data": {
        "session_id": "143a2d1c-aa64-4474-a9be-04210ebf72d9"
    }
}

{
    "type": "Heartbeat",
    "data": {
        "session_id": "143a2d1c-aa64-4474-a9be-04210ebf72d9"
    }
}

{
    "type": "Exit"
}

响应消息格式示例:
{
    "type": "AllocateResp",
    "data": {
        "session_id": "143a2d1c-aa64-4474-a9be-04210ebf72d9",
        "success_resources": [1],
        "failed_resources": [[2, "Resource conflict"]]
    }
}

{
    "type": "ReleaseResp",
    "data": {
        "session_id": "143a2d1c-aa64-4474-a9be-04210ebf72d9"
    }
}

{
    "type": "QueryResp",
    "data": {
        "session_id": "143a2d1c-aa64-4474-a9be-04210ebf72d9",
        "resources": [1]
    }
}

{
    "type": "HeartbeatResp",
    "data": {
        "session_id": "143a2d1c-aa64-4474-a9be-04210ebf72d9"
    }
}
*/