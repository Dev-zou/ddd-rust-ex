use std::net::SocketAddr;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use client::{Client};
use server::api::server::ApiServer;

async fn start_server() -> (SocketAddr, Arc<ApiServer>) {
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

// 测试客户端整体流程
#[tokio::test]
async fn test_client_flow() {
    // 确保服务器已启动
    let (server_addr, _) = start_server().await;

    // 服务器地址
    println!("连接到服务器: {}", server_addr);

    // 1. 创建客户端并建立连接
    let mut client = Client::create_client(server_addr).expect("客户端连接失败");
    println!("成功建立WebSocket连接");
    client.receive_and_set_session_id().await;

    // 2. 获取会话ID
    let session_id = client.session_id();
    println!("获取到会话ID: {:?}", session_id);

    // 等待1秒
    sleep(Duration::from_secs(1)).await;

    // 7. 断开连接
    println!("断开连接");
    let disconnect_result = client.disconnect().await;
    println!("断开连接结果: {:?}", disconnect_result);
    client.shutdown();

    // 所有操作完成
    assert!(true, "测试完成");
}

// 测试用户接入后，申请资源、释放资源
#[tokio::test]
async fn test_resource_allocation_release() {
    // 确保服务器已启动
    let (server_addr, _) = start_server().await;

    // 1. 创建客户端并建立连接
    let mut client = Client::create_client(server_addr).expect("客户端连接失败");
    println!("成功建立WebSocket连接");
    client.receive_and_set_session_id().await;
    let session_id = client.session_id();
    println!("用户 {:?} 接入成功", session_id);

    // 申请资源
    let resources = vec![1,2,3,4,5];
    println!("申请资源: {:?}", resources.clone());
    client.allocate_resource(resources.clone()).await.expect("资源申请失败");

    // 查询资源
    sleep(Duration::from_secs(1)).await;
    let query_resources = client.query_resources().await.expect("资源查询失败");
    println!("当前资源: {:?}", query_resources.clone());
    assert_eq!(resources.clone().sort(), query_resources.clone().sort(), "资源未成功分配");

    // 释放资源
    let release_resources = vec![3,4,5];
    println!("释放资源: {:?}", release_resources.clone());
    let release_result = client.release_resource(release_resources.clone()).await.expect("资源释放失败");
    println!("释放资源结果: {:?}", release_result);

    // 确认资源已释放
    sleep(Duration::from_secs(1)).await;
    let query_resources = client.query_resources().await.expect("资源查询失败");
    println!("释放后资源: {:?}", query_resources);
    let mut sorted_query_resources = query_resources.clone();
    sorted_query_resources.sort();
    assert_eq!(sorted_query_resources, vec![1,2], "资源未成功释放");

    client.disconnect().await.expect("断开连接失败");
    client.shutdown();
}
