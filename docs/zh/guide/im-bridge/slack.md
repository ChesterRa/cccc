# Slack 设置

将你的 CCCC 工作组连接到 Slack，实现团队协作。

## 概览

Slack 集成使用 Socket Mode 进行实时消息传递，适合：

- 团队协作
- 企业环境
- 已有 Slack 工作区的团队

## 前提条件

- 一个拥有管理员权限的 Slack 工作区
- CCCC 已安装并运行

## 第一步：创建 Slack App

1. 前往 [Slack API Apps](https://api.slack.com/apps)
2. 点击 **Create New App**
3. 选择 **From scratch**
4. 输入应用名称（例如 "CCCC Bot"）
5. 选择你的工作区
6. 点击 **Create App**

## 第二步：启用 Socket Mode

Socket Mode 允许 bot 无需暴露公开 URL 即可接收事件。

1. 在应用设置中，进入 **Socket Mode**
2. 将 **Enable Socket Mode** 切换为 ON
3. 点击 **Generate** 创建应用级 token
4. 为其命名（例如 "cccc-socket-token"）
5. 添加权限范围 `connections:write`
6. 点击 **Generate**
7. **复制 token**（以 `xapp-` 开头）

::: tip
这是你的 App Token，用于 WebSocket 连接。
:::

## 第三步：配置 Bot 权限

1. 进入 **OAuth & Permissions**
2. 在 **Scopes** → **Bot Token Scopes** 下添加：

| 权限范围 | 用途 |
|----------|------|
| `chat:write` | 发送消息 |
| `channels:history` | 读取公共频道消息 |
| `groups:history` | 读取私有频道消息 |
| `im:history` | 读取私信 |
| `mpim:history` | 读取群组私信 |
| `files:read` | 读取共享文件 |
| `files:write` | 上传文件 |
| `users:read` | 读取用户信息 |

## 第四步：启用事件订阅

1. 进入 **Event Subscriptions**
2. 将 **Enable Events** 切换为 ON
3. 在 **Subscribe to bot events** 下添加：

| 事件 | 用途 |
|------|------|
| `message.channels` | 公共频道的消息 |
| `message.groups` | 私有频道的消息 |
| `message.im` | 私信 |
| `message.mpim` | 群组私信 |
| `app_mention` | bot 被 @提及时 |

4. 点击 **Save Changes**

## 第五步：安装到工作区

1. 进入 **OAuth & Permissions**
2. 点击 **Install to Workspace**
3. 审查权限并点击 **Allow**
4. **复制 Bot Token**（以 `xoxb-` 开头）

## 第六步：设置环境变量

```bash
# 添加到你的 shell 配置文件
export SLACK_BOT_TOKEN="xoxb-your-bot-token"
export SLACK_APP_TOKEN="xapp-your-app-token"
```

::: warning 需要两个 Token
Slack 需要两个 token：
- **Bot Token**（`xoxb-`）：用于 API 调用
- **App Token**（`xapp-`）：用于 Socket Mode 连接
:::

## 第七步：配置 CCCC

### 选项 A：通过 Web UI

1. 打开 CCCC Web UI：`http://127.0.0.1:8848/`
2. 进入 **设置**（顶栏的齿轮图标）
3. 导航到 **IM Bridge** 部分
4. 选择 **Slack** 作为平台
5. 输入你的凭据：
   - **Bot Token 环境变量**：`SLACK_BOT_TOKEN`
   - **App Token 环境变量**：`SLACK_APP_TOKEN`
6. 点击 **保存**

### 选项 B：通过 CLI

```bash
cccc im set slack \
  --bot-token-env SLACK_BOT_TOKEN \
  --app-token-env SLACK_APP_TOKEN
```

两种方式都会保存到 `group.yaml`：

```yaml
im:
  platform: slack
  bot_token_env: SLACK_BOT_TOKEN
  app_token_env: SLACK_APP_TOKEN
```

## 第八步：启动 Bridge

```bash
cccc im start
```

## 第九步：在 Slack 中订阅

1. 邀请 bot 到频道：
   ```
   /invite @your-bot-name
   ```
2. 在频道中发送 `/subscribe`
3. 你应该会收到确认消息

私信方式：
1. 在私信中找到 bot
2. 发送 `/subscribe`

## 使用方法

### 向 Agent 发送消息

在频道中，先 @提及 bot，然后使用 `/send` 命令：

```
@YourBotName /send 请实现用户认证模块
```

在与 bot 的私信中，可以直接使用 `/send`：

```
/send 请实现用户认证模块
```

::: warning 注意
- 在频道中，必须先 @提及 bot 才能路由消息
- @提及 bot 后，纯文本会被视为隐式发送给 foreman
- 需要指定 `@all` 或 `@peers` 等收件人时使用 `/send`
:::

### 指定特定 Agent

使用 `@提及` 语法配合 `/send` 命令（使用 CCCC 的语法，而非 Slack 的）：

```
/send @foreman 审查最新的提交
/send @backend-agent 修复 API 端点
/send @all 请更新一下状态
```

### 接收消息

订阅后，你会自动收到：
- Agent 的回复
- 状态更新
- 错误通知

使用 `/verbose` 切换是否接收 agent 之间的消息。

### 线程回复

在线程中回复以保持对话有序。CCCC 会保留线程上下文。

### 文件分享

在消息中附加文件。文件会上传到 CCCC 的 blob 存储中，然后转发给 agent。

## 命令参考

| 命令 | 描述 |
|------|------|
| `/subscribe` | 开始接收 CCCC 消息 |
| `/unsubscribe` | 停止接收消息 |
| `/send <消息>` | 发送给 foreman（默认） |
| `/send @<actor> <消息>` | 发送给指定 agent |
| `/send @all <消息>` | 发送给所有 agent |
| `/send @peers <消息>` | 发送给非 foreman 的 agent |
| `/status` | 显示工作组和 agent 状态 |
| `/pause` | 暂停投递 |
| `/resume` | 恢复投递 |
| `/verbose` | 切换详细模式 |
| `/help` | 显示帮助 |

## 故障排除

### "invalid_auth" 错误

Token 无效或已过期：

1. 进入 **OAuth & Permissions**
2. 点击 **Reinstall to Workspace**
3. 更新 `SLACK_BOT_TOKEN` 环境变量

### "missing_scope" 错误

添加所需的权限范围：

1. 进入 **OAuth & Permissions**
2. 在 **Bot Token Scopes** 下添加缺失的范围
3. 重新安装应用

### Bot 无法接收消息

1. 检查 Socket Mode 是否已启用
2. 验证 `SLACK_APP_TOKEN` 是否正确
3. 确保在 **Event Subscriptions** 中已订阅事件
4. 检查 bot 是否已被邀请到频道

### 连接断开

Socket Mode 连接可能偶尔断开。CCCC 会自动重连，但如果问题持续：

```bash
cccc im stop
cccc im start
```

## 高级配置

### 频道限制

限制 bot 响应的频道：

```yaml
im:
  platform: slack
  bot_token_env: SLACK_BOT_TOKEN
  app_token_env: SLACK_APP_TOKEN
  allowed_channels:
    - C01234567  # 频道 ID
    - C89012345
```

### 自定义 Bot 名称

显示名称在 Slack 中设置：

1. 进入 **App Home**
2. 在 **Your App's Presence in Slack** 下
3. 编辑 **Display Name**

## 安全注意事项

- Bot token 拥有广泛的访问权限 — 限制在必要的工作区
- 定期审查频道成员
- 考虑使用 Enterprise Grid 以获得额外控制
- 审计谁可以在你的工作区安装应用
