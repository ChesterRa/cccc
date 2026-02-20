# Telegram 设置

将你的 CCCC 工作组连接到 Telegram，实现移动端访问。

## 概览

Telegram 是最容易设置的平台，适合：

- 个人使用
- 快速原型验证
- 独立开发者

## 前提条件

- 一个 Telegram 账号
- CCCC 已安装并运行

## 第一步：创建 Bot

1. 打开 Telegram，搜索 `@BotFather`
2. 开始聊天并发送 `/newbot`
3. 按提示操作：
   - 选择显示名称（例如 "My CCCC Bot"）
   - 选择用户名（必须以 `bot` 结尾，例如 `my_cccc_bot`）
4. BotFather 会给你一个 token，类似：
   ```
   123456789:ABCdefGHIjklMNOpqrsTUVwxyz
   ```
5. **保存此 token** — 下一步会用到

::: tip 推荐：关闭群组隐私模式
如果你打算在群聊中使用 bot，请关闭群组隐私模式，让 bot 能看到所有消息：

1. 向 BotFather 发送 `/mybots`
2. 选择你的 bot → **Bot Settings** → **Group Privacy**
3. 设置为 **Disabled**
:::

## 第二步：配置 CCCC

### 选项 A：通过 Web UI（推荐）

1. 打开 CCCC Web UI：`http://127.0.0.1:8848/`
2. 进入 **设置**（顶栏的齿轮图标）
3. 导航到 **IM Bridge** 标签页
4. 选择 **Telegram** 作为平台
5. 输入你的 bot token：
   - 直接粘贴 token（例如 `123456789:ABCdefGHIjklMNOpqrsTUVwxyz`）
   - 或输入环境变量名（例如 `TELEGRAM_BOT_TOKEN`）
6. 点击 **保存配置**

![CCCC IM Bridge 配置](/images/cccc-im-bridge-telegram.png)

::: tip 安全最佳实践
在生产环境中，请将 token 存储在环境变量中，而非直接粘贴：

```bash
# 添加到你的 shell 配置文件（~/.bashrc、~/.zshrc 等）
export TELEGRAM_BOT_TOKEN="your-token-here"
```

然后在 Web UI 中输入 `TELEGRAM_BOT_TOKEN`。永远不要将 token 提交到 git。
:::

### 选项 B：通过 CLI

```bash
# 使用环境变量名
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN

# 验证配置
cccc im config
```

两种方式都会保存配置到工作组的 `group.yaml`：

```yaml
im:
  platform: telegram
  token_env: TELEGRAM_BOT_TOKEN
```

## 第三步：启动 Bridge 并订阅

### 启动 Bridge

**通过 Web UI**：点击 **保存配置** — Bridge 会自动启动并显示 **运行中** 状态。

**通过 CLI**：

```bash
cccc im start
```

验证是否运行中：

```bash
cccc im status
```

### 在 Telegram 中订阅

1. 打开 Telegram，找到你的 bot（按用户名搜索）
2. 开始与 bot 聊天
3. 发送 `/subscribe`
4. 你应该会收到确认消息

在群聊中：
1. 将 bot 添加到群组
2. 在群组中发送 `/subscribe`
3. 所有已订阅的聊天都会收到 CCCC 的消息

## 使用方法

### 向 Agent 发送消息

Telegram 支持两种发送消息的方式：

**在群聊中** — @提及 bot 并直接输入消息：

```
@YourBotName 请实现登录功能
```

或使用显式的 `/send` 命令：

```
@YourBotName /send @all 请更新一下状态
```

**在私聊中** — 直接输入消息：

```
请实现登录功能
```

::: tip 隐式发送
当你 @提及 bot（在群聊中）或发送私聊消息时，纯文本会自动视为 `/send` 给 foreman。你只需要在指定其他 agent（如 `@all` 或 `@peers`）时使用显式的 `/send` 命令。
:::

### 指定特定 Agent

使用 `@提及` 语法配合 `/send` 命令：

```
/send @foreman 请审查这个 PR
/send @peer-1 运行测试
/send @all 请更新一下状态
```

### 接收消息

订阅后，你会自动收到：
- Agent 的回复
- 状态更新
- 错误通知

使用 `/verbose` 切换是否接收 agent 之间的消息。

### 文件附件

在消息中附加文件。文件会被下载并存储到 CCCC 的 blob 存储中，然后转发给 agent。

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
| `/pause` | 暂停消息投递 |
| `/resume` | 恢复消息投递 |
| `/verbose` | 切换详细模式（查看所有 agent 消息） |
| `/help` | 显示可用命令 |

## 故障排除

### Bot 没有响应

1. 检查 Bridge 是否运行中：
   ```bash
   cccc im status
   ```

2. 查看日志中的错误：
   ```bash
   cccc im logs -f
   ```

3. 验证 token 是否正确 — 在 BotFather 中重新检查（`/mybots` → 选择 bot → **API Token**）

### "Unauthorized" 错误

你的 token 无效。从 BotFather 获取新的 token：

1. 向 BotFather 发送 `/mybots`
2. 选择你的 bot
3. 点击 **API Token** → **Revoke current token**
4. 在 CCCC 设置（Web UI）或环境变量中更新 token

### 消息未投递

1. 确保已发送 `/subscribe`
2. 检查 CCCC Daemon 是否在运行
3. 在 Web UI 中或通过 `cccc im status` 验证 Bridge 状态

### 频率限制

Telegram 有频率限制。如果你发送了很多消息：
- 消息可能会延迟
- 考虑使用 `/verbose` 关闭详细模式以减少流量

## 安全注意事项

- 保管好你的 bot token
- 考虑为你的 Telegram 账号启用两步验证
- 审查谁可以访问 bot 所在的聊天
- Bot 可以看到它所在群组中的所有消息
