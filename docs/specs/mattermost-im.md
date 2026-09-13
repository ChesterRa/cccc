# CCCC Mattermost 连接器功能规格

日期：2026-09-07，状态更新于 2026-09-08。**连接器验收完成：T01–T19 技术验证已完成，用户明确确认“我已经验收完了，都正常”，T20 通过。最新 IM 回归 188 项、前端差异回归 12 项通过，既有全量回归记录保留。测试证据等级及 refs 文件引用事件的责任边界见验收记录。现进入公开 Fork 源码提交及上游 Issue 阶段，不代表上游已合并或发布正式版本。**
源码基线：`2a38ad78700a4a188b45f515434b74cc315b91ec`；此前 v0.4.37 调研只作为历史参考，不代表此基线已经部署。

入口：[完整功能对照](mattermost-im-features.md)、[验收用例](mattermost-im-acceptance.md)、[原生 IM 概念与边界](../guide/im-bridge/index.md)、[架构决策](../adr/0001-native-mattermost-im.md)。

## 上游贡献整理（2026-09-12）

本轮基于上游 `22733e9ac607989bb095a4a6b7cb0e518bab9d08` 整理独立 Mattermost PR，关联 Issue #99。保留已发布分支历史，通过合并接入最新上游，不强推旧提交。不携带 CLI 管理、Experimental 或文档治理；全局 `CONTEXT.md` 不在本 PR 相对上游的差异中。旧来源校验测试冲突采用上游实现，不更改其现行安全策略。

下文基线、授权与验收状态均记录 2026-09-07/08 当时的开发过程；本轮测试及发布以验收记录中的独立修订为准，不把既有现场验收描述为已经重新执行。本轮允许提交 PR，不包含合并上游、Release 或更换现有部署。

## PR #103 审核修订（2026-09-12）

- 入站去除 Bot 点名后，正文为空且没有附件时直接忽略，不回复授权提示、不触发反应或调用智能体；纯附件请求仍沿用原有授权与附件流程。
- Mattermost Web 地址校验与已有后端合同一致：仅 HTTP/HTTPS 站点，可带安装子路径，不带凭据、查询、片段或 `/api/v4` 后缀。无效地址禁用保存和启动并就地提示；保存或启动失败在页面显示错误，允许修正后重试。
- “每个 Group 使用独立 Bot”是部署者须遵守的身份隔离要求，不是程序已实现跨 Group/实例重复检测的保证。同一 Bot 的不同 Token 也不应复用；本补丁不新增全局锁或改变其他连接器。
- 英文平台总表使用英文单元格。新增行为用同模块 Rust 和现有前端测试验证，并在 Linux、Windows 测试机复测真实 Web 页面；协议/故障模拟与真实 Mattermost 现场验收分别记录。

## 1. 六项硬要求

本轮补充（2026-09-13，测试机验证完成；未提交、推送或更换日常部署）：

