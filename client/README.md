# Rust WebSocket客户端与C SDK实现

## 概述

这个项目提供了一个基于Rust的WebSocket客户端，并通过FFI（外部函数接口）导出C兼容的SDK，使得C语言程序可以调用Rust实现的功能。

## 项目结构

```
client/
├── src/
│   └── main.rs       # Rust客户端实现，包含C SDK导出
├── Cargo.toml        # 项目依赖配置
├── example.c         # C语言示例程序
└── README.md         # 本说明文件
```

## 功能特点

1. **Rust客户端功能**:
   - 与WebSocket服务器建立连接
   - 接收服务器推送的session_id
   - 申请资源
   - 释放资源
   - 断开连接

2. **C SDK接口**:
   - `client_init`: 初始化客户端并连接服务器
   - `client_allocate_resource`: 申请资源
   - `client_release_resource`: 释放资源
   - `client_disconnect`: 断开连接并释放资源

## 编译说明

### 编译Rust客户端与SDK

1. 确保已安装Rust环境
2. 进入client目录
3. 执行编译命令:

```bash
cd /Users/zouxinyao/project/01-RUST/ws-ex/client
cargo build --release
```

编译完成后，会在`target/release`目录下生成:
- `libclient.a`: 静态库
- `libclient.dylib`: 动态库(在macOS上)

### 编译C示例程序

```bash
gcc -o example example.c -Ltarget/release -lclient -Wl,-rpath,target/release
```

## 使用说明

### C SDK接口说明

#### 1. 初始化客户端

```c
ClientHandle client_init(const char* ip, unsigned short port);
```
- 功能: 初始化客户端并连接到指定的WebSocket服务器
- 参数:
  - `ip`: 服务器IP地址
  - `port`: 服务器端口
- 返回值: 客户端句柄，若连接失败则句柄的ptr字段为NULL

#### 2. 申请资源

```c
int client_allocate_resource(ClientHandle handle, unsigned short resource_id);
```
- 功能: 使用指定的资源ID申请资源
- 参数:
  - `handle`: 客户端句柄
  - `resource_id`: 资源ID
- 返回值: 成功返回1，失败返回0

#### 3. 释放资源

```c
int client_release_resource(ClientHandle handle, unsigned short resource_id);
```
- 功能: 释放指定的资源
- 参数:
  - `handle`: 客户端句柄
  - `resource_id`: 资源ID
- 返回值: 成功返回1，失败返回0

#### 4. 断开连接

```c
int client_disconnect(ClientHandle handle);
```
- 功能: 断开与服务器的连接并释放资源
- 参数:
  - `handle`: 客户端句柄
- 返回值: 成功返回1，失败返回0

## 运行示例

1. 首先启动WebSocket服务器
2. 运行编译好的C示例程序:

```bash
./example
```

## 注意事项

1. 确保在调用`client_disconnect`后不再使用客户端句柄
2. 本实现使用了tokio运行时，确保在多线程环境中正确使用
3. C SDK使用了线程安全的方式实现，可以在多线程环境中使用
4. 错误处理简化，实际应用中应增强错误处理逻辑