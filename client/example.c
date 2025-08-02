#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// 引入C SDK头文件
// 实际使用时，应该将这些声明放在一个头文件中
typedef struct ClientHandle {
    void* ptr;
} ClientHandle;

// 更新函数声明以匹配新的接口
extern ClientHandle client_init(const char* ip, unsigned short port);
extern int client_allocate_resource(
    ClientHandle handle, 
    const unsigned short* resource_ids, 
    unsigned int count, 
    unsigned short* output_buffer, 
    unsigned int* actual_count
);
extern int client_release_resource(
    ClientHandle handle, 
    const unsigned short* resource_ids, 
    unsigned int count, 
    unsigned short* output_buffer, 
    unsigned int* actual_count
);
extern int client_disconnect(ClientHandle handle);
extern void client_shutdown(ClientHandle handle);

// 打印数组函数
void print_array(const unsigned short* array, unsigned int count, const char* name) {
    printf("%s: ", name);
    for (unsigned int i = 0; i < count; i++) {
        printf("%d ", array[i]);
    }
    printf("\n");
}

// 检查资源是否已被申请
int is_resource_already_allocated(const unsigned short* resource_ids, unsigned int count, const unsigned short* allocated_resources, unsigned int allocated_count) {
    for (unsigned int i = 0; i < count; i++) {
        unsigned short id = resource_ids[i];
        for (unsigned int j = 0; j < allocated_count; j++) {
            if (allocated_resources[j] == id) {
                return 1; // 已被申请
            }
        }
    }
    return 0; // 未被申请
}

// 添加已申请的资源到记录中
void add_allocated_resources(const unsigned short* new_resources, unsigned int new_count, unsigned short* allocated_resources, unsigned int* allocated_count) {
    for (unsigned int i = 0; i < new_count; i++) {
        unsigned short id = new_resources[i];
        // 检查是否已存在（避免重复添加）
        int exists = 0;
        for (unsigned int j = 0; j < *allocated_count; j++) {
            if (allocated_resources[j] == id) {
                exists = 1;
                break;
            }
        }
        if (!exists) {
            allocated_resources[*allocated_count] = id;
            (*allocated_count)++;
        }
    }
}

// 从记录中移除已释放的资源
void remove_allocated_resources(const unsigned short* released_resources, unsigned int released_count, unsigned short* allocated_resources, unsigned int* allocated_count) {
    for (unsigned int i = 0; i < released_count; i++) {
        unsigned short id = released_resources[i];
        for (unsigned int j = 0; j < *allocated_count; j++) {
            if (allocated_resources[j] == id) {
                // 移除该资源（用最后一个元素替换）
                allocated_resources[j] = allocated_resources[*allocated_count - 1];
                (*allocated_count)--;
                break;
            }
        }
    }
}

