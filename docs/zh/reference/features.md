# 功能详解

CCCC 功能详细文档。

## IM 风格的消息系统

### 核心契约

- 消息是一等公民：一旦发送，即提交到 ledger
- 已读回执是显式的：Agent 调用 MCP 标记已读
- 回复/引用是结构化的：`reply_to` + `quote_text`
- @提及实现精准投递

### 发送消息

```bash
# CLI
cccc send "Hello"                 # 不指定 --to：使用默认接收者策略（默认 foreman）
cccc send "Hello" --to @all
cccc reply <event_id> "Reply text"

# MCP
cccc_message_send(text="Hello", to=["@all"])
cccc_message_reply(reply_to="evt_xxx", text="Reply")
```

### 已读回执

- Agent 调用 `cccc_inbox_mark_read(event_id)` 标记已读
- 已读是累积的：标记 X 意味着 X 及之前的所有消息都已读
- 游标存储在 `state/read_cursors.json`

### 投递机制

```
消息写入 ledger
    ↓
Daemon 解析 "to" 字段
    ↓
对每个目标 actor：
    ├─ PTY 运行中 → 注入终端
    └─ 否则 → 留在收件箱
    ↓
等待 agent 调用 mark_read
```

投递格式：
```
[cccc] user → peer-a: Please implement the login feature
[cccc] user → peer-a (reply to evt_abc): OK, please continue
```

## IM Bridge

### 设计原则

- **1 个工作组 = 1 个 Bot**：简单、隔离、易理解
- **显式订阅**：聊天必须 `/subscribe` 后才能接收消息
- **端口层薄**：只做消息转发；daemon 是唯一的状态源

### 支持的平台

| 平台 | 状态 | Token 配置 |
|------|------|------------|
| Telegram | ✅ 完整 | `token_env` |
| Slack | ✅ 完整 | `bot_token_env` + `app_token_env` |
| Discord | ✅ 完整 | `token_env` |
| 飞书/Lark | ✅ 完整 | `feishu_app_id_env` + `feishu_app_secret_env` |
| 钉钉 | ✅ 完整 | `dingtalk_app_key_env` + `dingtalk_app_secret_env`（+ 可选 `dingtalk_robot_code_env`） |

### 配置

```yaml
# group.yaml
im:
  platform: telegram
  token_env: TELEGRAM_BOT_TOKEN

# Slack 需要双 token
im:
  platform: slack
  bot_token_env: SLACK_BOT_TOKEN    # xoxb-... Web API
  app_token_env: SLACK_APP_TOKEN    # xapp-... Socket Mode
```

### IM 命令

| 命令 | 描述 |
|------|------|
| `/send <message>` | 使用工作组默认策略发送（默认：foreman） |
| `/send @<agent> <message>` | 发送给特定 agent |
| `/send @all <message>` | 发送给所有 agent |
| `/send @peers <message>` | 发送给非 foreman 的 agent |
| `/subscribe` | 订阅，开始接收消息 |
| `/unsubscribe` | 取消订阅 |
| `/verbose` | 切换详细模式 |
| `/status` | 显示工作组状态 |
| `/pause` / `/resume` | 暂停/恢复消息投递 |
| `/help` | 显示帮助 |

注意：
- 在私聊和群聊中 @bot 时，纯文本被视为隐式发送给默认接收者（默认：foreman）。
- 在频道（Slack/Discord）中，先 @bot 再使用 `/send`（以避免平台斜杠命令冲突）。
- 你可以在 Web UI 中配置默认接收者行为：设置 → 消息 → 默认接收者。

### CLI 命令

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
cccc im stop
cccc im status
cccc im logs -f
```

## Agent 引导

### 信息层次

```
System Prompt（薄层）
├── 你是谁：Actor ID、角色
├── 你在哪：工作组、Scope
└── 你能做什么：MCP 工具列表 + 关键提醒（见 cccc_help）

MCP Tools（权威操作手册 + 执行接口）
├── cccc_help：操作指南（playbook）
├── cccc_project_info：获取 PROJECT.md
├── cccc_inbox_list / cccc_inbox_mark_read：收件箱
└── cccc_message_send / cccc_message_reply：发送/回复

