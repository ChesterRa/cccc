# CCCC vNext — Features

> 说明：本页属于 vNext 历史设计快照，可能滞后于当前实现。当前行为以 `docs/reference/*` 与 `docs/standards/*` 为准。

## 1. IM 风格消息机制

### 1.1 核心契约

- 消息是一等公民：发出去就落地，所有人可见
- 已读是显式的：agent 调用 MCP 标记
- 回复/引用是结构化的：`reply_to` + `quote_text`
- @mention 是精准投递

### 1.2 消息发送

```bash
# CLI
cccc send "Hello" --to @all
cccc reply <event_id> "Reply text"

# MCP
cccc_message_send(text="Hello", to=["@all"])
cccc_message_reply(reply_to="evt_xxx", text="Reply")
```

### 1.3 已读机制

- Agent 调用 `cccc_inbox_mark_read(event_id)` 标记已读
- 已读是累积的：标记 X 表示 X 及之前都已读
- 游标存储在 `state/read_cursors.json`

### 1.4 投递机制

```
消息写入 ledger
    ↓
daemon 解析 to 字段
    ↓
对每个目标 actor：
    ├─ PTY running → 注入终端
    └─ 否则 → 留在 inbox
    ↓
等待 agent 调用 mark_read
```

投递格式：
```
[cccc] user → peer-a: 请帮我实现登录功能
[cccc] user → peer-a (reply to evt_abc): 好的，请继续
```

## 2. IM Bridge

### 2.1 设计原则

- **1 Group = 1 Bot**: 简单、隔离、易理解
- **显式订阅**: Chat 必须 `/subscribe` 后才能接收消息
- **Ports 是薄层**: 只做消息转发，daemon 是唯一状态源

### 2.2 支持平台

| 平台 | 状态 | Token 配置 |
|------|------|-----------|
| Telegram | ✅ 完成 | `token_env` |
| Slack | ✅ 完成 | `bot_token_env` + `app_token_env` |
| Discord | ✅ 完成 | `token_env` |
| Feishu | ✅ 完成 | `FEISHU_APP_ID` + `FEISHU_APP_SECRET` |
| DingTalk | ✅ 完成 | `DINGTALK_APP_KEY` + `DINGTALK_APP_SECRET` |

### 2.3 配置

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

# Feishu 使用环境变量（App ID/Secret）
# export FEISHU_APP_ID=xxx
# export FEISHU_APP_SECRET=xxx
im:
  platform: feishu

# DingTalk 使用环境变量（App Key/Secret；可选 Robot Code）
# export DINGTALK_APP_KEY=xxx
# export DINGTALK_APP_SECRET=xxx
# export DINGTALK_ROBOT_CODE=xxx  # optional
im:
  platform: dingtalk
```

### 2.4 IM 命令

| 命令 | 说明 |
|------|------|
| 直接发消息 | 发给所有 agents |
| `@<actor> 消息` | 发给指定 actor |
| `/subscribe` | 订阅，开始接收消息 |
| `/unsubscribe` | 取消订阅 |
| `/verbose` | 切换详细模式 |
| `/status` | 显示 Group 状态 |
| `/pause` / `/resume` | 暂停/恢复消息投递 |
| `/help` | 显示帮助 |

### 2.5 CLI 命令

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
cccc im stop
cccc im status
cccc im logs -f
```

## 3. Agent Guidance

### 3.1 信息层次

```
系统提示 (薄层)
├── 你是谁：Actor ID、角色
├── 你在哪：Working Group、Scope
└── 你能做什么：MCP 工具列表 + 关键提醒（参见 cccc_help）

MCP Tools (权威规程 + 执行接口)
├── cccc_help：操作指南（规程）
├── cccc_project_info：获取 PROJECT.md
├── cccc_inbox_list / cccc_inbox_mark_read：收件箱
└── cccc_message_send / cccc_message_reply：发送/回复

Ledger (完整记忆)
└── 所有历史消息、事件
```

### 3.2 核心原则

- **Do**: 一个权威规程（`cccc_help`）
- **Do**: 内核强约束（RBAC 由 daemon 执行）
- **Do**: 极短启动握手（Bootstrap）
- **Don't**: 三套文案各写一遍

