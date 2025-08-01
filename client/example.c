#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// 引入C SDK头文件
// 实际使用时，应该将这些声明放在一个头文件中
typedef struct ClientHandle {
    void* ptr;
} ClientHandle;

extern ClientHandle client_init(const char* ip, unsigned short port);
extern int client_allocate_resource(ClientHandle handle, unsigned short resource_id);
extern int client_release_resource(ClientHandle handle, unsigned short resource_id);
extern int client_disconnect(ClientHandle handle);

int main() {
    // 初始化客户端，连接到本地8080端口的服务器
    ClientHandle handle = client_init("127.0.0.1", 3099);

    if (handle.ptr == NULL) {
        printf("Failed to initialize client\n");
        return 1;
    }

    printf("Client initialized successfully\n");

    int result = 0;
    // 申请资源
    // result = client_allocate_resource(handle, 1);
    // if (result) {
    //     printf("Resource allocated successfully\n");
    // } else {
    //     printf("Failed to allocate resource\n");
    // }

    // // 释放资源
    // result = client_release_resource(handle, 1);
    // if (result) {
    //     printf("Resource released successfully\n");
    // } else {
    //     printf("Failed to release resource\n");
    // }

    // 断开连接
    result = client_disconnect(handle);
    if (result) {
        printf("Disconnected successfully\n");
    } else {
        printf("Failed to disconnect\n");
    }

    return 0;
}

// 编译命令示例:
// gcc -o example example.c -L. -lclient
// 其中-lclient是链接到我们编译的Rust客户端库