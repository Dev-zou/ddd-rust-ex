# Rust资源管理系统

这是一个基于Rust的资源管理系统，包含服务器和客户端两个主要组件。

## 项目结构
- `server/`: 资源管理服务器实现
- `client/`: 客户端库，提供与服务器交互的API
- `common/`: 共享数据结构和工具

## 构建和运行

### 构建项目
```bash
cargo build
```

### 运行服务器
```bash
cargo run -p server
```

## 覆盖率检查

本项目使用`cargo-tarpaulin`工具进行代码覆盖率检查。

### 使用方法
1. 确保已安装所需依赖:
   ```bash
   # 安装cargo-tarpaulin（如果尚未安装）
   cargo install cargo-tarpaulin
   ```

2. 运行覆盖率检查脚本:
   ```bash
   ./coverage.sh
   ```

### 覆盖率报告
- 覆盖率报告将生成在`./coverage`目录下
- 服务器覆盖率报告: `./coverage/server/cobertura.xml`
- 客户端覆盖率报告: `./coverage/client/cobertura.xml`

### 查看HTML报告（可选）
如果您想生成HTML格式的覆盖率报告，可以安装`lcov`工具并取消`coverage.sh`脚本中相关行的注释:
```bash
# 在macOS上
brew install lcov

# 在Ubuntu上
sudo apt-get install lcov
```

然后修改`coverage.sh`脚本，取消以下行的注释:
```bash
# 生成HTML报告（可选）
echo "Generating HTML report..."
genhtml -o ./coverage/html ./coverage/server/cobertura.xml
```

生成的HTML报告将位于`./coverage/html`目录下。