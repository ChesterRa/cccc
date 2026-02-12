# CCCC vNext — Architecture

> CCCC = Collaborative Code Coordination Center
>
> 全局唯一的 AI Agent 协作中枢：单一 daemon 管理多个工作组，Web/CLI/IM 作为入口。
>
> 说明：本页属于 vNext 历史设计快照，可能滞后于当前实现。当前行为以 `docs/reference/*` 与 `docs/standards/*` 为准。

## 1. 核心概念

### 1.1 Working Group（工作组）

- 像 IM 群聊，但具备执行/交付能力
- 每个 group 有一个 append-only ledger（事实流）
- 可绑定多个 Scope（项目目录）

### 1.2 Actor（执行者）

- **Foreman**: 协调者 + 执行者（第一个启用的 actor 自动成为 foreman）
- **Peer**: 独立专家（其他 actors）
- 支持 PTY（终端）和 Headless（纯 MCP）两种 runner

### 1.3 Ledger（账本）

- 单一事实源：`~/.cccc/groups/<group_id>/ledger.jsonl`
- 所有消息、事件、决策都记录在此
- 支持 snapshot/compaction

## 2. 全局目录布局

默认：`CCCC_HOME=~/.cccc`

```
~/.cccc/
├── registry.json                 # 工作组索引
├── daemon/
│   ├── ccccd.pid
│   ├── ccccd.log
│   └── ccccd.sock               # IPC socket
└── groups/<group_id>/
    ├── group.yaml               # 元数据
    ├── ledger.jsonl             # 事实流（append-only）
    ├── context/                 # 上下文（vision/sketch/tasks）
    └── state/                   # 运行时状态
        └── blobs/               # 大文本/附件等二进制（以引用方式进入 ledger）
```

## 3. 架构分层

```
┌─────────────────────────────────────────────────────────┐
│                      Ports (入口)                        │
│   Web UI (React)  │  CLI  │  IM Bridge  │  MCP Server   │
├─────────────────────────────────────────────────────────┤
│                    Daemon (ccccd)                        │
│   IPC Server  │  Delivery  │  Automation  │  Runners    │
├─────────────────────────────────────────────────────────┤
│                      Kernel                              │
│   Group  │  Actor  │  Ledger  │  Inbox  │  Permissions  │
├─────────────────────────────────────────────────────────┤
│                    Contracts (v1)                        │
│   Event  │  Message  │  Actor  │  IPC                   │
└─────────────────────────────────────────────────────────┘
```

### 3.1 Contracts（契约层）

- Pydantic models 定义所有数据结构
- 版本化：`src/cccc/contracts/v1/`
- 稳定边界，不引入业务实现

### 3.2 Kernel（内核）

- Group/Scope/Ledger/Inbox/Permissions
- 依赖 contracts，不依赖具体 ports

### 3.3 Daemon（守护进程）

- 单写者原则：所有 ledger 写入通过 daemon
- IPC + supervision + delivery/automation
- 管理 actor 生命周期

### 3.4 Ports（入口）

- 只通过 IPC 与 daemon 交互
- 不持有业务状态

## 4. Ledger Schema (v1)

### 4.1 Event Envelope（统一外壳）

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

### 4.2 Known Kinds

| Kind | 说明 |
|------|------|
| `group.create/update/attach/start/stop` | 工作组生命周期 |
| `actor.add/update/start/stop/restart/remove` | Actor 生命周期 |
| `chat.message` | 聊天消息 |
| `chat.read` | 已读标记 |
| `system.notify` | 系统通知 |

### 4.3 chat.message Data

```python
class ChatMessageData:
    text: str
    format: "plain" | "markdown"
    to: list[str]           # 收件人（空=广播）
    reply_to: str | None    # 回复哪条消息
    quote_text: str | None  # 被引用文本
    attachments: list[dict] # 附件元信息（内容存储在 CCCC_HOME 的 blobs 中）
```

### 4.4 收件人语义（to 字段）

