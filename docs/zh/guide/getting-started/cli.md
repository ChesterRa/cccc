# CLI 快速上手

通过命令行开始使用 CCCC。

## 第 1 步：进入项目目录

```bash
cd /path/to/your/project
```

## 第 2 步：创建工作组

```bash
cccc attach .
```

这会将当前目录绑定为"Scope"并创建一个工作组。

## 第 3 步：为运行时配置 MCP

```bash
cccc setup --runtime claude   # 或 codex, droid, opencode, copilot
```

这会配置 MCP（Model Context Protocol），使 Agent 能够与 CCCC 交互。

## 第 4 步：添加第一个 Agent

```bash
cccc actor add assistant --runtime claude
```

第一个启用的 Actor 自动成为"Foreman"（协调者）。

## 第 5 步：启动 Agent

```bash
cccc group start
```

或启动特定 Agent：

```bash
cccc actor start assistant
```

## 第 6 步：发送消息

```bash
cccc send "你好！请自我介绍一下。"
```

## 第 7 步：查看响应

实时查看 Ledger：

```bash
cccc tail -f
```

或查看收件箱：

```bash
cccc inbox --actor-id assistant
```

## 添加更多 Agent

添加第二个 Agent：

```bash
cccc actor add reviewer --runtime codex
cccc actor start reviewer
```

发送给特定 Agent：

```bash
cccc send "请实现这个功能" --to assistant
cccc send "请评审代码" --to reviewer
cccc send "请汇报状态" --to @all
```

## 回复消息

```bash
# 从 cccc tail 找到 event ID
cccc reply evt_abc123 "谢谢，看起来不错！"
```

## 常用命令

### 工作组管理

```bash
cccc groups              # 列出所有工作组
cccc use <group_id>      # 切换工作组
cccc active              # 显示当前工作组
cccc group show <group_id> # 显示工作组元数据
cccc group start         # 启动所有 Agent
cccc group stop          # 停止所有 Agent
```

### Actor 管理

```bash
cccc actor list                    # 列出 Actor
cccc actor add <id> --runtime <r>  # 添加 Actor
cccc actor start <id>              # 启动 Actor
cccc actor stop <id>               # 停止 Actor
cccc actor restart <id>            # 重启 Actor
cccc actor remove <id>             # 移除 Actor
```

### 消息

```bash
cccc send "message"                # 不加 --to：使用默认接收者策略（默认：foreman）
cccc send "msg" --to @all          # 显式广播
cccc send "msg" --to assistant     # 发送给特定 Actor
cccc reply <event_id> "response"   # 回复消息
cccc inbox --actor-id assistant    # 查看特定 Actor 的未读消息
cccc tail -n 50                    # 最近的事件
cccc tail -f                       # 实时追踪事件
```

### Daemon 控制

```bash
cccc daemon status    # 检查状态
cccc daemon start     # 启动 Daemon
cccc daemon stop      # 停止 Daemon
```

## 启动 Web UI（可选）

使用 CLI 的同时，也可以打开 Web UI：

```bash
cccc   # 启动 Daemon + Web UI
```

或者仅启动 Web UI（Daemon 已在运行时）：

```bash
cccc web
```

访问 http://127.0.0.1:8848/

## 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `CCCC_HOME` | `~/.cccc` | 运行时目录 |
| `CCCC_WEB_PORT` | `8848` | Web UI 端口 |
| `CCCC_LOG_LEVEL` | `INFO` | 日志详细程度 |

## 工作流示例

```bash
# 设置
cd ~/projects/my-app
cccc attach .
cccc setup --runtime claude
cccc actor add dev --runtime claude

# 工作
cccc group start
cccc send "请实现用户认证功能"

# 监控
cccc tail -f

# 交互
cccc reply evt_123 "请使用 JWT Token"
cccc send "进度如何？" --to dev

# 清理
cccc group stop
```

## 故障排查

### Daemon 无法启动？

```bash
cccc daemon status
cccc daemon stop      # 停止卡住的实例
cccc daemon start
```

### Agent 没有响应？

```bash
# 检查 Agent 状态
cccc actor list

# 重启 Agent
cccc actor restart <actor_id>

# 检查 MCP 配置
cccc setup --runtime <name>
```

### 找不到工作组？

```bash
# 列出所有工作组
cccc groups

# 如需重新关联
cd /path/to/project
cccc attach .
```

## 下一步

- [工作流](/zh/guide/workflows) — 学习协作模式
- [CLI 参考](/zh/reference/cli) — 完整命令参考
- [IM Bridge](/zh/guide/im-bridge/) — 设置移动端访问
