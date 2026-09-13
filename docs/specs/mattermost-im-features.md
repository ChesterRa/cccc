# CCCC 现有连接器功能与 Mattermost 对照

日期：2026-09-07。源码基线：`2a38ad78700a4a188b45f515434b74cc315b91ec`。
对应 [规格](mattermost-im.md) 和 [验收](mattermost-im-acceptance.md)。**2026-09-08：F01–F34 对应的 T01–T19 技术验证已完成，用户明确确认完整验收结果正常，T20 通过。具体证据等级以右列验收编号为准，不将模拟故障说成真实服务器故障。现进入公开 Fork 源码提交及上游 Issue 阶段；上游 PR 和正式版本发布另行决定。**

文件边界：各连接器传输的是 `attachments`，不是 `refs` 引用。此次 Codex 仅发引用导致文件未回传，保留为 Actor 工具使用事件，不追记为发送成功，也不据此给 Mattermost 增加与其他连接器不同的引用转附件行为。

## 1. 检查范围与证据规则

2026-09-12 PR #103 审核修订：F01/F27 增补 Mattermost 地址就地校验、保存/启动错误呈现与重试；F04/F21 忽略无正文且无附件的裸点名，F30 的纯附件路径保留；F28 的独立 Bot 部署要求明确为外部约束，不宣称程序有跨 Group/实例重复检测。新增验证见[本轮验收记录](mattermost-im-acceptance.md)。

覆盖 Telegram、Slack、Discord、飞书、钉钉、企业微信、微信的原生接入入口、公共命令/状态/授权、入站媒体、出站/流式、处理反应、生命周期，以及现有 CLI/Web 配置和官方接入指南。这里列的是用户可感知功能及必要的传输行为，不声称完整审计了依赖 SDK、所有平台 API 或全仓安全。

- **可直接实现**：MM API 能承载功能，或该功能由现有 CCCC 公共代码完成。
- **可等价实现**：用户能力可以保留，但平台呈现/协议不同，必须写明差异。
- **不适用**：源平台独有认证或传输形式，不原样复制；相应通用用途仍须实现。
- 所有可直接/等价项都纳入开发。只有实际测试结果才能改成“通过”；限制必须给 API、代码或测试证据，不能用“第一期”作为永久漏项理由。

指南与源码可能不一致。例如部分指南写保存即启动、WeCom 无 CLI 凭据参数、飞书要求订阅已读事件、Discord 使用 embeds；当前相关代码不能因此被推导为具备全部所述行为。取当前调用路径和测试为准，不照抄营销或过期说明。

## 2. 26 项原有基线功能

保留前期 F01–F26 编号，以便识别这次不再受特定业务范围限制的功能。