- 身份校验沿用原生文件锁与提交写入，按 Group 串行执行“读旧身份→核对配置/启动代次→持久清除旧授权→提交新身份”；旧启动不能在新启动之后覆盖身份。仍先清授权再写身份，失败不错误复用旧授权。
- 所有运行期错误的写入和清除，在原配置锁中校验启动代次及配置快照；过期 worker 仍可写脱敏诊断日志，但不能覆盖新配置的 `last_error`。
- Mattermost 启动按钮保存失败时不回读旧配置，保留平台、地址、凭据引用草稿和就地错误；保存成功后才启动、刷新状态。其他平台沿用原行为。
- 入站断线补收使用 Mattermost 原生 `connection_id` / `sequence_number` 恢复协议：保存本次 worker 已接收的下一事件序号，重连由服务器顺序补回缓存事件；所有事件均推进序号，只有 `posted` 进入原有有界入站队列。重复序号忽略，缺口触发重新恢复；认证拒绝仍终止 worker。
- 初连等待 `hello`；恢复连接可能直接返回补收事件而没有新的 `hello`，不得等待或吞掉首条补收。若服务器返回新连接 ID（服务器重启、缓存过期或跨节点等），重置序号并通过现有日志及 `last_error` 明确提示补收缺口；该提示不因普通重连成功被清除。恢复事件继续执行现行聊天授权、暂停、Bot 过滤及稳定 `client_id` 去重，不扩大权限。
- 不全量扫描频道，不补回进程重启/主动停止期间的历史，不增加 REST 历史轮询、数据库或新依赖；不承诺缓存外补收、处理成功确认或恰好一次。机制依据：[Mattermost Web 客户端](https://github.com/mattermost/mattermost/blob/v11.9.0/webapp/platform/client/src/websocket.ts)、[服务器恢复实现](https://github.com/mattermost/mattermost/blob/v11.9.0/server/channels/app/platform/web_conn.go)。这是运行期短线恢复，不是 Topic 或会议功能。
- 注册表失效方法用能力名称 `invalidate_start`；共享恢复入口保留当前已授权的 Mattermost 自行提交状态分支，不重构其他平台的启动返回合同。凭据只解析一次，Web URL 校验注明后端为权威。

新增验收覆盖身份提交竞争、旧错误写入/清除、保存失败草稿、无 hello 补收首帖、重复/缺口序号、缓存失效及恢复后授权过滤。所有构建、测试和真实页面验证仅在指定 Linux/Windows 测试机执行；完整检查与真实 Mattermost 协议补收均通过，具体结果及边界见[验收记录](mattermost-im-acceptance.md)。术语检查：沿用 Group、Bot、IM Bridge、Chat Target、Event、Ledger；传输恢复游标不是新增业务上下文。

本轮补充（2026-09-13，PR #103 第三轮审核）：Mattermost 启动结果仅在原生 worker 启动代次及配置快照均有效时写回；保存（包括保存相同配置）、停止、删除配置或新的启动使旧结果失效。HTTP 与自动恢复入口不重复写回 Mattermost 的启动状态，其他平台的生命周期合同不变。保存时在既有配置锁内失效旧代次，避免“先保存、后停止”的窗口被旧启动覆盖。新增回归覆盖成功与失败的过期结果。

WebSocket 重连遇到 HTTP 401/403 或显式认证失败时记录原生错误并结束 worker；运行状态由既有 worker 生命周期与状态查询收敛为不可用。网络中断、限流和服务器暂时故障仍按原有退避重连；不自动变更凭据或权限。新增回归覆盖终止、不再重试及暂时故障恢复。

Mattermost 未保存草稿在工作组切换时明确清空，不提供跨组草稿缓存；同一工作组内切换平台仍保留草稿，已保存配置仍正常回读，其他平台不变。新增回归覆盖 A→B→A 且未编辑 B 的情况。以上沿用原生组件、配置锁、启动代次和 worker 监督机制，不增加公共调度机制。

本轮补充（2026-09-13，PR #103 第二轮审核）：沿用企业微信连接器的有界入站队列和独立 worker，Mattermost 的附件处理不再占用 WebSocket 心跳循环。队列满时施加读取背压，不丢弃已接收事件，不无限创建任务；背压期间继续发送心跳，恢复读取后重新计算接收超时。入站按接收顺序处理，停止连接器时两个任务一起取消。现行断线行为已由上方原生恢复协议补充；仍不自动全量补拉历史。

处理反应仍复用原生 `Active` 和清理机制，仅在 Mattermost 的 daemon 提交及事件 ID 绑定之间加异步互斥；最终反应等待绑定后再匹配，锁不覆盖附件下载或反应 HTTP。不修改其他连接器的反应语义，不引入完成事件缓存。

Mattermost 平台草稿的第二轮实现采用工作组来源校验；第三轮已将其替换为上述“切换工作组即清空”的合同，不再保留来源标记。同组平台切换仍恢复未保存内容，其他平台草稿机制不变。补齐既有配置测试夹具的必需字段，不放宽生产类型。新增回归须验证慢附件期间心跳、队列背压与顺序、完成早于绑定及重复/无关完成、保持挂载时的跨组草稿隔离。术语沿用工作组（Group）、聊天连接器（IM Connector）、附件和协作事件（Event），不增加领域概念；执行结果另记验收记录。

| 编号 | 用户要求 | 落实规则 |
|---|---|---|
| U01 | 源码组织和发布与现有 CCCC 一致 | 先看同类真实文件；单文件实现不人为拆工程，已有分文件习惯也不强行合成巨型文件；随 CCCC 构建发布，不做外置包或 .so |
| U02 | 代码风格与相似连接器一致 | 沿用模块可见性、start 入口、任务生命周期、错误返回、日志、测试和公共 helper 习惯 |
| U03 | 对 CCCC 公共源码最小修改 | 仅平台注册、必要配置透传、UI 和文档入口；公共语义变更必须单独说明，不顺便重构 |
| U04 | Web 配置严格遵循现有做法 | 在现有 IM Bridge 页增平台和必需字段，复用组件、样式、草稿、保存/启动及 i18n |
| U05 | 核对现有全部连接器功能，覆盖 MM 能支持的部分 | 对照表逐项记录源码、API 映射和验收；不能因阶段划分永久跳过私聊、线程、附件等可实现功能 |
| U06 | 目标仅为在 Mattermost 使用 CCCC | 不嵌入 roundtable、会议编排、轮数、Topic、摘要或角色设计；这些不是平台接入职责 |

“全部功能”指本基线七个原生 IM 连接器对用户提供的功能并集，不是各平台 SDK 的所有 API，也不是 CCCC 整个 Web 的功能。平台专属协议按用途做等价映射，不为了模仿二维码或 AI 卡片而新增不必要机制。

## 2. 源码组织和发布方式

当前实际组织：

| 参考连接器 | 源码组织 | Mattermost 参考用途 |
|---|---|---|
| Telegram | `telegram.rs`、`telegram_inbound.rs`、`telegram_outbound.rs` | 公共命令、媒体传输、订阅目标、处理反应 |
| Slack | `slack.rs`、`slack_inbound.rs`、`slack_outbound.rs` | REST/WS、线程、附件和同帖渐进更新的主要范本 |
| Discord | 入口、入站、出站，另有反应、去重、代理辅助文件 | Token 验证、启动失败、反应反馈和最终兜底 |
| 飞书 | `feishu.rs`、`feishu_inbound.rs`、`feishu_outbound.rs` | 线程授权、公开消息过滤、处理反应 |
| 钉钉、企业微信、微信 | 按媒体、传输、流式或登录等真实职责拆分 | 仅参考能映射的能力，不复制平台特有协议 |

以上文件位于 [im_runtime](../../crates/cccc-web/src/im_runtime.rs) 同级模块目录。指南有时只列主入口，不能由此推断整个连接器只有一个文件。

Mattermost 先按 Slack 的同级命名组织为 `mattermost.rs`、`mattermost_inbound.rs`、`mattermost_outbound.rs`；简单辅助逻辑留在所属文件，只有确实需要并有同类先例才新增文件，不预建空模块。测试优先按同类放在 `#[cfg(test)]` 内，不建独立服务、crate、SDK 或插件目录。

发布沿用 CCCC 的源码仓库、Cargo workspace、现有 Web bundle 和打包流程，不新增独立连接器安装器。不为本补丁新增系统服务、Docker 运行层或发布管线；本 Fork 的发布目标与上游官方账号隔离。

## 3. 代码与 Web 的具体约束

### 3.1 后端

- 复用组级 worker：`start(home, daemon, group_id, config, ledger_events)`、既有任务返回形式及 `WorkerHandles`；需要主动关闭 WS 时使用已有 Stopper 模式。
- 复用 `resolve_config_credential`、`inbound_decision_for_thread`、`dispatch_inbound_with`、`AuthorizedChat`、`target_key`、外发过滤、`outbound_text`、分段和 Blob 工具。
- 保留 `pub(super)` 等局部可见性、现有 `Result<..., String>` 与 `tracing` 习惯；不把内部 helper 导出成新公共 API。
- 使用现有 `reqwest`、`tokio-tungstenite` 和异步工具；新依赖须先证明必要性。
- 不改 daemon 的 Group、Actor、Session、收件人或执行顺序。接入收到请求后提交 CCCC，不等智能体回答才接下一条。
- 遇到公共能力缺口，写明最小补丁理由并补回归；不以复制全部公共逻辑或静默改变其他平台行为解决。

### 3.2 Web

复用 [IMBridgeTab](../../web/src/components/modals/settings/IMBridgeTab.tsx)、[SettingsModal](../../web/src/components/SettingsModal.tsx)、[imBridgeConfig](../../web/src/components/modals/settings/imBridgeConfig.ts) 以及 [IM API](../../web/src/services/api/im.ts)：

- 平台选择继续使用 `SelectCombobox`。
- 表单继续使用 `settingsWorkspacePanelClass`、`inputClass`、`labelClass` 和现有提示/错误样式；不新增主题、CSS 框架或专属 Dashboard。
- 字段沿用 React 受控输入、平台草稿缓存、配置回读、保存前校验、保存与启动链路。
- 待审批、已授权聊天、拒绝/撤销、详细交流开关及运行状态继续使用现有 UI，不重做审批面板。
- 文案通过现有 `settings` namespace 和 locale 文件补键；保留语言切换、深浅色、窄屏及无障碍标签习惯。
- Token 输入遵循现有凭据字段及环境变量引用规则，不借本补丁改造全部平台凭据表单。推荐填写环境变量名，真实值不进入日志和测试截图。
- 文档中的交互步骤以当前代码为准；部分旧指南写“保存自动启动”，不可不核对实际按钮链路就照抄。

### 3.3 允许修改的公共接入点

LOG01 修复约束：组合 CLI 启动路径没有启用 tracing 输出，且现有 `im logs` 只读取组级 `state/im_bridge.log`。Mattermost 错误统一写入该原生路径并输出到 stderr，不修改其他平台的日志初始化或增加服务、依赖、独立模块。记录时间、平台、Group、操作和错误；不记录聊天正文、附件内容或原始 HTTP 响应，已配置 Bot Token 在写入前脱敏。日志采用 JSON 单行，复用现有文件锁，Unix 新文件为 0600；1 MiB 轮转保留一份备份，单条错误最多 4096 字符。文件写入失败须在 stderr 明确可见且不能阻断正常收发。测试覆盖追加、隔离、脱敏、轮转及写入失败，然后复测真实超限附件；开发者模式只控制读取，不控制错误记录。

| 位置 | 仅允许的必要变化 |
|---|---|
| [im_runtime.rs](../../crates/cccc-web/src/im_runtime.rs) | 模块声明、start 分派与 Mattermost 自行提交启动状态的代次失效挂接；其他平台路径行为不变 |
| [im_state.rs](../../crates/cccc-core/src/im_state.rs) | 平台集合、MM 字段规范化、必需字段检查 |
| [daemon IM](../../crates/cccc-daemon/src/ops/im.rs)、[Web IM](../../crates/cccc-web/src/routes/im.rs) | 平台校验与同形配置透传 |
| [CLI 参数](../../crates/cccc-cli/src/args/integrations.rs)、[命令转换](../../crates/cccc-cli/src/commands/integrations.rs) | MM server URL 参数及必要透传 |
| [Web 类型](../../web/src/types.ts)、上述 Web 入口及 locale | 新平台、必要字段、既有交互覆盖 |
| [IM 指南](../guide/im-bridge/index.md)、文档导航及用户说明 | Mattermost 安装说明与平台入口，跟随现有组织 |

其余文件修改逐项说明目的；不设置武断的“只能改一个现有文件”限制，因为原生平台注册本就跨这些入口。最小修改指必要、局部、可审查，不是省略后端/CLI 校验。

## 4. 标准使用与授权语义

- 沿用上游“一个 Group 使用一个 Bot 凭据及组级 worker”的约定；部署者负责不在多个 Group 或运行实例中复用同一 Bot 身份，程序没有全局重复检测。不新增跨 Group 连接池、锁或全局路由。
- 更换 MM 站点或 Bot 身份不能把旧接入的授权误用于新接入；同一 Bot 正常轮换 Token 与更换身份要分开验证，不把凭据文本当稳定身份。
- 允许同一 Group 的多个聊天目标分别授权、订阅、暂停和切换 verbose，保留原生外发语义；部署可只批准一个频道，但连接器不硬编码这一限制。
- 支持公共频道、私有频道、直接消息、平台实际允许 Bot 加入的群组直接消息和线程；缺少权限返回正常错误，不借管理员账号绕过。
- 频道目标 `thread_id` 为空时，出站到主时间线；线程目标保存 `root_id`，回复、文件和渐进更新留在原线程。授权匹配沿用公共实现，不擅自让频道批准自动授权所有线程。
- 聊天授权是访问当前 Group 的授权，不是逐用户 RBAC；同组多个频道/DM 共享该组上下文及符合订阅条件的外发内容。配置指南必须醒目标注，不宣称 DM 天然私密隔离。
- **本次规格调整不授权改变现有部署的访问范围。** 若部署仍需 Owner 限制，应另行确认如何在不改变通用连接器语义的条件下落实；实际测试只使用获授权的测试人员和目标。
- 普通群消息必须点名当前 Bot；已识别的 CCCC 命令若作为普通帖子到达，也可作为显式请求。直接消息批准后可直接输入正文。
- 点名 Actor 使用 CCCC 自身 ID，不要求它们在 MM 另有账号；正文提到另一个 Actor 不追加收件人；不做模型名单、角色或业务关键词推断。

### 4.1 命令与 Mattermost 差异

CCCC 的 `/send`、`/subscribe` 等是公共文本命令解析，不代表已为平台注册 Slash Command。MM 对以 `/` 开头的输入有自己的 [Slash 机制](https://developers.mattermost.com/integrate/slash-commands/)，纯 WS 连接不能保证这种输入变成普通帖子。

基础安装统一给出可投递语法，以 Bot 的实际 username 为准：

| 输入 | 公共语义 |
|---|---|
| `@cccc_bot /subscribe`、`@cccc_bot /sub` | 申请当前 Bot 对应 Group 的聊天授权 |
| `@cccc_bot /send 内容` 或 `@cccc_bot 内容` | 发给默认 `@foreman` |
| `@cccc_bot /send @reviewer 内容` | 发给指定 Actor |
| `@cccc_bot /send @all 内容` | 按 CCCC 广播给全部成员 |
| `@cccc_bot /send @peers 内容` | 按 CCCC 发给非 foreman 成员 |
| `@cccc_bot /pause`、`@cccc_bot /resume` | 暂停/恢复该聊天目标的桥接 |
| `@cccc_bot /verbose`、`@cccc_bot /verbose on`、`@cccc_bot /verbose off` | 详细交流开关 |
| `@cccc_bot /status`、`@cccc_bot /help` | 公共状态和帮助，不新增模型调用 |
| `@cccc_bot /unsubscribe`、`@cccc_bot /unsub` | 按公共实现取消该订阅 |

这是可实现的命令等价入口，不是漏掉命令功能。原生 `/rt` 面板、动态菜单和 HTTP 回调不是当前公共连接器已具备的合同，本基础补丁不以它们为前置条件，也不借此另开公网管理入口。

## 5. Mattermost 专有适配

最小配置：`platform=mattermost`、MM 站点地址 `mattermost_url`、`bot_token` / `bot_token_env`；CLI 使用 `--mattermost-url`。沿用原生 Token 别名和规范化方法，不复制真实凭据到提交中。

- REST 和 WS 使用同一已验证站点，支持站点子路径；拒绝 URL 内秘密、跨站携密重定向和不安全路径，不关闭 TLS 校验。测试 HTTP 若需要，明确限制使用环境。
- 启动校验 Bot 身份和 WS 认证结果，失效 Token、权限或不可连接须给正常错误；不报告虚假的 Running。
- 入站使用 MM 帖子真实 user_id、channel_id、root_id、post_id 和 file_ids，排除自身/Bot/系统事件。保留原生 source_*、client_id 和附件元数据。
- 出站使用 REST 创建/修改帖子，按授权目标保留主时间线或线程；对多个目标分别判断流完成，不因目标 A 已流式完成就抑制目标 B 的最终正文。
- `chat.stream` start/update/end 对应同帖创建/编辑；批量更新按平台限制节流，只有完整终态成功后才抑制对应目标的最终消息。保留 Unicode 安全分段、失败后的最终兜底和附件。
- 处理反馈参考原生 reaction 实现，MM 用帖子反应提供等价能力；失败/清理不能清除其他用户的反应，过期清理不等于杀模型。不新增逐 Actor TUI 状态面板。
- 所有 MM 支持的文件类型通过通用 Blob 传递：图片、普通文件及音视频文件；不承诺转写、OCR、PDF 提取、编解码或网页抓取。
- 复用实际公共体积限制与安全文件名，并核验 MM 配置/代理限制；不能把保存的 `files.max_mb` 当成所有旧连接器均已执行该限制的证明。
- 入站使用 Mattermost 原生连接 ID/序号恢复并复用内存去重；外发复用 ledger lag 补读。这两种补收方向不同，均不保证跨重启历史自动回放。不新增后台积压任务重放、持久待发箱或 exactly-once 承诺。
- 错误使用现有 tracing 和运行状态路径，错误内容脱敏；401/403 不扩大权限，429 按平台提示退避，辅助反应失败不阻塞正文。

全部功能及依据详见 [对照表](mattermost-im-features.md)。这里只说明实现边界，不替代逐项验收。

## 6. 从前版规格移出的业务与额外增强

以下内容不再作为通用原生连接器基础补丁的强制架构：会议主持/轮数/结束与总结、Topic 生命周期、自动筹备组、硬编码 Owner 白名单、一个 Bot 跨组路由、一组只能一个聊天入口、强制所有回复到主时间线、默认 verbose 开启、全量频道资料登记、独立投递数据库、跨重启补发、`/rt` 控制中心及新建 Actor 状态卡。

这是本次新要求下的范围拆分，不是宣称这些需求已实现或无价值。此前明确提出的部署升级回退、完整运维日志和专用错误频道上报仍保留在部署工作要求中；是否加入运行时连接器作为可选增强，后续单独确认，不把它们伪装成现有原生功能。

## 7. 开发及完成标准

1. 先完成本规格、源码风格清单和逐项功能映射；所有“可直接/等价实现”的功能均进入验收，不只做演示性消息收发。
2. 按参考模块实现平台及配置接入，再补消息/命令/授权、DM/频道/线程、公开外发、流式、附件、反应、重连和错误。
3. 同步完善现有 Web 设置和 CLI，不以手工写 JSON 代替可用的 Web 配置。
4. 针对性单测和模拟 REST/WS 测试通过后部署独立测试构建，通知用户亲自测试；不改现有 CCCC/Discord 服务的授权或配置。
5. 逐文件审查最小补丁、运行现有相关 CI，并完成真实功能验收后发布自己 Fork 的 CCCC 构建。上游 PR 等用户另行决定。

当前代码已接入 Rust/TS 配置、收发 worker 和协议模拟测试。CCCC 仅部署在独立应用服务器，Mattermost 测试服务器只提供聊天接口。此前误部署已撤销。后续已完成真实联调、日志缺陷修复及相关回归；早期 Linux 测试失败及最终串行复验的证据均保留在验收记录，不将阶段性失败当作当前状态。用户现已确认完整验收结果正常，并授权在执行全历史秘密扫描及人工检查后，推送公开 Fork 并向上游提交 Issue。PR、Release、默认分支变更及部署不在此次范围内。
