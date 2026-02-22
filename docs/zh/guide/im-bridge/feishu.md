# 飞书（Lark）设置

将你的 CCCC 工作组连接到飞书，实现企业协作。

## 概览

飞书（也称 Lark 国际版）适合：

- 中国企业
- 已在使用飞书的团队
- 字节跳动生态用户

## 前提条件

- 拥有管理员权限的飞书企业账号
- CCCC 已安装并运行

## 第一步：创建应用

1. 前往[飞书开放平台](https://open.feishu.cn/app)
2. 点击 **创建自建应用**
3. 填写应用信息：
   - 应用名称（例如 "CCCC Bot"）
   - 应用描述
   - 应用图标
4. 点击 **创建**

## 第二步：配置权限

1. 进入 **权限管理**
2. 点击 **添加权限**
3. 在搜索框中搜索 `im:message`
4. 选择 **应用权限** 标签
5. 点击 **全选** 选中所有 `im:message` 相关权限
6. 同时搜索 `im:chat:readonly` 并启用（用于显示聊天标题）
7. 点击 **确认并申请**

![飞书权限配置](/images/feishu-permissions.png)

::: tip 所需权限
| 权限 | 用途 |
|------|------|
| `im:message`（全部） | 发送和接收消息 |
| `im:chat:readonly` | 显示聊天/群组标题（可选，缺失时回退到聊天 ID） |
:::

## 第三步：配置 CCCC

1. 在应用管理后台，进入 **凭证与基础信息**
2. 复制 **App ID** 和 **App Secret**

::: warning 安全提示
请妥善保管 App Secret。永远不要将其提交到版本控制。
:::

### 选项 A：通过 Web UI

1. 打开 CCCC Web UI：`http://127.0.0.1:8848/`
2. 进入 **设置**（顶栏的齿轮图标）
3. 导航到 **IM Bridge** 标签页
4. 选择 **飞书/Lark** 作为平台
5. 输入你的凭据：
   - **App ID**：你的飞书 App ID（例如 `cli_a9e92055a5b89bc6`）
   - **App Secret**：你的飞书 App Secret
6. 点击 **保存配置**

![CCCC IM Bridge 配置](/images/cccc-im-bridge-feishu.png)

### 选项 B：通过 CLI

首先设置环境变量：

```bash
export FEISHU_APP_ID="cli_your_app_id"
export FEISHU_APP_SECRET="your_app_secret"
```

然后配置 CCCC：

```bash
cccc im set feishu \
  --app-key-env FEISHU_APP_ID \
  --app-secret-env FEISHU_APP_SECRET
```

两种方式都会保存到 `group.yaml`：

```yaml
im:
  platform: feishu
  feishu_app_id_env: FEISHU_APP_ID
  feishu_app_secret_env: FEISHU_APP_SECRET
```

## 第四步：启动 Bridge

### 通过 Web UI

在 IM Bridge 设置中点击 **保存配置** 按钮。Bridge 会自动启动并显示 **运行中** 状态。

### 通过 CLI

```bash
cccc im start
```

验证是否运行中：

```bash
cccc im status
```

## 第五步：启用长连接（推荐）

::: warning 前提条件
在配置事件订阅之前，CCCC IM Bridge 必须已经在运行。启用长连接后，CCCC 可以通过长连接接收事件（此模式不需要公开的回调 URL）。
:::

1. 返回[飞书开放平台](https://open.feishu.cn/app)
2. 导航到你的应用 → **事件与回调**
3. 在 **事件配置** 标签页，找到 **订阅方式**
4. 选择 **使用长连接接收事件**（推荐）
5. 点击 **保存**

![飞书事件配置 - 长连接](/images/feishu-event-config.png)

## 第六步：配置事件订阅

1. 在 **事件订阅** 中，点击 **添加事件**
2. 订阅以下事件：

| 事件 | 用途 |
|------|------|
| `im.message.receive_v1` | 接收消息 |
| `im.message.message_read_v1` | 已读回执 |

3. 点击 **保存**

## 第七步：启用机器人

::: tip 为什么需要这一步？
你必须启用机器人能力，这样用户才能在应用发布后找到并与机器人聊天。
:::

1. 在侧边栏中，进入 **应用能力** → **机器人**
2. 在 **机器人设置** 中，填写 **入门指引** 字段（例如 "cccc im"）
3. 点击 **保存**

![飞书机器人设置](/images/feishu-bot-setting.png)

## 第八步：发布应用

1. 进入 **版本管理与发布**
2. 创建新版本
3. 提交审核（企业内部应用可能自动审批）
4. 审批通过后，发布到你的组织

## 第九步：在飞书中订阅

1. 在飞书应用中找到机器人
2. 开始聊天或添加到群组
3. 发送 `/subscribe`
4. 确认订阅

## 使用方法

### 向 Agent 发送消息

飞书支持两种发送消息的方式：

**在群聊中** — @提及机器人并直接输入消息：

```
@YourBotName 请实现登录功能
```

或使用显式的 `/send` 命令指定收件人：

```
@YourBotName /send @all 请更新一下状态
```

**在私聊中** — 直接输入消息：

```
请实现登录功能
```

::: tip 隐式发送
当你 @提及机器人（在群聊中）或发送私聊消息时，纯文本会自动视为 `/send` 给 foreman。你只需要在指定其他 agent（如 `@all` 或 `@peers`）时使用显式的 `/send` 命令。
:::

### 指定特定 Agent

使用 `@提及` 语法配合 `/send` 命令：

```
/send @backend 检查 API 端点
/send @all 请更新一下状态
/send @peers 请审查这个 PR
```

### 接收消息

订阅后，你会自动收到：
- Agent 的回复
- 状态更新
- 错误通知

使用 `/verbose` 切换是否接收 agent 之间的消息。

### 文件分享

在消息中附加文件。飞书文件会被下载并存储到 CCCC 的 blob 存储中，然后转发给 agent。

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

### "Invalid app_id" 错误

1. 在飞书开放平台验证你的 App ID
2. 检查环境变量是否设置正确
3. 确保应用已发布且审批通过

### "Permission denied" 错误

1. 进入 **权限管理**
2. 添加缺失的权限
3. 提交新版本进行审批

### 机器人无法接收消息

1. 检查事件订阅是否已配置
2. 验证应用是否已安装到聊天中
3. 确保应用版本已发布
4. 确保 CCCC IM Bridge 正在运行

### Token 过期

CCCC 会自动刷新 token，但如果问题持续：

```bash
cccc im stop
cccc im start
```

## 高级配置

CCCC 目前支持：

- 通过 REST API 发送消息
- 通过长连接接收消息（Python `lark-oapi`）

Webhook 回调（开发者服务器 URL）、消息卡片和加密设置目前不通过 CCCC 配置。

## 安全注意事项

- 将凭据存储在环境变量或密钥管理器中
- 使用最小必要权限
- 定期审查应用访问权限
- 为敏感通信启用平台加密（可选）
- 通过飞书管理后台审计应用使用情况