| 编号 | CCCC 现有功能及源码依据 | Mattermost 可行性与映射 | 本次结论 | 验收 |
|---|---|---|---|---|
| F01 | Web/CLI 配置、凭据校验、启动和运行状态；[C1][C2][C3][C4] | Bot Token、站点 REST 身份查询、WS 认证；在原 UI 增平台 | 可直接实现；代码已接入；验收通过，证据见右列 | T02、T03 |
| F02 | 组级 start/stop、任务回收、恢复 enabled；[C1][C5] | 同一组级生命周期管理 WS/外发任务，不控制 Actor | 可直接实现；代码已接入；验收通过，证据见右列 | T04 |
| F03 | DM、群/频道、线程目标；Slack/飞书按 thread、Discord 用 thread channel；[C6][C7][C8] | MM channel_id 表示频道/DM，root_id 表示线程；群组 DM 受 Bot 加入能力约束；[M1][M2] | 可直接实现；代码已接入；真实目标权限验证通过 | T06 |
| F04 | DM 正文隐式发给 foreman；群中 mention 或已识别命令显式触发；[C6][C8][C9] | MM 解析当前 Bot 的真实 mention；裸斜线命令受平台拦截，使用 @Bot 前缀投递；[M3] | 可等价实现；代码已接入；验收通过，证据见右列 | T07、T08 |
| F05 | `/send` 默认收件人、`/send @ActorID` 指定收件人；[C10] | 复用公共消息转换，只剥离平台 Bot 寻址，不扫描正文追加 Actor | 可直接实现；代码已接入；验收通过，证据见右列 | T07 |
| F06 | `@all`、`@peers` 等 CCCC 寻址交给 daemon；[C10] | 不要求 MM 有同名用户或身份组，不逐个启动 CLI | 可直接实现；代码已接入；验收通过，证据见右列 | T07 |
| F07 | `/subscribe`、`/sub`、10 分钟配对码、可读目标组说明；[C9] | 对 MM 聊天/线程申请授权，使用普通 Bot 消息回配对结果 | 可直接实现；代码已接入；验收通过，证据见右列 | T05、T08 |
| F08 | Web 待审批、批准/拒绝、手输绑定码、授权列表、撤销；CLI 对应操作；[C2][C3][C4] | 用原有界面和状态结构保存 MM 目标，无新审批系统 | 可直接实现；代码已接入；验收通过，证据见右列 | T05 |
| F09 | `/unsubscribe`、`/unsub` 及订阅失效；[C9] | 保留原生命令与状态修改语义，不删除 Group | 可直接实现；代码已接入；验收通过，证据见右列 | T05、T08 |
| F10 | `/pause`、`/resume` 只影响该聊天/线程的收发；[C9][C10] | 同一目标键控制 MM 出入站，不当成停止模型或补发积压 | 可直接实现；代码已接入；验收通过，证据见右列 | T08、T09 |
| F11 | `/status` 组名、运行/组状态、Actor 数和订阅标志；`/help`；[C9] | 普通帖子回复当前公共信息，不增加逐 Actor 工具状态接口 | 可直接实现；代码已接入；验收通过，证据见右列 | T08、T09 |
| F12 | `/verbose` 无参数启用；on/off 及 true/false、1/0；默认关闭；[C9][C10] | 同组各目标分别保存开关，保留默认值和公共可见性 | 可直接实现；代码已接入；验收通过，证据见右列 | T08、T09、T10 |
| F13 | 仅 `im_visibility: public` 且非 Actor 定向的 system.notify 外发；[C1] | 发布到有效 MM 订阅；不转发私有系统事件和终端内部内容 | 可直接实现；代码已接入；验收通过，证据见右列 | T10 |
| F14 | 正文使用 sender_title，缺失回退 Actor ID；[C11] | 使用 MM Markdown 正文标签，不创建每 Actor 一个 Bot | 可直接实现；代码已接入；验收通过，证据见右列 | T10 |
| F15 | chat.stream start/update/end、节流、按目标保存编辑句柄；微信例外；[C12][C13][C14] | POST 创建初帖，PUT patch 更新；无流事件不凭终端猜流；[M1][M4] | 可等价实现；代码已接入；验收通过，证据见右列 | T11 |
| F16 | 只有完整终态成功才抑制最终消息，失败/超限最终兜底、Unicode 安全分段；[C12][C13][C15] | 按 MM 实际帖子限制分段；各订阅独立处理；[M1][M4] | 可直接实现；代码已接入；验收通过，证据见右列 | T11、T12 |
| F17 | 入站文件下载、体积检查、Blob 存储及元数据；[C16][C17] | GET 文件及元数据，核对帖子归属，不信任任意远端 URL；[M5] | 可直接实现；代码已接入；验收通过，证据见右列 | T13、T14 |
| F18 | 出站从本组 Blob 准备、校验文件名、单/多附件、正文兜底；[C12][C13][C18] | POST files 后把 file_ids 与正文/线程一起创建帖子；[M1][M6] | 可直接实现；代码已接入；验收通过，证据见右列 | T13、T14 |
| F19 | source_user/message/thread、im_*、reply_to 与稳定 client_id 关联；[C10][C19] | 保留 MM 用户、原帖、root_id 和对应 CCCC 事件，不用昵称充当身份 | 可直接实现；代码已接入；验收通过，证据见右列 | T06、T15 |
| F20 | client_id 交由 daemon 去重；部分平台另有有界内存去重；[C10][C20] | 同一 MM post_id 重复事件使用同一请求标识；不增加新持久队列 | 可直接实现；代码已接入；验收通过，证据见右列 | T15 |
| F21 | 出站排除人类/IM 入站，平台入站过滤 Bot，避免回流；[C1][C6][C8] | 识别本 Bot、其他 Bot、系统事件；平台权限为前提 | 可直接实现；代码已接入；验收通过，证据见右列 | T10、T15 |
| F22 | Telegram/Discord/飞书/钉钉处理反应、关联完成/失败、超时清理；[C19][C21] | MM 创建/删除 Bot 自己的 reaction；反应不是任务取消或精确执行证明；[M7] | 可等价实现；代码已接入；验收通过，证据见右列 | T16 |
| F23 | 运行期 event hub lag 后按游标从 ledger 补读；[C1] | 直接复用公共外发 worker，不另走 SSE/SDK | 可直接实现；代码已接入；验收通过，证据见右列 | T17 |
| F24 | 连接重试/重连、认证失败、启动错误；不同平台策略不同；Discord 依赖原生恢复会话；[C6][C8] | WS 连接/认证/心跳、原生连接 ID/下一序号恢复；缓存失效明确报告，非全量历史补拉；REST 限流与错误处理；[M8][M9] | 现行合同见规格；新补收能力与本轮复验单独记录，不以旧验收代替 | T03、T17 |
| F25 | tracing 日志、last_error、运行状态、CLI logs；[C1][C3][C4] | 日志及已有状态入口呈现 MM 错误，脱敏且不吞失败 | 可直接实现；代码已接入；验收通过，证据见右列 | T03、T14、T17 |
| F26 | 组配置、authorized/pending/subscribers 持久化与共享锁更新；[C22] | 增 MM 平台和字段，复用公共文件形状，不建独立数据库 | 可直接实现；代码已接入；验收通过，证据见右列 | T04、T05 |