### 3.3 Agent 标准工作流

```
1. 收到 SYSTEM 注入 → 知道自己是谁
2. 调用 cccc_inbox_list → 获取未读消息
3. 处理消息 → 执行任务
4. 调用 cccc_inbox_mark_read → 标记已读
5. 调用 cccc_message_reply → 回复结果
6. 等待下一条消息
```

## 4. Automation

### 4.1 保留的机制

| 机制 | 配置项 | 默认值 | 说明 |
|------|--------|--------|------|
| Nudge | `nudge_after_seconds` | 300s | 未读消息超时提醒 |
| Actor idle | `actor_idle_timeout_seconds` | 600s | Actor 空闲通知 foreman |
| Keepalive | `keepalive_delay_seconds` | 120s | Next: 声明后提醒 |
| Silence | `silence_timeout_seconds` | 600s | 群聊静默通知 foreman |

### 4.2 投递限流

| 配置项 | 默认值 | 说明 |
|--------|--------|------|
| `min_interval_seconds` | 60s | 连续投递最小间隔 |

### 4.3 已移除的机制

| 机制 | 原因 |
|------|------|
| self_check | Foreman 观察并决定 |
| system_refresh | Actor 可通过 MCP 获取 |
| Auto-Compact | 各 CLI 自带机制 |
| Policy Filter | 通过提示词约束 |

## 5. Web UI

### 5.1 Agent-as-Tab 模式

- 每个 agent 是一个 tab
- Chat tab + Agent tabs
- 点击 tab 切换视图
- Mobile: 滑动切换

### 5.2 主要功能

- Group 管理（创建/编辑/删除）
- Actor 管理（添加/启动/停止/编辑/删除）
- 消息发送（@mention 自动补全）
- 消息回复（引用显示）
- 终端嵌入（xterm.js）
- Context 面板（vision/sketch/tasks）
- Settings 面板（automation 配置）
- IM Bridge 配置

### 5.3 主题系统

- Light / Dark / System
- CSS 变量定义所有颜色
- 终端颜色自适应

### 5.4 远程访问（手机随时随地）

当前阶段只提供**配置说明**（Web UI 的 Settings 弹窗里有 “Remote Access” 指南），CCCC 暂不内置 VPN/Tunnel 管理。

推荐方案：

- **Cloudflare Tunnel + Cloudflare Access（推荐）**
  - 体验最好：手机浏览器直接访问，无需安装 VPN app
  - 强烈建议用 Access 做登录保护（Web UI 属于高权限入口）
  - Quick（临时 URL）：`cloudflared tunnel --url http://127.0.0.1:8848`
  - Stable（自定义域名）：使用 `cloudflared tunnel create/route/run`，将域名指向本机 `127.0.0.1:8848`

- **Tailscale（VPN）**
  - 安全边界清晰（Tailnet ACL）
  - 推荐只绑定到 tailnet IP：`TAILSCALE_IP=$(tailscale ip -4)` 后用 `CCCC_WEB_HOST=$TAILSCALE_IP cccc`
  - 手机访问：`http://<TAILSCALE_IP>:8848/ui/`

## 6. Multi-Runtime 支持

### 6.1 支持的 Runtime

| Runtime | 命令 | 说明 |
|---------|------|------|
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

### 6.2 Setup 命令

```bash
cccc setup --runtime claude   # 配置 MCP（auto）
cccc setup --runtime codex
cccc setup --runtime droid
cccc setup --runtime amp
cccc setup --runtime auggie
cccc setup --runtime neovate
cccc setup --runtime gemini
cccc setup --runtime cursor   # prints config guidance (manual)
cccc setup --runtime kilocode # prints config guidance (manual)
cccc setup --runtime opencode
cccc setup --runtime copilot
cccc setup --runtime custom
```

### 6.3 Runtime 检测

```bash
cccc doctor        # 环境检查 + runtime 检测
cccc runtime list  # 列出可用 runtime (JSON)
```

---

详细设计历史见 `archive/` 目录
