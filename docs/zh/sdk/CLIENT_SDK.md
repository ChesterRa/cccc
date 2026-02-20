# CCCC 客户端 SDK

将外部应用/服务与运行中的 CCCC daemon 集成的官方 SDK。

## 仓库和包

- 仓库：[ChesterRa/cccc-sdk](https://github.com/ChesterRa/cccc-sdk)
- Python 包：`cccc-sdk`（导入为 `cccc_sdk`）
- TypeScript 包：`cccc-sdk`

## 与 CCCC 核心的关系

CCCC 核心（`cccc-pair`）是运行时系统：

- daemon
- ledger/状态
- Web/CLI/MCP/IM 端口

SDK 是客户端层：

- 不启动/拥有 daemon 状态
- 连接到已有的 daemon
- 使用与 Web/CLI/MCP 相同的控制平面语义

## 何时使用 SDK vs MCP

当你在构建以下内容时使用 SDK：

- 后端服务
- Bot
- IDE 集成
- Agent 运行时之外的自动化服务

当调用者是会话内的 Agent/工具运行时时使用 MCP。

## 安装

```bash
# Python
pip install -U cccc-sdk

# TypeScript
npm install cccc-sdk
```

## 运行时要求

需要 CCCC daemon 已经在运行。

```bash
cccc daemon status
```

SDK 客户端会连接到 CCCC 运行时配置的 daemon 传输层（`CCCC_HOME`、daemon socket/TCP 设置）。

## 集成模型

典型的生产环境部署：

1. 运行 CCCC 核心（`cccc-pair`）作为本地控制平面。
2. 通过 SDK 连接你的应用/服务。
3. 使用 SDK 调用进行工作组/actor/消息/上下文/自动化操作。
4. 在 CCCC ledger 和工作组状态中保持运维事实来源。

## 兼容性说明

- SDK 和核心独立发布，但建议保持相同的主/次版本号以获得最佳兼容性。
- 协议级别的详细信息，请参阅：
  - `docs/standards/CCCS_V1.md`
  - `docs/standards/CCCC_DAEMON_IPC_V1.md`

## 下一步

具体的 API 示例和各语言的使用方式，请查阅 SDK 仓库文档：

- [cccc-sdk README](https://github.com/ChesterRa/cccc-sdk)