## 3. 展开的附属功能，避免只对齐主流程

| 编号 | 已有能力及源码依据 | Mattermost 映射与边界 | 本次结论 | 验收 |
|---|---|---|---|---|
| F27 | Token 字面量/环境变量引用/旧别名规范化，保存既有 files 策略；[C2][C22] | 复用同一规则，额外保存 MM 站点；真实文件上限以执行路径为准，不能只测配置保存 | 可直接实现；代码已接入；验收通过，证据见右列 | T02、T14 |
| F28 | 一组多订阅，按 target_key 区分授权、线程、暂停、verbose 和流完成状态；[C10][C12] | 允许 MM 多目标订阅同 Group；它们共享上下文且可收到同组外发，不是独立私密会话 | 可直接实现；代码已接入；验收通过，证据见右列 | T09、T11、T18 |
| F29 | Telegram/微信/WeCom 接收图片、音频、视频等并转换通用媒体元数据；[C17][C23][C24] | MM 对文件类媒体使用 file_ids 下载/上传；保留 MIME，不承诺语音识别或原平台播放器外观 | 可等价实现；代码已接入；验收通过，证据见右列 | T13 |
| F30 | 仅附件输入及附件占位正文，混合正文/多个附件；[C10][C12][C17] | 获授权 DM 可仅发附件；群消息仍需明确寻址；向 daemon 传公共附件形状 | 可直接实现；代码已接入；验收通过，证据见右列 | T13 |
| F31 | Markdown/纯文本、链接、代码和发言者展示，平台自己的格式转换；[C11][C12][C13] | 使用 MM 支持的正文格式，不为模仿另一平台 Embed 新建卡片协议 | 可等价实现；代码已接入；验收通过，证据见右列 | T10、T12 |
| F32 | CLI config/set/unset/start/stop/status/logs/pending/authorized/bind/reject/revoke；启动旧历史边界与弃用选项归一；[C4][C22][C25] | 同一 CCCC CLI 接口增 MM 参数；不恢复已弃用 /context、/launch、/quit 或 backlog 重放开关 | 可直接实现；代码已接入；验收通过，证据见右列 | T04、T17 |
| F33 | Web 平台表单、草稿、保存/启动校验、状态、待审批、授权管理、主题及 i18n；[C2][C26] | 在原组件与受控状态路径增必要字段，覆盖既有表单测试和真实浏览器操作 | 可直接实现；代码已接入；验收通过，证据见右列 | T02 |
| F34 | 适配器按平台使用已有 HTTP/WS 客户端及其代理配置；Discord 有专属代理桥；[C6][C27] | 自托管站点+现有客户端网络配置，REST 与 WS 分别核验；不复制 Discord 专用桥或私设另一个代理服务 | 可等价实现；代码已接入；验收通过，证据见右列 | T03 |

## 4. 平台专属项和不应误算为现有能力的内容

| 编号 | 原平台事实 | Mattermost 判断 | 对应通用能力 |
|---|---|---|---|
| N01 | 微信二维码登录、二次验证码、扫描账号自动授权、退出和凭据清理；[C28] | **不适用原样实现**：MM 接入选 Bot Token 与显式聊天配对，不开发个人账户扫码授权 | F01、F07、F08、F27 |
| N02 | 钉钉 AI Card Streaming、WeCom callback req_id 绑定的原生 stream；[C14][C29] | **等价实现**：MM 同帖渐进编辑保留功能，无需另一种平台的 Card 模板/回调时窗 | F15、F16 |
| N03 | WeCom AES 媒体下载解密和 WebSocket 分块上传；[C24][C29] | **不适用原样实现**：MM 使用受认证的文件 REST 接口，不自己添加一层 AES 协议 | F17、F18、F29 |

以下不构成本基线连接器的已证实功能，不能根据平台有 API 就扩张本次目标：

