# IM Bridge 概览

将你的 CCCC 工作组桥接到主流 IM 平台，实现移动端访问。

## 什么是 IM Bridge？

IM Bridge 允许你：

- 从手机向 Agent 发送消息
- 接收更新和通知
- 通过斜杠命令控制工作组
- 分享文件和附件

## 支持的平台

| 平台 | 状态 | 最适合 |
|------|------|--------|
| [Telegram](./telegram) | ✅ | 个人使用，快速设置 |
| [Slack](./slack) | ✅ | 团队协作 |
| [Discord](./discord) | ✅ | 社区/游戏 |
| [飞书/Lark](./feishu) | ✅ | 企业（中国/全球） |
| [钉钉](./dingtalk) | ✅ | 企业（中国） |

## 设计原则

- **1 个工作组 = 1 个 Bot**：每个工作组连接一个 bot 实例，保持简洁和隔离
- **显式订阅**：用户必须先 `/subscribe` 才能接收消息
- **轻量端口**：IM Bridge 仅转发消息；Daemon 是唯一的真实来源

## 通用命令

订阅任何平台后，以下命令通用：

| 命令 | 描述 |
|------|------|
| `/send <消息>` | 发送给 foreman（默认） |
| `/send @<actor> <消息>` | 发送给指定 actor |
| `/send @all <消息>` | 发送给所有 agent |
| `/send @peers <消息>` | 发送给非 foreman 的 agent |
| `/subscribe` | 开始接收消息 |
| `/unsubscribe` | 停止接收消息 |
| `/status` | 显示工作组状态 |
| `/pause` | 暂停消息投递 |
| `/resume` | 恢复消息投递 |
| `/verbose` | 切换详细模式 |
| `/help` | 显示帮助 |

::: tip 隐式发送
在所有平台上，@提及 bot（在群聊中）或直接发送纯文本消息会自动视为 `/send` 给 **foreman**。你只需要在指定其他 agent 时使用显式的 `/send` 命令。
:::

## CLI 命令

```bash
# 配置（平台特定，请参见各平台指南）
cccc im set <platform> --token-env <ENV_VAR>

# 控制
cccc im start        # 启动 IM Bridge
cccc im stop         # 停止 IM Bridge
cccc im status       # 检查 Bridge 状态
cccc im logs         # 查看日志
cccc im logs -f      # 实时跟踪日志
```

## 快速开始

1. 从上方列表中选择一个平台
2. 按照设置指南创建 bot
3. 使用 bot 凭据配置 CCCC
4. 启动 Bridge 并在聊天中订阅

## 下一步

- [Telegram 设置](./telegram) - 快速个人设置
- [Slack 设置](./slack) - 团队协作
- [Discord 设置](./discord) - 社区访问
- [飞书/Lark 设置](./feishu) - 企业（中国/全球）
- [钉钉设置](./dingtalk) - 企业（中国）
