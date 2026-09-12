# Mattermost 接入

本文适用于包含 Mattermost 连接器的 CCCC 构建；使用前请确认 IM Bridge 的平台列表中可以选择 Mattermost。证据等级和已知使用边界以[验收记录](../../specs/mattermost-im-acceptance.md)为准。

Mattermost 连接器把一个 CCCC Group 接入 Mattermost，使用 Bot Token、REST 和 WebSocket。无需向公网暴露回调接口，也不增加会议编排或新的智能体运行层。

CCCC 安装在自己的应用服务器，由它主动连接 Mattermost 站点的 HTTPS/WSS；不需要在 Mattermost 服务器上安装或构建 CCCC。

## 接入边界

::: warning 同组聊天共享内容
部署者应为每个 Group 配置独立 Bot，不要在多个 Group 或运行中的 CCCC 实例中复用同一 Bot 身份，即使使用它的不同 Token。CCCC 没有跨 Group/实例的 Bot 重复检测或互斥锁，这是一项部署要求，不是程序会自动阻止的操作。如果多个 Group 同时批准了该 Bot 所在的同一聊天，消息可能被重复处理并混合多个 Group 的回复。

一个 Group 可以批准多个频道、私聊或线程，但它们共享该组上下文及订阅输出。私聊不是独立、保密的智能体会话。批准一个聊天目标，意味着允许该聊天的参与者访问本 Group；不等同于对其中每个人单独授权。
:::

## 1. 准备 Bot

1. 在自己的 Mattermost 站点创建专用 Bot 账号，并取得其访问令牌。不要使用个人或管理员令牌代替 Bot Token。
2. 把 Bot 加入需要连接的团队和频道；私有频道需另行邀请。
3. 允许 Bot 读取其参与的频道、发送和编辑自己的帖子、上传及读取附件、添加和移除自己的反应。不需要授予系统管理员权限。
4. 从运行 CCCC 的机器验证站点 HTTPS 和 WebSocket 可达，反向代理必须支持 WebSocket 升级。