Ledger（完整记忆）
└── 所有历史消息和事件
```

### 核心原则

- **要做**：一份权威操作手册（`cccc_help`）
- **要做**：内核层强制执行（RBAC 由 daemon 管控）
- **要做**：最小启动握手（Bootstrap）
- **不要做**：同一份内容写三个版本

### Agent 标准工作流

```
1. 接收 SYSTEM 注入 → 知道自己是谁
2. 调用 cccc_inbox_list → 获取未读消息
3. 处理消息 → 执行任务
4. 调用 cccc_inbox_mark_read → 标记已读
5. 调用 cccc_message_reply → 回复结果
6. 等待下一条消息
```

## 自动化

自动化现在是一个规则引擎（用于提醒 + 运维操作），不仅仅是内置的催促。

### 规则触发器

| 触发器类型 | Web 标签 | 协议 | 典型用途 |
|------------|----------|------|----------|
| 间隔 | 每 N 分钟 | `every_seconds` | 站会/检查点提醒 |
| 定期计划 | 每日/每周/每月 | `cron` | 固定时间的定期提醒 |
| 一次性计划 | 倒计时/精确时间 | `at` | 一次性提醒和操作 |

注意：
- Web UI 默认隐藏原始 cron 表达式编辑。
- 运维操作特意限制为一次性触发。

### 规则动作

| 动作 | 配置方 | 触发器支持 | 描述 |
|------|--------|------------|------|
| `notify` | Web + MCP | 间隔/定期/一次性 | 向选定接收者发送系统通知 |
| `group_state` | Web（foreman/管理员） | 仅一次性 | 设置工作组状态（`active` / `idle` / `paused` / `stopped`） |
| `actor_control` | Web（foreman/管理员） | 仅一次性 | 启动/停止/重启选定的 actor 运行时 |

### 一次性完成语义

- 一次性规则触发后自动标记为已完成。
- 已完成的一次性规则会被禁用（不会重复触发）。
- UI 支持清理已完成项目。

### 内置策略（与自定义规则分开）

| 策略 | 配置 | 默认值 | 描述 |
|------|------|--------|------|
| 催促 | `nudge_after_seconds` | 300s | 未读消息超时提醒 |
| 要求回复催促 | `reply_required_nudge_after_seconds` | 300s | 要求回复义务的跟进 |
| 关注确认催促 | `attention_ack_nudge_after_seconds` | 600s | 关注消息缺少确认的跟进 |
| 未读催促 | `unread_nudge_after_seconds` | 900s | 收件箱仍有未读的提醒 |
| Actor 空闲 | `actor_idle_timeout_seconds` | 600s | Actor 空闲通知给 foreman |
| 保活 | `keepalive_delay_seconds` | 120s | Foreman 保活提醒 |
| 静默 | `silence_timeout_seconds` | 600s | 工作组静默通知给 foreman |
| 帮助催促 | `help_nudge_interval_seconds` / `help_nudge_min_messages` | 600s / 10 | 提示 actor 重新查阅 `cccc_help` |

### 投递节流

| 配置 | 默认值 | 描述 |
|------|--------|------|
| `min_interval_seconds` | 0s | 连续投递之间的最小间隔（`0` 禁用节流） |

## 运行时专用 Actor 密钥

CCCC 支持每个 actor 的私有环境变量，用于运行时定制（不同 actor 使用不同的模型/API 栈）。

- 存储在 `CCCC_HOME/state/secrets/actors/` 下的运行时状态中
- 不写入工作组 ledger
- 不包含在工作组模板/蓝图中
- 仅以键元数据形式可见（读取 API 从不返回值）

CLI 接口：

```bash
cccc actor secrets <actor_id> --set KEY=VALUE
cccc actor secrets <actor_id> --unset KEY
cccc actor secrets <actor_id> --keys
```

## 蓝图/工作组模板

CCCC Web 支持蓝图的导出/导入，用于可移植的工作组配置。

- 导出会捕获 actor、设置、自动化规则/代码片段和引导覆盖。
- 导入使用替换语义（将传入的配置作为新的工作组设置应用）。
- Ledger 历史被保留（导入不会重写历史事件）。
- 环境密钥被有意排除。

### MCP 管理接口

```text
cccc_automation_state
cccc_automation_manage(op=create|update|enable|disable|delete|replace_all, ...)
```

`cccc_automation_manage` 为 Agent 的提醒管理做了优化：
- Foreman 可以管理所有 notify 提醒和完整替换。
- Peer 只能管理自己的个人或共享的 notify 提醒。
- 运维操作（`group_state`、`actor_control`）保持 Web/管理员面向。

## Web UI

### Agent 标签页模式

- 每个 agent 是一个标签页
- 聊天标签 + Agent 标签
- 点击标签切换视图
- 移动端：滑动切换

### 主要功能

- 工作组管理（创建/编辑/删除）
- Actor 管理（添加/启动/停止/编辑/删除）
- 消息发送（@提及自动补全）
- 消息回复（引用显示）
- 内嵌终端（xterm.js）
- 上下文面板（vision/sketch/tasks）
- 设置面板（自动化配置）
- IM Bridge 配置

### 主题系统

- 浅色/深色/跟随系统
- CSS 变量定义所有颜色
- 终端颜色自动适配

### 远程访问

推荐方案：

- **Cloudflare Tunnel + Cloudflare Access（推荐）**
  - 最佳体验：直接从手机浏览器访问
  - 强烈建议使用 Access 进行登录保护
  - 快速（临时 URL）：`cloudflared tunnel --url http://127.0.0.1:8848`
  - 稳定（自定义域名）：使用 `cloudflared tunnel create/route/run`

- **Tailscale（VPN）**
  - 清晰的安全边界（Tailnet ACL）
  - 建议仅绑定 tailnet IP：`CCCC_WEB_HOST=$TAILSCALE_IP cccc`

## 多运行时支持

### 支持的运行时

| 运行时 | 命令 | 描述 |
|--------|------|------|
| amp | `amp` | Amp |
| auggie | `auggie` | Auggie (Augment CLI) |
| claude | `claude` | Claude Code |
| codex | `codex` | Codex CLI |
| cursor | `cursor-agent` | Cursor CLI |
| droid | `droid` | Droid |
| gemini | `gemini` | Gemini CLI |
| kilocode | `kilocode` | Kilo Code CLI |
| neovate | `neovate` | Neovate Code |
| opencode | `opencode` | OpenCode |
| copilot | `copilot` | GitHub Copilot CLI |
| custom | 自定义 | 任意命令 |

### 安装配置命令

```bash
cccc setup --runtime claude   # 配置 MCP（自动）
cccc setup --runtime codex
cccc setup --runtime droid
cccc setup --runtime amp
cccc setup --runtime auggie
cccc setup --runtime neovate
cccc setup --runtime gemini
cccc setup --runtime cursor   # 打印配置指引（手动）
cccc setup --runtime kilocode # 打印配置指引（手动）
cccc setup --runtime opencode
cccc setup --runtime copilot
cccc setup --runtime custom
```

### 运行时检测

```bash
cccc doctor        # 环境检查 + 运行时检测
cccc runtime list  # 列出可用运行时（JSON）
```
