# CCCC vNext Documentation

> CCCC = Collaborative Code Coordination Center
> 
> 一个全局的 AI Agent 协作中枢：单一 daemon 管理多个工作组，Web/CLI/IM 作为入口。
>
> 说明：`vnext/` 目录是历史设计与迭代记录，内容可能滞后。当前实现请优先参考 `docs/reference/*` 与 `docs/standards/*`。

## 快速开始

```bash
# 安装
pip install -e .

# 启动（daemon + web）
cccc

# 打开 Web UI
open http://127.0.0.1:8848/ui/

# 或使用 CLI
cccc attach .                    # 创建/绑定工作组
cccc actor add agent-1 --runtime claude  # 添加 agent
cccc group start                 # 启动所有 agents
cccc send "Hello" --to @all      # 发送消息
```

## 文档结构

| 文档 | 内容 |
|------|------|
| [ARCHITECTURE.md](./ARCHITECTURE.md) | 架构设计、核心概念、目录结构、Ledger Schema |
| [STATUS.md](./STATUS.md) | 当前实现状态、路线图、待办事项 |
| [FEATURES.md](./FEATURES.md) | 功能详解：IM Bridge、消息机制、Agent 指导 |
| [RELEASE.md](./RELEASE.md) | 发布流程、tag/version 约定、TestPyPI 安装 RC |
| [archive/](./archive/) | 历史设计文档（供参考） |

## 核心概念

### Working Group（工作组）
- 像 IM 群聊，但具备执行/交付能力
- 每个 group 有一个 append-only ledger（事实流）
- 可绑定多个 Scope（项目目录）

### Actor（执行者）
- **Foreman**: 协调者 + 执行者（第一个启用的 actor 自动成为 foreman）
- **Peer**: 独立专家（其他 actors）
- 支持 PTY（终端）和 Headless（纯 MCP）两种 runner

### Ledger（账本）
- 单一事实源：`~/.cccc/groups/<group_id>/ledger.jsonl`
- 所有消息、事件、决策都记录在此
- 支持 snapshot/compaction

## 目录布局

```
~/.cccc/                          # CCCC_HOME
├── registry.json                 # 工作组索引
├── daemon/                       # daemon 运行时
│   ├── ccccd.sock               # IPC socket
│   └── ccccd.log
└── groups/<group_id>/
    ├── group.yaml               # 元数据
    ├── ledger.jsonl             # 事实流
    ├── context/                 # 上下文（vision/sketch/tasks）
    └── state/                   # 运行时状态
```

## 技术栈

- **Kernel/Daemon**: Python + Pydantic
- **Web Port**: FastAPI + Uvicorn
- **Web UI**: React + TypeScript + Vite + Tailwind + xterm.js
- **MCP**: stdio mode, JSON-RPC tool surface

## 相关链接

- 源码：`src/cccc/`
- 老版本（v0.3.x tmux 版）：https://github.com/ChesterRa/cccc-tmux
- AGENTS.md（本地文件，不提交）：工作区开发规范/协作约定（可自行维护）