- 已读回执完整同步：飞书指南列有已读事件，但本次定位的 worker 接收处理路径未发现对应回写；不能据此给 MM 强加已读同步需求。
- 完整 Web/TUI Actor 状态、隐藏工具输出、自动会议控制：不属于公共 IM 状态命令的能力。
- Mattermost 原生 Slash 注册、弹窗/按钮控制台：与公共文本命令不是一回事；F04 的前缀入口覆盖原有命令用途，额外界面单独评估。
- 停机补发/持久待发箱/恰好一次：公共外发从当前 ledger 边界启动，运行期 lag 补读不等于跨重启可靠重放。不得在说明中混淆。
- PDF/OCR/网页处理和音视频转写：当前连接器传输媒体，不负责内容理解；模型工具能力不算平台接入已实现。

## 5. 真实验收须验证的 MM 差异

1. 群组 DM 的 Bot 成员资格与私有频道权限，以目标服务器实际 API 为准；不通过提升成系统管理员绕过限制。
2. 首字符斜线输入由客户端触发原生命令时，不会因为存在 WS 就必然进入公共解析器；在 UI 实测全部 @Bot 前缀命令及线程输入。
3. MM Post 创建、编辑、文件上传及反应都有各自权限；依次验证最小权限，不用笼统的“管理员权限保证可用”。
4. 帖子长度、每帖附件数、文件大小和限流使用实际服务器契约，不沿用 Discord 的数值。
5. 同组多订阅共享内容是原生行为，使用者必须知情；本文不授权改变现有部署权限，也不自动批准额外目标。

## 6. 源码索引

以下本地链接均指本次固定基线；实现修改后保留本文的基线说明以便回查。

[C1]: ../../crates/cccc-web/src/im_runtime.rs
[C2]: ../../crates/cccc-web/src/routes/im.rs
[C3]: ../../crates/cccc-daemon/src/ops/im.rs
[C4]: ../../crates/cccc-cli/src/args/integrations.rs
[C5]: ../../crates/cccc-web/src/im_runtime/worker.rs
[C6]: ../../crates/cccc-web/src/im_runtime/slack.rs
[C7]: ../../crates/cccc-web/src/im_runtime/feishu.rs
[C8]: ../../crates/cccc-web/src/im_runtime/discord.rs
[C9]: ../../crates/cccc-web/src/im_runtime/commands.rs
[C10]: ../../crates/cccc-web/src/im_runtime/state.rs
[C11]: ../../crates/cccc-web/src/im_runtime/outbound_message.rs
[C12]: ../../crates/cccc-web/src/im_runtime/slack_outbound.rs
[C13]: ../../crates/cccc-web/src/im_runtime/discord_outbound.rs
[C14]: ../../crates/cccc-web/src/im_runtime/dingtalk_streaming.rs
[C15]: ../../crates/cccc-web/src/im_runtime/outbound_chunks.rs
[C16]: ../../crates/cccc-web/src/im_runtime/inbound_attachments.rs
[C17]: ../../crates/cccc-web/src/im_runtime/telegram_inbound.rs
[C18]: ../../crates/cccc-web/src/im_runtime/outbound_attachment.rs
[C19]: ../../crates/cccc-web/src/im_runtime/processing_reactions.rs
[C20]: ../../crates/cccc-web/src/im_runtime/discord_dedup.rs
[C21]: ../../crates/cccc-web/src/im_runtime/discord_reactions.rs
[C22]: ../../crates/cccc-core/src/im_state.rs
[C23]: ../../crates/cccc-web/src/im_runtime/weixin_inbound.rs
[C24]: ../../crates/cccc-web/src/im_runtime/wecom_message.rs
[C25]: ../guide/im-bridge/index.md
[C26]: ../../web/src/components/modals/settings/imBridgeConfig.ts
[C27]: ../../crates/cccc-web/src/im_runtime/discord_gateway_proxy.rs
[C28]: ../../crates/cccc-web/src/im_runtime/weixin_login.rs
[C29]: ../../crates/cccc-web/src/im_runtime/wecom_client.rs

## 7. Mattermost 官方依据

查阅日期：2026-09-07。这些接口支持上述映射分析，不代表目标服务器已经验证通过。

[M1]: https://docs.mattermost.com/api/reference/create-post
[M2]: https://raw.githubusercontent.com/mattermost/mattermost/v11.9.0/api/v4/source/channels.yaml
[M3]: https://developers.mattermost.com/integrate/slash-commands/
[M4]: https://docs.mattermost.com/api/reference/patch-post
[M5]: https://docs.mattermost.com/api/reference/get-file
[M6]: https://docs.mattermost.com/api/reference/upload-file
[M7]: https://raw.githubusercontent.com/mattermost/mattermost/v11.9.0/api/v4/source/reactions.yaml
[M8]: https://docs.mattermost.com/api/reference/connect-web-socket
[M9]: https://github.com/mattermost/mattermost/blob/v11.9.0/server/channels/app/platform/web_conn.go
