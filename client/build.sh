#!/bin/bash

# 编译Rust库
cargo build --release

# 检查库文件是否存在
if [ -f "./../target/release/libclient.dylib" ]; then
    echo "Rust库编译成功"
else
    echo "警告：未找到Rust库文件，可能编译路径有问题"
    # 尝试查找库文件
    echo "正在查找库文件..."
    find . -name "libclient.dylib" || echo "未找到库文件"
    exit 1
fi

# 编译C示例代码
gcc -o example example.c -L ../target/release -l client

# 检查是否编译成功
if [ -f "example" ]; then
    echo "C示例代码编译成功"
    echo "运行示例：./example"
else
    echo "C示例代码编译失败"
    exit 1
fi