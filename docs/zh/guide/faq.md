# 常见问题

关于 CCCC 的常见问题解答。

## 安装与设置

### 如何安装 CCCC？

```bash
# 从 PyPI 安装
pip install -U cccc-pair

# 从 TestPyPI 安装（显式 RC 测试）
pip install -U --pre \
  --index-url https://test.pypi.org/simple \
  --extra-index-url https://pypi.org/simple \
  cccc-pair

# 从源码安装
git clone https://github.com/ChesterRa/cccc
cd cccc
pip install -e .
```

### 如何从旧版本（0.3.x）升级？

你必须先卸载旧版本：

```bash
# pipx 用户
pipx uninstall cccc-pair

# pip 用户
pip uninstall cccc-pair

# 移除残留的可执行文件
rm -f ~/.local/bin/cccc ~/.local/bin/ccccd
```

然后安装新版本。注意 0.4.x 与 0.3.x 的命令结构完全不同。

### 系统要求是什么？

- Python 3.9+
- macOS、Linux 或 Windows（Windows 上推荐 WSL 以支持 PTY）
- 至少一个支持的 Agent 运行时 CLI

### 如何检查 CCCC 是否正常工作？

```bash
cccc doctor
```

这会检查 Python 版本、可用的运行时和 Daemon 状态。

## Agent 相关

### 支持哪些 AI Agent？

- Claude Code (`claude`)
- Codex CLI (`codex`)
- GitHub Copilot CLI (`copilot`)
- Droid (`droid`)
- OpenCode (`opencode`)
- Gemini CLI (`gemini`)
- Amp (`amp`)
- Auggie (`auggie`)
- Cursor (`cursor`)
- Kilocode (`kilocode`)
- Neovate (`neovate`)
- Custom（任意命令）

### Foreman 和 Peer 有什么区别？

- **Foreman**：第一个启用的 actor。负责协调工作，接收系统通知，可以管理其他 actor。
- **Peer**：独立的专家。有自己的判断力，只能管理自己。

### 如何添加自定义 Agent？

```bash
cccc actor add my-agent --runtime custom --command "my-custom-cli"
```

### Agent 无法启动？

1. 检查终端标签页的错误消息
2. 验证 MCP 已配置：`cccc setup --runtime <name>`
3. 确保 CLI 已安装且在 PATH 中
4. 尝试：`cccc actor restart <actor_id>`

## 消息相关

### 如何向特定 Agent 发送消息？

```bash
cccc send "请执行 X" --to agent-name
```

或在 Web UI 中，在消息中输入 `@agent-name`。

### Agent 没有回复我的消息？

1. 检查 agent 是否正在运行（Web UI 中的绿色指示灯）
2. 检查收件箱：`cccc inbox --actor-id <agent-id>`
3. 查看终端标签页的错误信息
4. 尝试重启该 agent

### 已读回执是如何工作的？

Agent 调用 `cccc_inbox_mark_read` 来标记消息为已读。这是累积的——标记消息 X 意味着 X 之前的所有消息都已读。

## 远程访问

### 如何从手机访问 CCCC？

**选项 1：Cloudflare Tunnel**
```bash
cloudflared tunnel --url http://127.0.0.1:8848
```

**选项 2：IM Bridge**
```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
```

**选项 3：Tailscale**
```bash
CCCC_WEB_HOST=$(tailscale ip -4) cccc
```

### 暴露 Web UI 安全吗？

始终设置认证 token：
```bash
export CCCC_WEB_TOKEN="your-secret-token"
cccc
```

使用 Cloudflare Access 或 Tailscale 增加安全层。

## 性能

### CCCC 占用多少资源？

- Daemon：极少（Python 异步）
- Web UI：标准 React 应用
- Agent：取决于运行时

### Ledger 文件越来越大

CCCC 支持快照/压缩。大型 blob 单独存储在 `blobs/` 目录中。

### 如何降低消息延迟？

1. 确保 agent 已经在运行
2. 使用特定的 @提及 而非广播
3. 保持 Daemon 运行（不要频繁重启）

## 故障排除

### Daemon 无法启动

```bash
cccc daemon status  # 检查是否已在运行
cccc daemon stop    # 停止现有实例
cccc daemon start   # 重新启动
```

### 端口 8848 被占用

```bash
CCCC_WEB_PORT=9000 cccc
```

### MCP 不工作

```bash
cccc setup --runtime <name>  # 重新运行设置
cccc doctor                  # 检查配置
```

### Web UI 无法加载

1. 检查 Daemon 是否在运行：`cccc daemon status`
2. 检查端口：http://127.0.0.1:8848/
3. 检查浏览器控制台的错误
4. 尝试其他浏览器

## 概念

### 什么是工作组（Working Group）？

工作组就像一个带有执行能力的 IM 群聊。它包括：
- 一个追加写入的 Ledger（消息历史）
- 一个或多个 Actor（agent）
- 可选的 Scope（项目目录）

### 什么是 Ledger？

Ledger 是一个追加写入的事件流，存储所有消息、状态变更和决策。它是工作组的唯一真实来源。

### 什么是 MCP？

MCP（Model Context Protocol，模型上下文协议）是 Agent 与 CCCC 交互的方式。它提供了丰富的工具接口，用于消息通信、上下文管理、自动化和系统控制。

### 什么是 Scope？

Scope 是附加到工作组的项目目录。Agent 在 Scope 内工作，事件归属于 Scope。
