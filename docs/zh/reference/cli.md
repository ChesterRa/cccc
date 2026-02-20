# CLI 命令参考

CCCC CLI 完整命令参考。

## 全局命令

### `cccc`

同时启动 daemon 和 Web UI。

```bash
cccc                    # 启动 daemon + Web UI
cccc --help             # 显示帮助
```

### `cccc doctor`

检查环境并诊断问题。

```bash
cccc doctor             # 完整环境检查
```

### `cccc runtime list`

列出可用的 Agent 运行时。

```bash
cccc runtime list       # 列出已检测到的运行时
cccc runtime list --all # 列出所有支持的运行时
```

## Daemon 命令

### `cccc daemon`

管理 CCCC daemon。

```bash
cccc daemon status      # 检查 daemon 状态
cccc daemon start       # 启动 daemon
cccc daemon stop        # 停止 daemon
```

注意：
- 如果 pid 文件对应的进程仍然存活但 IPC 无响应，`cccc daemon start` 会拒绝启动重复的 daemon。
- 此时请先运行 `cccc daemon stop`（或清理过期的运行时状态）再重试启动。

## 工作组命令

### `cccc attach`

创建或关联一个工作组。

```bash
cccc attach .           # 将当前目录关联为 scope
cccc attach /path/to/project
```

### `cccc groups`

列出所有工作组。

```bash
cccc groups             # 列出工作组
```

### `cccc use`

切换到不同的工作组。

```bash
cccc use <group_id>     # 切换工作组
```

### `cccc group`

管理当前工作组。

```bash
cccc group create --title "my-group"         # 创建工作组
cccc group show <group_id>                   # 显示工作组元数据
cccc group update --group <id> --title "..." # 更新标题/主题
cccc group use <group_id> .                  # 设置活跃 scope
cccc group start --group <id>                # 启动工作组的 actor
cccc group stop --group <id>                 # 停止工作组的 actor
cccc group set-state idle --group <id>       # 设置状态：active/idle/paused/stopped
cccc group detach-scope <scope_key> --group <id>
cccc group delete --group <id> --confirm <id>
```

## Actor 命令

### `cccc actor add`

向工作组添加新的 actor。

```bash
cccc actor add <actor_id> --runtime claude
cccc actor add <actor_id> --runtime codex
cccc actor add <actor_id> --runtime custom --command "my-agent"
```

选项：
- `--runtime`：Agent 运行时（claude、codex、droid 等）
- `--command`：自定义命令（用于 custom 运行时）
- `--runner`：运行器类型（pty 或 headless）
- `--title`：显示标题

### `cccc actor`

管理 actor。

```bash
cccc actor list                    # 列出 actor
cccc actor start <actor_id>        # 启动 actor
cccc actor stop <actor_id>         # 停止 actor
cccc actor restart <actor_id>      # 重启 actor
cccc actor remove <actor_id>       # 移除 actor
cccc actor update <actor_id> ...   # 更新 actor 设置
cccc actor secrets <actor_id> ...  # 管理运行时专用密钥
```

## 消息命令

### `cccc send`

发送消息。

```bash
cccc send "Hello"                  # 不指定 --to：使用默认接收者策略（默认：foreman）
cccc send "Hello" --to @all        # 显式广播
cccc send "Hello" --to @foreman    # 发送给 foreman
cccc send "Hello" --to peer-1      # 发送给特定 actor
```

### `cccc reply`

回复消息。

```bash
cccc reply <event_id> "Reply text"
```

### `cccc inbox`

查看收件箱。

```bash
cccc inbox --actor-id <id>         # 查看 actor 未读消息
cccc inbox --actor-id <id> --mark-read
```

### `cccc tail`

追踪 ledger。

```bash
cccc tail                          # 显示最近的事件
cccc tail -n 50                    # 显示最近 50 条事件
cccc tail -f                       # 持续追踪新事件
```

## IM Bridge 命令

### `cccc im`

管理 IM Bridge。

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im set slack --bot-token-env SLACK_BOT_TOKEN --app-token-env SLACK_APP_TOKEN
cccc im set discord --token-env DISCORD_BOT_TOKEN
cccc im set feishu --app-key-env FEISHU_APP_ID --app-secret-env FEISHU_APP_SECRET
cccc im set dingtalk --app-key-env DINGTALK_APP_KEY --app-secret-env DINGTALK_APP_SECRET --robot-code-env DINGTALK_ROBOT_CODE

cccc im start                      # 启动 IM bridge
cccc im stop                       # 停止 IM bridge
cccc im status                     # 检查 IM bridge 状态
cccc im logs                       # 查看 IM bridge 日志
cccc im logs -f                    # 持续追踪 IM bridge 日志
```

## 安装配置命令

### `cccc setup`

为 Agent 运行时配置 MCP。

```bash
cccc setup --runtime claude        # 自动配置 Claude Code
cccc setup --runtime codex         # 自动配置 Codex
cccc setup --runtime cursor        # 打印手动配置说明
```

## Web 命令

### `cccc web`

仅启动 Web UI（需要 daemon 已运行）。

```bash
cccc web                           # 启动 Web UI
cccc web --port 9000               # 自定义端口
```

## MCP 命令

### `cccc mcp`

启动 MCP 服务器（用于 Agent 集成）。

```bash
cccc mcp                           # 启动 MCP 服务器（stdio 模式）
```

## 环境变量

| 变量 | 默认值 | 描述 |
|------|--------|------|
| `CCCC_HOME` | `~/.cccc` | 运行时主目录 |
| `CCCC_WEB_HOST` | `127.0.0.1` | Web UI 绑定地址 |
| `CCCC_WEB_PORT` | `8848` | Web UI 端口 |
| `CCCC_WEB_TOKEN` | （无） | Web UI 认证令牌 |
| `CCCC_LOG_LEVEL` | `INFO` | 日志级别 |