参考 Mattermost 官方[机器人账号说明](https://developers.mattermost.com/integrate/reference/bot-accounts/)和 [API 文档](https://api.mattermost.com/)。群组私聊是否允许 Bot 加入取决于目标站点的权限和版本；不要通过提高 Bot 到管理员来绕过限制。

## 2. 在 CCCC Web 配置

在目标工作组的 **Settings → IM Bridge** 中：

1. 选择 **Mattermost**。
2. 输入站点根地址，例如 `https://mattermost.example.com`；若安装在子路径，可填 `https://example.com/chat`。不要追加 `/api/v4`、查询参数或账号密码。
3. 输入 Bot Token，或运行 CCCC 进程中已配置的环境变量名，例如 `MATTERMOST_BOT_TOKEN`。推荐使用环境变量引用，避免截图或分享配置时暴露真实令牌。
4. 保存配置，再启动连接器。地址不符合上述格式时，页面显示提示并禁用保存和启动。现有“启动”操作也会先保存当前表单；保存失败不会启动，保存或启动错误会在页面显示，可修正后重试。
5. 确认显示运行中。启动会验证 Bot 身份及 WebSocket `hello`，不只检查 Token 字符串是否填写。

生产站点使用 HTTPS；HTTP 仅适用于明确可信的本机或隔离测试网络，令牌和消息都不会加密。连接器不禁用证书验证，也不跟随认证请求的重定向；请直接填写最终站点地址。

## 3. 授权聊天

以下 `cccc_bot` 必须替换为 Bot 的实际 **username**，不是显示昵称。在频道或私聊输入：

```text
@cccc_bot /subscribe
```

在 CCCC **Pending Requests** 确认请求对应的 Group 和目标后批准，也可粘贴密钥绑定。密钥有效期为 10 分钟。频道和线程分别授权，批准频道不会自动批准全部帖子线程。

::: tip Mattermost 的斜杠命令
Mattermost 客户端会拦截开头为 `/` 的输入。频道和私聊里的 CCCC 命令都建议加上 `@cccc_bot` 前缀，不需要安装名为 `/send` 或 `/subscribe` 的 Mattermost 自定义命令。
:::

## 4. 发送消息及控制订阅

| 输入 | 作用 |
|---|---|
| `@cccc_bot 你好` | 发给默认 foreman |
| `@cccc_bot /send @reviewer 请检查这份材料` | 只发给指定 Actor ID |
| `@cccc_bot /send @all 请各自回答` | 广播给全部 Actor |
| `@cccc_bot /send @peers 请补充意见` | 发给非 foreman 成员 |
| `@cccc_bot /status`、`@cccc_bot /help` | 查看状态和帮助，不调用模型 |
| `@cccc_bot /pause`、`@cccc_bot /resume` | 暂停或恢复当前聊天目标的订阅 |
| `@cccc_bot /verbose on`、`@cccc_bot /verbose off` | 开关更详细的公开交流；不开放私有事件 |
| `@cccc_bot /unsubscribe` | 取消订阅；也支持 `/unsub` |

授权后的私聊可直接输入普通正文。频道中的普通问题和附件需要点名 Bot；正文顺带提到另一个 Actor 不会增加收件人。`/sub` 是 `/subscribe` 的别名；`/verbose` 不带参数表示开启，也接受 `true/false` 和 `1/0`。

频道订阅的输出在频道主时间线显示；独立批准的线程订阅保留原线程。所有符合该 Group 订阅规则的输出会分发给相应目标，不承诺只回复最初提问的人。

## 5. 渐进输出和文件

- CCCC 发布 `chat.stream` 时，Bot 创建并更新同一帖子；没有流事件时发送最终回答。连接器不会把 CLI 的全部 TUI 状态变成聊天消息。
- 只有完整流式终态已经成功显示且与最终正文一致，才省略重复正文；编辑失败或预览超长时保留完整最终回答。
- 长消息按 Unicode 字符安全分段，默认每帖最多 16,383 字符；管理员配置或代理若更严格，需要据实际错误检查。
- 图片、普通文件、PDF、音频和视频作为文件双向传递，通过当前 Group 的 Blob 存储。支持只有附件的消息；这不代表连接器进行了 OCR、PDF 解析或语音转写。
- 沿用公共文件安全限制，每文件最多 10 MiB，并取更低的组级 `files.max_mb`。`files.enabled=false` 时不转发附件。文件失败会提示，不会伪装成已交给智能体。
- 处理中显示 Bot 自己添加的反应；响应后更新成功或失败反应。反应过期清理不是取消正在运行的智能体。

### 文件引用不等于附件

消息正文中的文件名、本地路径以及 `refs` 文件引用不会自动上传到 Mattermost。连接器只发送 CCCC 消息中的 `attachments`，这与其他原生 IM 连接器的分工一致。

智能体交付文件应调用 `cccc_file(action="send", ...)`，并检查工具返回结果。该操作要求文件位于当前工作目录范围内；收到的 `state/blobs/...` 附件可先通过文件工具读取或解析路径，再将需回传的文件复制到工作目录后发送。工具失败时应如实说明，不能以普通消息或 `refs` 替代附件并声称“已发送”。

排查时分别检查 CCCC 的 `attachments` 和 Mattermost 帖子的 `file_ids`；仅看到“附件补发”文字不能证明文件已交付。

## 6. CLI 和运维

```sh
cccc im set mattermost --group g_example \
  --mattermost-url https://mattermost.example.com \
  --bot-token-env MATTERMOST_BOT_TOKEN
cccc im start --group g_example
cccc im status --group g_example
cccc im pending --group g_example
cccc im bind --group g_example --key KEY_FROM_CHAT
cccc im authorized --group g_example
cccc im logs --group g_example -f
cccc im stop --group g_example
```

其他现有操作如 `config`、`unset`、`reject`、`revoke` 同样适用，参数以 `cccc im --help` 为准。停止连接器不停止 Actor。网络 worker 仍由 CCCC Web 进程承载，不是单独的后台服务。

`im logs` 需要在现有全局可观测性设置中开启开发者模式；关闭时会返回 `developer_mode_required`，但不停止错误记录。Mattermost 将错误写入当前 Group 的 `state/im_bridge.log`，同时输出到进程 stderr（Docker 部署可从容器日志读取），不依赖组合 CLI 是否初始化 tracing。记录包含时间、Group、操作和脱敏错误，不包含聊天正文及附件内容；单条错误最多 4096 字符，文件超过 1 MiB 前轮转至 `im_bridge.log.1`，只保留一份备份。文件写入失败在 stderr 报告，不阻断收发。`last_error` 是最后错误状态，不是日志历史；重连或启停清除状态不会删除日志。

断线会自动重连；运行期 ledger 消费落后时沿用公共补读机制。重启从新的边界开始，不自动回灌旧消息，也不提供持久待发箱或 exactly-once 保证。创建帖子遇到结果不明确的网络错误不会盲目重发。

附件下载与 WebSocket 接收分属两个 worker，入站队列最多暂存 128 个事件并按接收顺序处理。队列满时暂停继续读取（包括尚未读到的控制帧），继续发送本端心跳；腾出位置后恢复读取，不把本地背压时间算成远端失活。持续过载仍可能导致服务端断开，重连不自动补拉遗漏帖子。停止桥接同时取消接收和入站处理，不是无限积压或可靠投递队列。

更换站点或 Bot 身份会清除旧聊天授权、待批准请求和订阅，需重新批准；同一个 Bot 正常轮换 Token 保留授权。身份检查失败时不会退回个人身份或扩大权限。错误可在现有状态面板及连接器日志查看。