int main() {
    // 初始化客户端，连接到本地8080端口的服务器
    ClientHandle handle = client_init("127.0.0.1", 3099);

    if (handle.ptr == NULL) {
        printf("Failed to initialize client\n");
        return 1;
    }

    printf("Client initialized successfully\n\n");

    // 记录已申请的资源
    unsigned short allocated_resources[16]; // 最大资源ID为15
    unsigned int allocated_count = 0;

    // 测试场景1: 申请单个资源
    unsigned short resource1[16] = {1,2,3};
    unsigned short result1[1];
    unsigned int actual1;
    unsigned short released1[1];
    unsigned int actual_released1;
    int release_success1;

    if (is_resource_already_allocated(resource1, 1, allocated_resources, allocated_count)) {
        printf("=== 测试场景1: 申请单个资源 ===\n");
        printf("资源 %d 已被申请，跳过申请\n", resource1[0]);
    } else {
        int success1 = client_allocate_resource(handle, resource1, 1, result1, &actual1);
        printf("=== 测试场景1: 申请单个资源 ===\n");
        printf("申请资源: ");
        print_array(resource1, 1, "");
        if (success1) {
            printf("申请成功，实际获得资源数: %d\n", actual1);
            print_array(result1, actual1, "获得的资源");
            // 记录已申请的资源
            add_allocated_resources(result1, actual1, allocated_resources, &allocated_count);
            printf("当前已申请资源总数: %d\n", allocated_count);
            print_array(allocated_resources, allocated_count, "已申请资源列表");
        } else {
            printf("申请失败\n");
        }
    }
    printf("\n");

    // 测试场景1.1: 释放单个资源
    if (allocated_count > 0) {
        printf("=== 测试场景1.1: 释放单个资源 ===\n");
        print_array(resource1, 1, "释放资源");
        release_success1 = client_release_resource(handle, resource1, 1, released1, &actual_released1);
        if (release_success1) {
            printf("释放成功，实际释放资源数: %d\n", actual_released1);
            print_array(released1, actual_released1, "释放的资源");
            // 从记录中移除已释放的资源
            remove_allocated_resources(released1, actual_released1, allocated_resources, &allocated_count);
            printf("当前已申请资源总数: %d\n", allocated_count);
            print_array(allocated_resources, allocated_count, "已申请资源列表");
        } else {
            printf("释放失败\n");
        }
    }
    printf("\n");

    // 测试场景2: 申请多个资源
    unsigned short resources2[16] = {1, 2, 3};
    unsigned short result2[3];
    unsigned int actual2;
    unsigned short released2[3];
    unsigned int actual_released2;
    int release_success2;
    
    if (is_resource_already_allocated(resources2, 3, allocated_resources, allocated_count)) {
        printf("=== 测试场景2: 申请多个资源 ===\n");
        printf("部分或全部资源已被申请，跳过申请\n");
    } else {
        int success2 = client_allocate_resource(handle, resources2, 3, result2, &actual2);
        printf("=== 测试场景2: 申请多个资源 ===\n");
        print_array(resources2, 3, "申请资源");
        if (success2) {
            printf("申请成功，实际获得资源数: %d\n", actual2);
            print_array(result2, actual2, "获得的资源");
            // 记录已申请的资源
            add_allocated_resources(result2, actual2, allocated_resources, &allocated_count);
            printf("当前已申请资源总数: %d\n", allocated_count);
            print_array(allocated_resources, allocated_count, "已申请资源列表");
        } else {
            printf("申请失败\n");
        }
    }
    printf("\n");

    // 测试场景2.1: 释放多个资源
    if (allocated_count > 0) {
        printf("=== 测试场景2.1: 释放多个资源 ===\n");
        print_array(resources2, 3, "释放资源");
        release_success2 = client_release_resource(handle, resources2, 3, released2, &actual_released2);
        if (release_success2) {
            printf("释放成功，实际释放资源数: %d\n", actual_released2);
            print_array(released2, actual_released2, "释放的资源");
            // 从记录中移除已释放的资源
            remove_allocated_resources(released2, actual_released2, allocated_resources, &allocated_count);
            printf("当前已申请资源总数: %d\n", allocated_count);
            print_array(allocated_resources, allocated_count, "已申请资源列表");
        } else {
            printf("释放失败\n");
        }
    }
    printf("\n");

    // 测试场景3: 尝试重复申请已有的资源
    unsigned short resources3[16] = {4};
    unsigned short result3[1];
    unsigned int actual3;
    unsigned short result3_2[1];
    unsigned int actual3_2;
    unsigned short released3[1];
    unsigned int actual_released3;
    int release_success3;
    
    // 先申请资源4
    if (is_resource_already_allocated(resources3, 1, allocated_resources, allocated_count)) {
        printf("=== 测试场景3: 尝试重复申请 ===\n");
        printf("资源 %d 已被申请，跳过首次申请\n", resources3[0]);
    } else {
        int success3 = client_allocate_resource(handle, resources3, 1, result3, &actual3);
        printf("=== 测试场景3: 尝试重复申请 ===\n");
        printf("首次申请资源: ");
        print_array(resources3, 1, "");
        if (success3) {
            printf("首次申请成功，实际获得资源数: %d\n", actual3);
            print_array(result3, actual3, "获得的资源");
            // 记录已申请的资源
            add_allocated_resources(result3, actual3, allocated_resources, &allocated_count);
            printf("当前已申请资源总数: %d\n", allocated_count);
            print_array(allocated_resources, allocated_count, "已申请资源列表");

            // 再次申请相同资源
            printf("\n尝试再次申请相同资源...\n");
            int success3_2 = client_allocate_resource(handle, resources3, 1, result3_2, &actual3_2);
            if (success3_2) {
                printf("再次申请成功（不符合预期），实际获得资源数: %d\n", actual3_2);
                print_array(result3_2, actual3_2, "获得的资源");
                // 记录已申请的资源
                add_allocated_resources(result3_2, actual3_2, allocated_resources, &allocated_count);
                printf("当前已申请资源总数: %d\n", allocated_count);
                print_array(allocated_resources, allocated_count, "已申请资源列表");
            } else {
                printf("再次申请失败（符合预期）\n");
            }
        } else {
            printf("首次申请失败\n");
        }
    }
    printf("\n");

    // 测试场景3.1: 释放测试场景3的资源
    if (allocated_count > 0) {
        printf("=== 测试场景3.1: 释放测试场景3的资源 ===\n");
        print_array(resources3, 1, "释放资源");
        release_success3 = client_release_resource(handle, resources3, 1, released3, &actual_released3);
        if (release_success3) {
            printf("释放成功，实际释放资源数: %d\n", actual_released3);
            print_array(released3, actual_released3, "释放的资源");
            // 从记录中移除已释放的资源
            remove_allocated_resources(released3, actual_released3, allocated_resources, &allocated_count);
            printf("当前已申请资源总数: %d\n", allocated_count);
            print_array(allocated_resources, allocated_count, "已申请资源列表");
        } else {
            printf("释放失败\n");
        }
    }
    printf("\n");

    // 测试场景4: 申请超出范围的资源ID
    unsigned short resources4[16] = {10, 15, 16}; // 16超出最大ID 15
    unsigned short result4[3];
    unsigned int actual4;
    unsigned short released4[3];
    unsigned int actual_released4;
    int release_success4;
    
    if (is_resource_already_allocated(resources4, 3, allocated_resources, allocated_count)) {
        printf("=== 测试场景4: 申请超出范围的资源 ===\n");
        printf("部分或全部资源已被申请，跳过申请\n");
    } else {
        int success4 = client_allocate_resource(handle, resources4, 3, result4, &actual4);
        printf("=== 测试场景4: 申请超出范围的资源 ===\n");
        print_array(resources4, 3, "申请资源");
        if (success4) {
            printf("申请成功，实际获得资源数: %d\n", actual4);
            print_array(result4, actual4, "获得的资源");
            // 记录已申请的资源
            add_allocated_resources(result4, actual4, allocated_resources, &allocated_count);
            printf("当前已申请资源总数: %d\n", allocated_count);
            print_array(allocated_resources, allocated_count, "已申请资源列表");
        } else {
            printf("申请失败\n");
        }
    }
    printf("\n");

    // 测试场景4.1: 释放测试场景4的资源（如果有）
    if (allocated_count > 0) {
        printf("=== 测试场景4.1: 释放测试场景4的资源 ===\n");
        // 只释放有效的资源（排除16）
        unsigned short valid_resources4[16] = {10, 15};
        release_success4 = client_release_resource(handle, valid_resources4, 2, released4, &actual_released4);
        if (release_success4) {
            printf("释放成功，实际释放资源数: %d\n", actual_released4);
            print_array(released4, actual_released4, "释放的资源");
            // 从记录中移除已释放的资源
            remove_allocated_resources(released4, actual_released4, allocated_resources, &allocated_count);
            printf("当前已申请资源总数: %d\n", allocated_count);
            print_array(allocated_resources, allocated_count, "已申请资源列表");
        } else {
            printf("释放失败\n");
        }
    }
    printf("\n");

    // 测试场景5: 释放不存在的资源
    unsigned short invalid_resource[16] = {99};
    unsigned short result_invalid[1];
    unsigned int actual_invalid;
    int success_invalid = client_release_resource(handle, invalid_resource, 1, result_invalid, &actual_invalid);
    printf("=== 测试场景5: 释放不存在的资源 ===\n");
    print_array(invalid_resource, 1, "释放资源");
    if (success_invalid) {
        printf("释放成功（不符合预期），实际释放资源数: %d\n", actual_invalid);
        print_array(result_invalid, actual_invalid, "释放的资源");
    } else {
        printf("释放失败（符合预期）\n");
    }
    printf("\n");

    // 测试场景6: 释放后重新申请
    unsigned short resource6[16] = {5};
    unsigned short result6[1];
    unsigned int actual6;
    unsigned short released6[1];
    unsigned int actual_released6;
    int release_success6;
    
    // 先申请
    if (is_resource_already_allocated(resource6, 1, allocated_resources, allocated_count)) {
        printf("=== 测试场景6: 释放后重新申请 ===\n");
        printf("资源 %d 已被申请，跳过申请\n", resource6[0]);
    } else {
        int success6 = client_allocate_resource(handle, resource6, 1, result6, &actual6);
        printf("=== 测试场景6: 释放后重新申请 ===\n");
        printf("申请资源: ");
        print_array(resource6, 1, "");
        if (success6) {
            printf("申请成功，实际获得资源数: %d\n", actual6);
            print_array(result6, actual6, "获得的资源");
            // 记录已申请的资源
            add_allocated_resources(result6, actual6, allocated_resources, &allocated_count);
            printf("当前已申请资源总数: %d\n", allocated_count);
            print_array(allocated_resources, allocated_count, "已申请资源列表");

            // 释放
            printf("\n释放资源...\n");
            release_success6 = client_release_resource(handle, resource6, 1, released6, &actual_released6);
            if (release_success6) {
                printf("释放成功，实际释放资源数: %d\n", actual_released6);
                print_array(released6, actual_released6, "释放的资源");
                // 从记录中移除已释放的资源
                remove_allocated_resources(released6, actual_released6, allocated_resources, &allocated_count);
                printf("当前已申请资源总数: %d\n", allocated_count);
                print_array(allocated_resources, allocated_count, "已申请资源列表");

                // 重新申请
                printf("\n重新申请资源...\n");
                int success6_2 = client_allocate_resource(handle, resource6, 1, result6, &actual6);
                if (success6_2) {
                    printf("重新申请成功，实际获得资源数: %d\n", actual6);
                    print_array(result6, actual6, "获得的资源");
                    // 记录已申请的资源
                    add_allocated_resources(result6, actual6, allocated_resources, &allocated_count);
                    printf("当前已申请资源总数: %d\n", allocated_count);
                    print_array(allocated_resources, allocated_count, "已申请资源列表");

                    // 再次释放
                    release_success6 = client_release_resource(handle, resource6, 1, released6, &actual_released6);
                    if (release_success6) {
                        printf("再次释放成功，实际释放资源数: %d\n", actual_released6);
                        print_array(released6, actual_released6, "释放的资源");
                        // 从记录中移除已释放的资源
                        remove_allocated_resources(released6, actual_released6, allocated_resources, &allocated_count);
                        printf("当前已申请资源总数: %d\n", allocated_count);
                        print_array(allocated_resources, allocated_count, "已申请资源列表");
                    } else {
                        printf("再次释放失败\n");
                    }
                } else {
                    printf("重新申请失败\n");
                }
            } else {
                printf("释放失败\n");
            }
        } else {
            printf("申请失败\n");
        }
    }
    printf("\n");

    // 断开连接
    int disconnect_result = client_disconnect(handle);
    if (disconnect_result) {
        printf("Disconnected successfully\n");
    } else {
        printf("Failed to disconnect\n");
    }

    // 关闭客户端
    client_shutdown(handle);

    return 0;
}

// 编译命令示例:
// gcc -o example example.c -L. -lclient
// 其中-lclient是链接到我们编译的Rust客户端库