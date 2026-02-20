# 工作流示例

使用 CCCC 协调 AI Agent 的常见模式。

## 单人开发 + 单 Agent

最简配置：一个 Agent 辅助你开发。

### 配置

```bash
cd /your/project
cccc attach .
cccc actor add assistant --runtime claude
cccc
```

### 工作流

1. 打开 Web UI：http://127.0.0.1:8848/
2. 启动 Agent
3. 通过聊天发送任务："实现登录功能"
4. 在终端标签页中观察 Agent 工作
5. 审查变更并提供反馈

## 双 Agent 配对编程

一个 Agent 负责实现，另一个负责评审。

### 配置

```bash
cccc actor add implementer --runtime claude
cccc actor add reviewer --runtime codex
cccc group start
```

### 工作流

1. 向 `@implementer` 发送实现任务
2. 完成后，让 `@reviewer` 评审代码
3. 根据评审反馈迭代

### 技巧

- 评审者可以发现 Bug 并提出改进建议
- 使用不同的运行时获得多样化视角
- 保持任务聚焦且具体

## 多 Agent 团队

复杂项目可使用多个专业化 Agent。

### 配置示例

```bash
cccc actor add architect --runtime claude    # 架构决策
cccc actor add frontend --runtime codex      # UI 实现
cccc actor add backend --runtime droid       # API 实现
cccc actor add tester --runtime copilot      # 测试
```

### 协调

- 第一个启用的 Actor（architect）成为 Foreman
- Foreman 协调各 Peer 的工作
- 使用 @mention 将任务定向到特定 Agent
- 使用 Context 面板维护共享理解

### 最佳实践

- 为每个 Agent 定义清晰的职责
- 使用里程碑跟踪进度
- 定期检查以确保对齐

## 通过手机远程监控

随时随地监控和控制你的 Agent。

### 配置方式

**方式 1：Cloudflare Tunnel（推荐）**

```bash
# 快速方式（临时 URL）
cloudflared tunnel --url http://127.0.0.1:8848

# 稳定方式（自定义域名）
cloudflared tunnel create cccc
cloudflared tunnel route dns cccc cccc.yourdomain.com
cloudflared tunnel run cccc
```

**方式 2：IM Bridge**

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
```

然后通过 Telegram 应用：
- 向 Agent 发送消息
- 接收状态更新
- 使用斜杠命令控制工作组

### 工作流

1. 设置远程访问
2. 让 Agent 在开发机上持续运行
3. 从手机监控和发送命令
4. 接收重要事件通知

## 通宵任务

无人值守运行长时间任务。

### 配置

1. 定义明确的成功标准
2. 设置 IM Bridge 用于通知
3. 配置自动化超时

### 示例

```bash
# 配置通知
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start

# 启动任务
cccc send "请重构整个认证模块。每小时报告一次进度。" --to @foreman
```

### 监控

- IM Bridge 将更新推送到你的手机
- 需要时通过 Web UI 查看进度
- Agent 在完成或出错时发出通知
