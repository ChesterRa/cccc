# 钉钉设置

将你的 CCCC 工作组连接到钉钉，实现企业协作。

## 概览

钉钉适合：

- 中国企业
- 阿里巴巴生态用户
- 已在使用钉钉的团队

CCCC 使用钉钉 Stream 模式（持久 WebSocket 连接）接收消息，使用钉钉开放 API 发送消息。不需要公网 URL。

## 前提条件

- 拥有管理员权限的钉钉企业账号
- CCCC 已安装并运行

## 第一步：创建应用

1. 前往[钉钉开放平台](https://open.dingtalk.com/)
2. 使用企业管理员账号登录
3. 点击 **应用开发** → **企业内部开发**
4. 点击 **创建应用**
5. 填写：
   - 应用名称（例如 "CCCC Bot"）
   - 应用描述
   - 应用图标
6. 点击 **确认**

## 第二步：配置权限

1. 进入 **权限管理**
2. 申请以下权限：

| 权限 | 用途 |
|------|------|
| `Robot.SingleChat.ReadWrite` | 单聊机器人管理 |
| `qyapi_robot_sendmsg` | 机器人主动发送消息 |
| `qyapi_chat_read` | 读取群基本信息 |
| `qyapi_chat_manage` | 管理群聊（创建、更新、发送消息） |

3. 点击启用每个权限（企业内部应用无需审批）

## 第三步：启用机器人

1. 在 **应用能力** → **机器人**
2. 启用机器人能力
3. 配置机器人设置：
   - 机器人名称
   - 机器人头像

## 第四步：发布应用

1. 进入 **版本管理**
2. 创建新版本
3. 配置可见范围：
   - 全部员工
   - 指定部门
   - 指定用户
4. 发布版本

## 第五步：配置并启动 CCCC

1. 在应用管理中，进入 **凭证与基础信息**
2. 复制 **AppKey** 和 **AppSecret**
3. （可选）复制 **RobotCode**（如果在机器人设置中显示）——CCCC 有时可以在第一条入站消息后自动获取，但预先配置对附件发送更可靠

### 选项 A：通过 Web UI

1. 打开 CCCC Web UI：`http://127.0.0.1:8848/`
2. 进入 **设置**（顶栏的齿轮图标）
3. 导航到 **IM Bridge** 标签页
4. 选择 **钉钉** 作为平台
5. 输入你的凭据：
   - **App Key**：你的钉钉 AppKey
   - **App Secret**：你的钉钉 AppSecret
6. 点击 **保存配置** — Bridge 会自动启动并显示 **运行中** 状态

### 选项 B：通过 CLI

首先设置环境变量：

```bash
export DINGTALK_APP_KEY="your_app_key"
export DINGTALK_APP_SECRET="your_app_secret"
export DINGTALK_ROBOT_CODE="your_robot_code"  # 可选但推荐
```

然后配置并启动 Bridge：

```bash
cccc im set dingtalk \
  --app-key-env DINGTALK_APP_KEY \
  --app-secret-env DINGTALK_APP_SECRET \
  --robot-code-env DINGTALK_ROBOT_CODE

cccc im start
```

验证是否运行中：

```bash
cccc im status
```

两种方式都会保存到 `group.yaml`：

```yaml
im:
  platform: dingtalk
  dingtalk_app_key_env: DINGTALK_APP_KEY
  dingtalk_app_secret_env: DINGTALK_APP_SECRET
  dingtalk_robot_code_env: DINGTALK_ROBOT_CODE
```

## 第六步：在钉钉中订阅

1. 在钉钉应用中找到机器人
2. 添加到群聊或开始单聊
3. 发送 `/subscribe`
4. 确认订阅

## 使用方法

### 向 Agent 发送消息

钉钉支持两种发送消息的方式：

**私聊（隐式发送）** — 直接输入消息：

```
请检查一下代码质量
```

**显式 `/send` 命令** — 指定收件人：

```
/send @foreman 请检查代码质量
/send @all 请更新一下状态
```

::: tip 隐式发送
钉钉消息始终定向到机器人（通过群聊中的 @提及或单聊），所以纯文本会自动视为 `/send` 给 foreman。你只需要在指定其他 agent 时使用显式的 `/send` 命令。
:::

### 指定特定 Agent

使用 `@提及` 语法配合 `/send` 命令：

```
/send @foreman 请分配今天的开发任务
/send @reviewer 请审查最新的提交
/send @all 请更新一下状态
```

### 接收消息

订阅后，你会自动收到：
- Agent 的回复
- 状态更新
- 错误通知

使用 `/verbose` 切换是否接收 agent 之间的消息。

### 消息类型

钉钉支持多种消息类型：

- **文本**：纯文本消息
- **Markdown**：格式化文本
- **链接**：URL 卡片
- **ActionCard**：带按钮的交互卡片

CCCC 会自动选择合适的格式。

### 文件分享

在消息中附加文件。钉钉文件会被下载并存储到 CCCC 的 blob 存储中，然后转发给 agent。

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

### "Invalid appkey" 错误

1. 在钉钉开放平台验证 AppKey
2. 检查环境变量是否设置正确
3. 确保应用已发布

### "No permission" 错误

1. 检查所需权限是否已授予
2. 验证应用对用户是否可见
3. 确保应用版本已发布

### 机器人没有响应

1. 检查机器人是否已添加到聊天中
2. 验证 Bridge 是否运行中：
   ```bash
   cccc im status
   ```
3. 查看日志：
   ```bash
   cccc im logs -f
   ```

### 连接断开

如果连接意外断开：

1. 检查网络连接
2. 重启 Bridge：
   ```bash
   cccc im stop
   cccc im start
   ```

## 安全注意事项

- 妥善保管 AppSecret 并定期轮换
- 使用最小必要权限
- 定期审查机器人/应用访问权限
- 定期审计消息日志
- 限制机器人对必要员工的可见性
