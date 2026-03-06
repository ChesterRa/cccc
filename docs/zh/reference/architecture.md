# 架构

> CCCC = Collaborative Code Coordination Center（协作代码协调中心）
>
> 全局 AI Agent 协作中枢：单一 daemon 管理多个工作组，Web/CLI/IM 作为入口。

## 核心概念

### 工作组（Working Group）

- 类似 IM 群聊，但具备执行/投递能力
- 每个工作组拥有一个追加写入的 ledger（事件流）
- 可绑定多个 Scope（项目目录）

### Actor

- **Foreman**：协调者 + 执行者（第一个启用的 actor 自动成为 foreman）
- **Peer**：独立专家（其他 actor）
- 支持 PTY（终端）和 Headless（仅 MCP）两种运行器

### Ledger

- 唯一事实来源：`~/.cccc/groups/<group_id>/ledger.jsonl`
- 所有消息、事件和决策都记录在此
- 支持快照/压缩

## 目录结构

默认：`CCCC_HOME=~/.cccc`

```
~/.cccc/
├── registry.json                 # 工作组索引
├── daemon/
│   ├── ccccd.pid
│   ├── ccccd.log
│   └── ccccd.sock               # IPC 套接字
└── groups/<group_id>/
    ├── group.yaml               # 元数据
    ├── ledger.jsonl             # 事件流（追加写入）
    ├── context/                 # 上下文（vision/sketch/tasks）
    └── state/                   # 运行时状态
        └── blobs/               # 大文本/附件（在 ledger 中引用）
```

## 架构分层

```
┌─────────────────────────────────────────────────────────┐
│                      Ports（入口层）                      │
│   Web UI (React)  │  CLI  │  IM Bridge  │  MCP Server   │
├─────────────────────────────────────────────────────────┤
│                    Daemon (ccccd)                        │
│   IPC Server  │  Delivery  │  Automation  │  Runners    │
├─────────────────────────────────────────────────────────┤
│                      Kernel（内核层）                     │
│   Group  │  Actor  │  Ledger  │  Inbox  │  Permissions  │
├─────────────────────────────────────────────────────────┤
│                    Contracts (v1)                        │
│   Event  │  Message  │  Actor  │  IPC                   │
└─────────────────────────────────────────────────────────┘
```

### 契约层（Contracts）

- Pydantic 模型定义所有数据结构
- 版本化：`src/cccc/contracts/v1/`
- 稳定边界，不含业务实现

### 内核层（Kernel）

- Group/Scope/Ledger/Inbox/Permissions
- 依赖契约层，不依赖特定端口

### Daemon

- 单写入者原则：所有 ledger 写入都经过 daemon
- IPC + 监督 + 投递/自动化
- 管理 actor 生命周期

### 端口层（Ports）

- 仅通过 IPC 与 daemon 交互
- 不持有业务状态

## Ledger 模式（v1）

### 事件信封

```jsonc
{
  "v": 1,
  "id": "event-id",
  "ts": "2025-01-01T00:00:00.000000Z",
  "kind": "chat.message",
  "group_id": "g_xxx",
  "scope_key": "s_xxx",
  "by": "user",
  "data": {}
}
```

### 已知事件类型

| 类型 | 描述 |
|------|------|
| `group.create/update/attach/start/stop/set_state/settings_update/automation_update` | 工作组生命周期和配置 |
| `actor.add/update/start/stop/restart/remove` | Actor 生命周期 |
| `chat.message` | 聊天消息 |
| `chat.read` / `chat.ack` | 已读和确认事件 |
| `system.notify` / `system.notify_ack` | 系统通知和确认 |

### chat.message 数据

```python
class ChatMessageData:
    text: str
    format: "plain" | "markdown"
    to: list[str]           # 接收者（空 = 广播）
    reply_to: str | None    # 回复哪条消息
    quote_text: str | None  # 引用文本
    attachments: list[dict] # 附件元数据（内容存储在 CCCC_HOME blobs 中）
```

### 接收者语义（to 字段）

| 标识 | 语义 |
|------|------|
| `[]`（空） | 广播 |
| `user` | 用户 |
| `@all` | 所有 actor |
| `@peers` | 所有 peer |
| `@foreman` | Foreman |
| `<actor_id>` | 特定 actor |

## 文件和附件

### 设计原则

- **Ledger 只存引用，不存大二进制文件**：大文本/附件存放在 `CCCC_HOME` blobs 中（如 `groups/<group_id>/state/blobs/`）。
- **默认不自动写入仓库**：附件属于运行时域（`CCCC_HOME`）；如需放入 scope/仓库，用户/agent 显式复制/导出。
- **内容可移植**：附件使用 `sha256` 作为稳定标识，支持未来跨组/仓库复制和引用重写。

## 角色和权限

### 角色定义

- **Foreman = 协调者 + 执行者**
  - 参与实际工作，不只是分配任务
  - 额外协调职责（接收 actor_idle、silence_check 通知）
  - 可以添加/启动/停止任何 actor

- **Peer = 独立专家**
  - 拥有独立的专业判断
  - 可以质疑 foreman 的决策
  - 只能管理自己

### 权限矩阵

| 操作 | user | foreman | peer |
|------|------|---------|------|
| actor_add | ✓ | ✓ | ✗ |
| actor_start | ✓ | ✓（任意） | ✗ |
| actor_stop | ✓ | ✓（任意） | ✓（自己） |
| actor_restart | ✓ | ✓（任意） | ✓（自己） |
| actor_remove | ✓ | ✓（自己） | ✓（自己） |

## MCP 服务器

MCP 以能力组的形式暴露（工具数量不硬编码）：

### 协作控制（`cccc_*`）

- 收件箱和引导：`cccc_inbox_*`、`cccc_bootstrap`
- 消息和文件：`cccc_message_*`、`cccc_file_send`、`cccc_blob_path`
- 工作组/actor 操作：`cccc_group_*`、`cccc_actor_*`、`cccc_runtime_list`
- 自动化：`cccc_automation_state`、`cccc_automation_manage`
- 项目/帮助信息：`cccc_project_info`、`cccc_help`

### 上下文同步（`cccc_context_*` 及相关）

- 上下文批量操作：`cccc_context_get`、`cccc_context_sync`
- 愿景/草图：`cccc_vision_update`、`cccc_sketch_update`
- 里程碑/任务：`cccc_milestone_*`、`cccc_task_*`
- 笔记/引用/在线状态：`cccc_note_*`、`cccc_reference_*`、`cccc_presence_*`

### Headless 和通知

- Headless 运行器：`cccc_headless_*`
- 系统通知：`cccc_notify_*`

### 诊断和终端记录

- 终端记录：`cccc_terminal_tail`
- 开发者诊断：`cccc_debug_*`

## 技术栈

| 层级 | 技术 |
|------|------|
| Kernel/Daemon | Python + Pydantic |
| Web Port | FastAPI + Uvicorn |
| Web UI | React + TypeScript + Vite + Tailwind + xterm.js |
| MCP | stdio 模式，JSON-RPC |

## 源码结构

```
src/cccc/
├── contracts/v1/          # 契约层
├── kernel/                # 内核
├── daemon/                # Daemon 进程
├── runners/               # PTY/Headless 运行器
├── ports/
│   ├── web/              # Web 端口
│   ├── im/               # IM Bridge
│   └── mcp/              # MCP 服务器
└── resources/            # 内置资源
```
