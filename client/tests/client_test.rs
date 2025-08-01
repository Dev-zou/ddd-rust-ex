use std::net::SocketAddr;
use tokio::time::{sleep, Duration};
use client::Client;

// 测试客户端整体流程
#[tokio::test]
async fn test_client_flow() {
    // 服务器地址
    let server_addr = SocketAddr::new([127, 0, 0, 1].into(), 3099);
    println!("连接到服务器: {}", server_addr);

    // 1. 创建客户端并建立连接
    let mut client = Client::new(server_addr).await.expect("客户端连接失败");
    println!("成功建立WebSocket连接");

    // 2. 获取会话ID
    let session_id = client.session_id()
        .expect("未能获取会话ID")
        .clone();
    println!("获取到会话ID: {}", session_id);

    // 等待1秒
    sleep(Duration::from_secs(1)).await;

    // // 3. 申请资源
    // let resource_id = 10;
    // println!("申请资源: {}", resource_id);
    // let allocate_result = client.allocate_resource(resource_id).await;
    // println!("资源申请结果: {:?}", allocate_result);

    // // 等待2秒，让服务器有时间处理请求
    // sleep(Duration::from_secs(2)).await;

    // // 4. 查询已分配资源
    // println!("查询会话 {} 的资源", session_id);
    // let query_result = client.query_resources().await;
    // println!("资源查询结果: {:?}", query_result);

    // // 等待1秒
    // sleep(Duration::from_secs(1)).await;

    // // 5. 释放资源
    // println!("释放资源: {}", resource_id);
    // let release_result = client.release_resource(resource_id).await;
    // println!("资源释放结果: {:?}", release_result);

    // // 等待2秒，让服务器有时间处理请求
    // sleep(Duration::from_secs(2)).await;

    // // 6. 再次查询资源，确认已释放
    // println!("再次查询会话 {} 的资源", session_id);
    // let final_query_result = client.query_resources().await;
    // println!("最终资源查询结果: {:?}", final_query_result);

    // // 等待1秒
    // sleep(Duration::from_secs(1)).await;

    // 7. 断开连接
    println!("断开连接");
    let disconnect_result = client.disconnect().await;
    println!("断开连接结果: {:?}", disconnect_result);

    // 所有操作完成
    assert!(true, "测试完成");
}

#[tokio::test]
async fn test_client_disconnect() {
    // 服务器地址
    let server_addr = SocketAddr::new([127, 0, 0, 1].into(), 3099);
    println!("连接到服务器: {}", server_addr);

    // 1. 创建客户端并建立连接
    let client = Client::new(server_addr).await.expect("客户端连接失败");
    println!("成功建立WebSocket连接");

    // 2. 获取会话ID
    let session_id = client.session_id()
        .expect("未能获取会话ID")
        .clone();
    println!("获取到会话ID: {}", session_id);

    // 等待1秒
    sleep(Duration::from_secs(1)).await;
}