| Token | 语义 |
|-------|------|
| `[]`（空） | 广播 |
| `user` | 用户本人 |
| `@all` | 所有 actors |
| `@peers` | 所有 peer |
| `@foreman` | foreman |
| `<actor_id>` | 指定 actor |

## 5. 文件与附件（vNext 方向）

### 5.1 设计原则

- **Ledger 只存引用，不存大二进制**：大文本/附件落到 `CCCC_HOME` 的 blobs（例如 `groups/<group_id>/state/ledger/blobs/`）。
- **默认不自动写入 repo**：附件属于运行时域（`CCCC_HOME`）；如需落到 scope/repo，由 user/agent 显式执行拷贝/导出动作。
- **内容可迁移**：附件以 `sha256` 为稳定身份，允许未来跨 group/repo 复制与引用重写。

### 5.2 跨 Group/Repo（规划）

> 当前版本尚未实现跨 group 消息投递；以下为后续规划。

- **跨 group 发送消息时的附件语义**：
  - 发送侧：消息引用本 group 的附件（blob + 元信息）。
  - 传输层：将附件 blob 复制到目标 group 的 blob 存储（避免路径耦合，便于 GC/隔离）。
  - 目标侧：写入目标 group 的 ledger，并将 `attachments[].path` 重写为目标 group 内的相对路径；`attachments[].sha256` 保持不变用于去重/校验。


## 6. 角色与权限

### 6.1 角色定义

- **Foreman = Coordinator + Worker**
  - 做实际工作，不只是分配任务
  - 额外协调职责（收到 actor_idle, silence_check 通知）
  - 可添加/启动/停止任何 actor
  
- **Peer = Independent Expert**
  - 有独立专业判断
  - 可质疑 foreman 决策
  - 只能管理自己

### 6.2 权限矩阵

| Action | user | foreman | peer |
|--------|------|---------|------|
| actor_add | ✓ | ✓ | ✗ |
| actor_start | ✓ | ✓ (any) | ✗ |
| actor_stop | ✓ | ✓ (any) | ✓ (self) |
| actor_restart | ✓ | ✓ (any) | ✓ (self) |
| actor_remove | ✓ | ✓ (self) | ✓ (self) |

## 7. MCP Server

MCP 工具按能力命名空间组织（工具数量会随版本迭代）：

### 7.1 cccc.* (协作控制面)

- `cccc_inbox_list` / `cccc_inbox_mark_read`
- `cccc_message_send` / `cccc_message_reply`
- `cccc_group_info` / `cccc_actor_list`
- `cccc_actor_add/remove/start/stop/restart`
- `cccc_runtime_list` / `cccc_project_info`

### 7.2 context.* (状态同步)

- `cccc_context_get` / `cccc_context_sync`
- `cccc_vision_update` / `cccc_sketch_update`
- `cccc_milestone_*` / `cccc_task_*`
- `cccc_note_*` / `cccc_reference_*`

### 7.3 headless.* (Headless runner)

- `cccc_headless_status` / `cccc_headless_set_status`
- `cccc_headless_ack_message`

### 7.4 notify.* (系统通知)

- `cccc_notify_send` / `cccc_notify_ack`

## 8. 技术栈

| 层 | 技术 |
|----|------|
| Kernel/Daemon | Python + Pydantic |
| Web Port | FastAPI + Uvicorn |
| Web UI | React + TypeScript + Vite + Tailwind + xterm.js |
| MCP | stdio mode, JSON-RPC |

## 9. 源码结构

```
src/cccc/
├── contracts/v1/          # 契约层
├── kernel/                # 内核
├── daemon/                # 守护进程
├── runners/               # PTY/Headless runner
├── ports/
│   ├── web/              # Web port
│   ├── im/               # IM Bridge
│   └── mcp/              # MCP Server
└── resources/            # 内置资源
```

---

详细设计历史见 `archive/CCCC_NEXT_GLOBAL_DAEMON.md`
