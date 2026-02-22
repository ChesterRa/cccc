# Discord 设置

将你的 CCCC 工作组连接到 Discord，实现社区访问。

## 概览

Discord 集成适合：

- 开发者社区
- 开源项目
- 游戏和爱好群组
- 公开协作

## 前提条件

- 一个拥有管理员权限的 Discord 服务器
- CCCC 已安装并运行

## 第一步：创建 Discord 应用

1. 前往 [Discord Developer Portal](https://discord.com/developers/applications)
2. 点击 **New Application**
3. 输入名称（例如 "CCCC Bot"）
4. 接受条款并点击 **Create**

## 第二步：创建 Bot

1. 在你的应用中，进入侧边栏的 **Bot**
2. 点击 **Add Bot**
3. 点击 **Yes, do it!** 确认

### 配置 Bot 设置

在 Bot 部分：

| 设置 | 推荐值 |
|------|--------|
| Public Bot | OFF（除非你希望他人添加它） |
| Requires OAuth2 Code Grant | OFF |
| Presence Intent | OFF |
| Server Members Intent | OFF |
| Message Content Intent | **ON**（必需！） |

::: warning 重要
**Message Content Intent** 必须启用，否则 bot 无法读取消息。
:::

## 第三步：获取 Bot Token

1. 在 **Bot** 部分，点击 **Reset Token**
2. 确认并复制新 token
3. **安全保存此 token**

::: danger 安全警告
永远不要分享你的 bot token。如果泄露，请立即重新生成。
:::

## 第四步：设置 Bot 权限

1. 进入 **OAuth2** → **URL Generator**
2. 在 **Scopes** 下选择：
   - `bot`
   - `applications.commands`（可选，用于斜杠命令）

3. 在 **Bot Permissions** 下选择：

| 权限 | 用途 |
|------|------|
| Read Messages/View Channels | 查看频道和消息 |
| Send Messages | 回复用户 |
| Send Messages in Threads | 在线程中回复 |
| Embed Links | 富消息格式 |
| Attach Files | 分享文件 |
| Read Message History | 访问对话历史 |
| Add Reactions | 添加表情回应 |

4. 复制底部生成的 URL

## 第五步：将 Bot 添加到服务器

1. 在浏览器中打开第四步的 URL
2. 从下拉菜单中选择你的服务器
3. 点击 **Continue**
4. 审查权限并点击 **Authorize**
5. 完成验证码

## 第六步：设置环境变量

```bash
# 添加到你的 shell 配置文件
export DISCORD_BOT_TOKEN="your-bot-token-here"
```

## 第七步：配置 CCCC

### 选项 A：通过 Web UI

1. 打开 CCCC Web UI：`http://127.0.0.1:8848/`
2. 进入 **设置**（顶栏的齿轮图标）
3. 导航到 **IM Bridge** 部分
4. 选择 **Discord** 作为平台
5. 输入你的凭据：
   - **Token 环境变量**：`DISCORD_BOT_TOKEN`
6. 点击 **保存**

### 选项 B：通过 CLI

```bash
cccc im set discord --token-env DISCORD_BOT_TOKEN
```

两种方式都会保存到 `group.yaml`：

```yaml
im:
  platform: discord
  token_env: DISCORD_BOT_TOKEN
```

## 第八步：启动 Bridge

```bash
cccc im start
```

## 第九步：在 Discord 中订阅

1. 在 bot 有权限的频道中
2. 发送 `/subscribe`
3. 你应该会收到确认消息

## 使用方法

### 向 Agent 发送消息

在频道中，先 @提及 bot，然后使用 `/send` 命令：

```
@YourBotName /send 请审查最新的 Pull Request
```

在与 bot 的私信中，可以直接使用 `/send`：

```
/send 请审查最新的 Pull Request
```

::: warning 注意
- 在频道中，必须先 @提及 bot 才能路由消息
- @提及 bot 后，纯文本会被视为隐式发送给 foreman
- 需要指定 `@all` 或 `@peers` 等收件人时使用 `/send`
:::

### 指定特定 Agent

使用 `@提及` 语法配合 `/send` 命令：

```
/send @foreman 协调发布工作
/send @tester 运行集成测试
/send @all 请更新一下状态
```

### 接收消息

订阅后，你会自动收到：
- Agent 的回复
- 状态更新
- 错误通知

使用 `/verbose` 切换是否接收 agent 之间的消息。

### 线程支持

创建线程进行聚焦讨论。CCCC 会跟踪线程上下文。

### 文件附件

在消息中附加文件。文件会存储到 CCCC 的 blob 存储中，然后转发给 agent。

### 嵌入消息

CCCC 会在适当时使用 Discord 嵌入格式（Embed）来提高可读性。

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

## 斜杠命令（可选）

注册应用命令以获得更好的用户体验：

1. 在 Developer Portal 中进入你的应用
2. 导航到 **Bot** → **Interactions Endpoint URL**
3. 或使用 Discord API 注册命令

示例斜杠命令结构：
```
/cccc send <消息>
/cccc status
/cccc agents
```

## 故障排除

### "Missing Access" 错误

Bot 缺少权限：

1. 在服务器设置中检查 bot 的角色
2. 确保角色具有必要的权限
3. 验证频道特定的权限

### "Missing Intent" 错误

启用 Message Content Intent：

1. 进入 Developer Portal → 你的应用 → Bot
2. 启用 **Message Content Intent**
3. 保存更改
4. 重启 Bridge

### Bot 显示离线

1. 检查 Bridge 是否运行中：
   ```bash
   cccc im status
   ```

2. 验证 token：
   ```bash
   cccc im logs -f
   ```

3. 如果需要，重新生成 token

### 频率限制

Discord 有严格的频率限制：
- 降低消息发送频率
- 使用 `/verbose off` 减少流量
- 考虑批量更新

## 高级配置

### 指定频道

限制 bot 到特定频道：

```yaml
im:
  platform: discord
  token_env: DISCORD_BOT_TOKEN
  allowed_channels:
    - 123456789012345678  # 频道 ID
```

获取频道 ID：在 Discord 设置中启用开发者模式，右键点击频道 → 复制 ID。

### 活动状态

设置 bot 的状态消息：

```yaml
im:
  platform: discord
  token_env: DISCORD_BOT_TOKEN
  activity:
    type: watching  # playing, streaming, listening, watching
    name: "for commands"
```

### 多服务器

一个 bot 可以服务多个服务器。每个服务器需要：
1. 通过 OAuth URL 添加 bot
2. 在所需的频道中订阅

## 安全注意事项

- 保管好 bot token
- 将 bot 权限限制在所需范围内
- 使用频道权限限制访问
- 定期审查服务器角色
- 考虑为你的服务器设置验证级别
- 谨慎使用公共 bot — 任何人都可以添加它们
