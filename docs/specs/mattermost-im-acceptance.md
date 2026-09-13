# CCCC Mattermost 连接器验收

日期：2026-09-07，提交前回归更新于 2026-09-13。对应 [规格](mattermost-im.md) 和 [功能清单](mattermost-im-features.md)。此前面向特定业务的验收表已被本表替代；**T01–T19 技术验证已完成，T20 已获用户明确确认“我已经验收完了，都正常”。真实平台、协议模拟、共享回归和用户确认分别记录；验收完成不等于上游已合并或正式发布。**

## PR #103 第三轮审核修订复验（2026-09-13）

基于 `436028de741031be7b6048bf0cb2f58f261ef64a` 修复本轮三项意见，继续更新原 PR，不涉及 CLI 管理、其他连接器行为或日常部署。

| 修订 | 验证及结果 |
|---|---|
| 过期启动不得覆盖新状态 | `superseded_http_start_cannot_overwrite_save_stop_unset_or_new_start` 卡住真实 HTTP 入口的旧启动，分别保存新配置、保存相同配置、停止、删除或再次启动，再释放旧请求；两种 WebSocket 结果共十个组合，新状态完整保留。`stale_success_and_error_commits_are_both_discarded` 分别验证过期成功及错误不能写回 |
| 永久认证失败不无限重连 | `permanent_reconnect_authentication_failure_stops_registered_worker` 验证 401、403、认证拒绝三种情况：原生 worker 结束，HTTP 状态不运行、不可用、无 PID，保留具体错误；再等待超过原退避周期，没有新增请求。临时 503、429、非法 JSON 仍可重试；既有断线恢复测试继续通过 |
| 明确工作组草稿生命周期 | 两个实际 SettingsModal 用例均通过，新增 A→B→A 且不编辑 B 的情形，返回 A 后草稿也已清空；同组平台切换和保存只作用于当前组。两平台真实网页在设置窗不卸载时换组，验证清空、重新填写、同组恢复和保存回读 |

- Linux 针对性 Mattermost 测试 28 通过、2 个真实站点 live 用例默认忽略；Ruff、111 项 Python、Web format/lint/typecheck、293 文件的 1,524 项单测、生产构建、25 项打包用例及 wheel/Twine、Rust fmt/Clippy 和安装器/发布资产通过。独立夹具类型检查、完整非 daemon workspace、daemon 串行全量（含 525 项库测试）、三项自启动及按 CI 条件启用的 Codex/Claude/Kilo 原生会话检查全部通过，`ALL_LINUX_CHECKS_PASS`。Kilo 筛选中的 OpenCode 条件用例未启用，不称为真实 OpenCode 联调。
- Windows 原生测试：核心 IM 8、完整 IM runtime 195（2 live 默认忽略）、七组 Windows smoke 共 11 项及构建通过，`ALL_WINDOWS_CHECKS_PASS`。既有平台条件编译警告保留，不修改无关模块。
- 两平台原生 Web 的完整配置、无效地址、保存/网络/启动失败及重试、刷新持久化、三语、窄屏、删除配置及保持挂载的跨组草稿隔离均通过，`CROSS_GROUP_GUI_PASS`、`GUI_PASS`。人工检查 Linux 深色及 Windows 浅色截图；Linux GUI 二进制与最终构建的校验和一致。只使用隔离 Home、合成 Group 和凭据引用，浏览器及服务已关闭，原资源恢复。
- 首次 Rust 编译发现代理测试仍直接对新错误类型调用字符串方法，改用文本表示后通过；独立夹具检查第一次在错误工作目录查找 `vite/client`，改从 Web 项目目录运行后通过。没有放宽生产类型、修改依赖或把失败轮次算作通过。

所有测试、构建、浏览器操作和发布扫描仅在指定测试机执行。本轮没有实际调用 Mattermost 或模型，不把协议模拟替代真实站点验收；既有现场记录保留。推送仍以前置全历史及增量 Gitleaks、两平台源码一致性和文档链接检查通过为门禁；不合并、不发布、不更换日常运行实例。

## PR #103 第二轮审核修订复验（2026-09-13）

本次基于 `90fb70017525615fbde37d28066ffb341638d747` 修复第二轮审核的四项意见，仍只更新原 Mattermost PR，不涉及 CLI 管理、公共调度或其他连接器行为。

| 修订 | 具体证据 |
|---|---|
| 附件处理移出 WebSocket 循环 | `slow_attachment_does_not_block_socket_pongs_or_reorder_inbound` 实际启动四个 worker，阻塞文件下载期间仍回应服务端 Ping，之后的 `/help` 不越过附件请求 |
| 有界处理及顺序 | `socket_backpressure_keeps_sending_heartbeats_and_preserves_order` 把队列缩为 1，超过三次心跳周期仍发送 Ping，不因本地背压误重连，恢复后事件按序到达；接收端关闭后 socket 任务退出。满队列暂停读取的限制见指南，不承诺无限负载下不断线 |
| 完成反应早于 ID 绑定 | `completion_waits_for_dispatch_binding_and_is_applied_only_once` 验证成功/失败完成均等绑定，重复完成不重复反应，无关完成不清除当前请求；保留既有失败及清理路径 |
| Mattermost 草稿不跨组恢复 | 新 `SettingsModal.mattermost.test.tsx` 保持同一组件挂载，仅改变 Group；同组恢复，跨组 URL/Token 清空，保存只提交新组内容。测试机临时撤去草稿补丁时，此测试确实因恢复旧组 URL 而失败；恢复补丁后通过 |
| 既有类型夹具补齐 | 补 `mattermostUrl` 及既有 `weixinAccountId` 必填项，不放宽类型；另以项目 TypeScript 配置显式纳入 `web/tests/` 的该夹具检查，通过 `FIXTURE_STRICT_TYPES_PASS` |

Linux 定向协议测试 28 项通过、2 项凭据依赖 live 用例按默认规则忽略；前端定向测试 27 项通过。Windows 核心 IM 8 项、完整 IM runtime 191 项及七组 11 项 smoke 通过，原生编译通过。新异步用例包含在两平台测试中，不把零项筛选算通过。

完整复验结果：

- Linux：Ruff、111 项 Python；Web format/lint/typecheck、293 个文件的 1,523 项单测及生产构建；25 项打包测试、wheel/Twine 与产物形状；完整 Rust fmt/Clippy、安装器/发布资产、非 daemon workspace、daemon 串行、三项自启动及按 CI 条件启用的 Codex/Claude/Kilo 原生会话检查通过。首次 workspace 的未修改 `verified_live_owner_is_recovered_after_hostname_changes` 报 `Text file busy`；准确筛选到该项复跑通过，随后完整 Rust 流水线重跑至 `ALL_RUST_CHECKS_PASS`，未修改或跳过此测试。不能把首次完整命令记为成功。
- Windows Server 2025：上述原生回归及构建最终 `ALL_WINDOWS_CHECKS_PASS`；保留既有编译警告，不为本补丁屏蔽或修改无关模块。
- 两平台真实 Web：原有配置、无效 URL、保存/启动失败与重试、持久化、三语、窄屏及清除配置流程全部重跑；新增原生 Group 删除事件驱动的换组，验证同一个设置窗保持挂载，另一组 URL/Token 不恢复，新组草稿切换和保存回读正确。只删除独立 Home 内本次创建的合成组，无现用资料；浏览器和隔离服务已关闭。人工检查 Linux 浅色、Windows 深色的跨组截图。
- 新单测首轮发现夹具字段缺失并补齐后全量通过；浏览器脚本早期失败来自 DOM 返回值、隐藏 dialog 选择器和 Windows 命令参数解析，修正脚本后完整重跑，无产品代码为这些定位问题改变。失败批次保留，不混入通过记录。

全部执行于指定测试机，本机未运行测试、构建或扫描。模拟 HTTP/WS、合成 Group 和浏览器错误响应不等于重新进行真实 Mattermost 收发验收；两项默认忽略的 live 用例未在本轮重跑。最终提交仍须通过全历史和增量 Gitleaks、两平台源码一致性及文档链接门禁后才能推送；日常实例没有部署变更。

## PR #103 审核修订复验（2026-09-12）

本次在 `e73666d7682e0a1546a651c22dcb090605c0b52f` 之后修复审核意见，继续更新同一个 PR，不新增 Issue、合并或部署。

| 修订 | 验证方式 | 当前结果 |
|---|---|---|
| 去掉点名后的空消息不回复、不调用；纯附件仍处理 | 同模块协议测试：四类频道、五种空正文/裸点名，以及四类纯附件授权请求 | Linux、Windows 均通过 |
| Mattermost 站点 URL 校验及保存/启动禁用 | 原有配置工具与页面单测；有效站点、子路径、IPv6、错误协议、凭据、查询、片段、API 后缀 | 29 项针对性前端测试通过 |
| 保存/启动失败可见并可重试 | 原生 Web + 浏览器模拟保存拒绝、网络失败、启动前保存拒绝；另测真实缺失凭据启动失败 | Linux、Windows 真实页面均通过；失败不保存/不启动，解除故障后可保存重试 |
| Bot 独立身份是部署要求，不是全局互斥保证 | 指南、三语 UI 和规格核查；不新增跨 Group/实例的 Bot 注册或锁 | 文案已统一，原授权模型不变 |
| 英文平台表语言一致 | 对照其他平台单元格人工核查 | Mattermost 行已改为英文 |

完整复验结果：

- Linux 指定测试机：Ruff、111 项 Python；Web format/lint/typecheck、292 个文件的 1,522 项测试和生产构建；25 项打包测试、wheel/Twine/产物形状；workspace fmt/Clippy、安装器/发布资产、非 daemon workspace、daemon 串行、3 项自启动回归，以及 CI 规定的 Codex/Claude/Kilo 原生会话检查全部通过，最终 `ALL_LINUX_CHECKS_PASS`。
- Windows Server 2025 指定测试机：原生编译、8 项核心 IM、188 项 IM runtime（2 项真实服务 live 用例按默认规则忽略）、上游 Windows smoke 七组 11 项实际测试通过，最终 `ALL_WINDOWS_CHECKS_PASS`。
- 两平台原生 Web：无效地址及修正、保存拒绝、网络故障、启动前保存拒绝、失败不保存/不运行、修正重试、跨平台草稿隔离、保存回读、刷新持久化、缺凭据启动失败、中英日三语、390×844 窄屏和删除配置均通过；并人工检查 Linux 浅色、Windows 深色下的提示及换行截图。隔离服务/浏览器已关闭，日常实例没有更新。
- 首轮检查捕获并修复一处新增错误状态的 TypeScript 空值判断；随后完整重跑通过，不把首次失败算通过。已有 npm 依赖审计提示和 Windows 编译警告保留，没有更换依赖或屏蔽告警。

术语继续沿用 Group、Bot、聊天目标、授权及 `attachments`，不引入会议/roundtable 概念。以上协议和浏览器故障使用隔离测试资料，不冒称真实 Mattermost 断网或重新完成现场收发验收。本机没有运行测试、构建或扫描。最终推送另需全历史及新增提交的 Gitleaks 扫描、源文件一致性和文档链接核查，原始证据保留在仓库外。

## 上游 PR 提交前复验记录（2026-09-12，审核修订前）

- 本轮已获授权准备上游 PR，关联既有 [Issue #99](https://github.com/ChesterRa/cccc/issues/99)，不另建重复 Issue、不直接合并主分支、不发布 Release、不更新日常部署。下方早期“只发 Issue、不创建 PR”的限制是当时边界，不代表本轮授权。
- 从已发布的连接器分支合并上游 `22733e9ac607989bb095a4a6b7cb0e518bab9d08`，产品代码复验点为 `8656a48e841fa7f5aded39700711e88c1b9c9955`。后续仅整理本验收记录与指南文字。净差异限定于 Mattermost 的原生 IM 接入、测试和文档；不含 CLI 管理、Experimental 页面、全仓词汇表、依赖或 CI 工作流变更。来源校验测试保留上游原样。
- **Linux 指定测试机通过**：Ruff、111 项 Python 测试；Web 静态/类型检查、292 个文件的 1,501 项测试及生产构建；25 项打包测试、原生 wheel 检查、Twine 和产物结构断言；workspace fmt、all-targets Clippy（`-D warnings`）、安装器/发布资产脚本测试。随后依上游 CI 顺序执行非 daemon workspace 回归、daemon 串行回归、组合进程 3 项自启动测试及固定版本 Codex/Claude/Kilo 的原生会话测试，最终 `ALL_LINUX_CHECKS_PASS`，退出码 0。
- **Windows Server 2025 指定测试机通过**：原生编译；`im_state` 8 项；IM runtime 187 项通过、0 失败、2 项真实 Mattermost live 测试按默认规则忽略；上游 Windows smoke 的七组共 11 项实际测试通过（PTY、挂起进程、Job、控制台编码、Web 绑定失败、异常退出、Kilo 启动）。最终 `ALL_WINDOWS_CHECKS_PASS`，退出码 0。已有 Windows 模块编译警告未通过无关补丁或全局 suppress 隐藏。
- **两平台真实 Web 配置页通过**：以各自原生二进制启动独立 Home/Group，通过浏览器操作原生“设置 → 当前工作组 → IM 桥接”：Mattermost 平台选择、空值禁止保存、站点与环境变量引用填写、跨平台草稿隔离、保存后 API 回读、刷新持久化、缺失凭据时失败可见且不运行、中/英/日三语言、390×844 窄屏及删除配置后 API 确认均完成。浏览器会话和隔离服务已关闭；没有调用 Actor、使用真实 Bot Token 或向真实频道发帖。本轮截图不补签新的深色主题验收，历史主题记录仍单独保留。
- 本轮浏览器脚本的早期失败来自隔离 Windows Home 缺少 AppData、异步设置面板等待、选择器转义/快照格式，以及 Linux debug 二进制读取旧测试目录静态资源。按原生运行机制修正测试环境与定位后，两平台从全新数据目录完整重跑通过；没有修改连接器业务代码、放宽产品断言或把失败尝试计为通过。Linux 初次 Python 失败来自归档缺 Git 元数据及先前 root 产物权限；补齐真实 Git 状态并恢复非 root 夹具后，完整回归通过。
- 两轮源码核查分别检查原生接入及既有行为、认证/文件隔离/重试/最终输出合同；这是同一执行者的两轮审查，不冒称独立审核者。命名继续采用 Group、Actor、Bot、聊天目标、授权、订阅、`attachments` 与 `refs` 的原生含义。
- **证据边界**：本轮重跑隔离协议和双平台 GUI，不冒称重新登录全部聊天平台，也不把默认忽略的真实 Mattermost 用例算作本轮执行。真实测试服收发、附件、线程、流式和用户验收证据见下方 2026-09-08 记录。本机未运行测试、构建或扫描；标准实例、日常开发实例及其登录数据未修改。
- 推送前继续执行全部 Git 历史和最终提交的 Gitleaks 门禁；原始脱敏报告、服务器日志和截图保存在仓库外，不将凭据或真实内部部署信息带入公开补丁。

## 2026-09-08 历史验收状态

- 用户最终验收：2026-09-08 明确反馈“我已经验收完了，都正常”，据此关闭 T20。后续明确授权公开 Fork 推送和上游 Issue，替代此前的暂停指示；本次不修改已验收的运行代码或 Actor 提示词，不创建 PR 或 Release。
- 推送门禁：每次推送前使用 `gitleaks` 或 `trufflehog` 扫描全部 Git 历史及最终待推送提交，并做人工检查；报告脱敏后保存在仓库外。误报须逐项核实，上游历史例外须有明确批准，不能以目录白名单忽略新增告警。待推送提交或范围变化后重新扫描。
- 文件回传现象的责任边界：用户上传的 TXT 已正常进入 CCCC，Codex 读出编号 73；随后两条 Bot 帖子的 `file_ids` 均为空。“附件补发”对应的 CCCC 消息只有 `refs` 文件引用，没有 `attachments`。因此该次回传未成功，不能追记为成功发送，也不是 Mattermost 隐藏同名文件。Mattermost 与当前其他原生连接器一样，仅按 `attachments` 传输文件，不将 `refs` 擅自转换为附件；本例未证明连接器缺陷，不改变既有正确附件路径的通过结果。
- CCCC 原生说明要求使用 `cccc_file(action="send", ...)` 交付文件，发送路径必须在当前工作目录范围内。收到的 `state/blobs/...` 可读取或解析路径，但不能直接作为该发送操作的输入；回传时应先复制到工作目录，再调用文件发送工具并检查结果。补强 Actor 操作说明属于独立改进，本次未实施，也不将其加入连接器补丁。依据：[原生说明](../../resources/cccc-help.md)、[文件工具](../../crates/cccc-mcp/src/local_tools.rs)、[Mattermost 出站](../../crates/cccc-web/src/im_runtime/mattermost_outbound.rs)。
- FILE02/NET02 通过：下载元数据和文件正文分别 403 时不留下 Blob；完整 POST 已被服务器读取但响应丢失时不重发。IM runtime **187 passed、0 failed、2 ignored**（16.43 秒），clippy 32.27 秒退出 0；两个默认忽略项均是必须显式指定真实凭据和目标的 live 测试。
- CODE01 真实 MM 通过：合成长代码、中文/Emoji、换行、代码围栏和链接共 **22,158 字符**，两个帖子按服务端顺序拼接与完整带作者原文逐字相等；每帖不超过 16,383 字符，保留原线程及 Bot 作者。空白 sender_title 正确回退 reviewer。测试仅发送协议样本，没有调用 Actor。测试耗时 0.90 秒，1 passed；跨帖代码围栏只承诺原文无损，不承诺每个分片独立呈现完整代码块。
- UI02 检查通过：只补原生 IM 说明中漏列的 Mattermost 名称，沿用中文/英文/日文 locale，不改变布局。4 个前端文件的 12 项定向测试通过；格式化 814 文件、lint 781 文件及 TypeScript 检查通过；构建 27.99 秒完成（保留既有大分块警告，不改打包策略）。新资源备份后只更新独立实例静态目录，不重启 Actor。三语言通过真实页面原生选择器切换并读取说明和 MM 表单，凭据继续显示环境变量引用。
- GROUP02 通过：同一临时 Home 下两个 Group 使用不同合成 Bot 身份/Token 和 HTTP/WS 模拟服务，分别验证配置保存回读、授权/verbose、定向出站、撤销一组不改变另一组及错用凭据 401。没有创建真实账号或修改现有绑定；不冒称两个真实 Bot 联调。首次构建遇到测试夹具 HTTP 头类型比较错误，修正后最终 **188 passed、0 failed、2 ignored**（15.75 秒），clippy 27.76 秒退出 0。默认忽略的两项 live 测试均另有明确通过记录。
- 最终文件核对：本地三个 Mattermost 文件与远端测试挂载逐个 SHA-256 一致，`git diff --check` 通过；Cargo manifest/lock、依赖和发布工作流未改变。前端入口本地构建与远端一致，三语言真实页面复查并恢复中文、自动主题。随后 T20 已获用户明确确认，不以自动化结果代替用户签署。
- LOG05 原生 `im logs -f` 真实通过：未绑定聊天的空白合成组使用无效测试凭据触发两次实际 HTTP 401，新记录分别在 0.85 秒、0.83 秒内显示，跨两个轮询间隔没有重复；Ctrl+C 退出 0。测试脚本清除合成组配置，恢复原可观测性配置，正常组桥接及订阅不变。不是手工写入日志夹具。
- CMD02 真客户端补齐 `/verbose` 无参数、true、false、1、0、非法值及最终 `/status`。均使用输入框和 @Bot 前缀，得到预期启用、关闭或用法提示；最终 authorized=true、paused=false、verbose=false。与先前 on/off、配对别名及其他命令证据合并，不重跑已通过的相同步骤。
- NET01 隔离验证通过：REST/WS 经 HTTP 代理、NO_PROXY 绕过、失效代理及代理后 WS 401；向明文测试端口发送 TLS 明确失败且不回显测试凭据。首次代理用例因夹具 `/sub` 与请求 `/mm` 不一致而返回 404，修正测试地址后为 **182 passed、0 failed、1 ignored**（15.88 秒），clippy 退出 0（33.00 秒）。保持无外网，没有修改真实代理或服务器证书；不宣称已验证过期证书或 HTTPS CONNECT 代理。
- AUX01 模拟补测通过：403 反应/上传失败记录完整错误且正文继续送达、非本组 Blob 不上传、403 创建请求只尝试一次，以及编辑/无关/其他 Bot 消息忽略和重复 posted 去重。最终 **185 passed、0 failed、1 ignored**（15.53 秒），clippy 退出 0（35.05 秒），`git diff --check` 通过。仅新增同模块断言，不改变线上业务代码。这里不包含传输中断后非幂等重试的模拟证据。
- **LOG03 修复后真实错误日志链路通过**：新上传 10 MiB + 1 字节合成附件后，用户收到失败提示；组级 im_bridge.log 产生一条包含 UTC 时间、Group、platform、operation=inbound、超限原因的 JSON 记录。原生 CLI 返回内容与文件逐行一致，Docker stderr 中同条记录的 SHA-256 也一致。Unix 文件权限 0600，文件中未检出实际 Bot Token 或管理员令牌。测试时临时开启开发者模式仅供读取，finally 中恢复 false，其他可观测性配置不变。
- 修复限定于现有三个 Mattermost 源文件：统一错误记录入口覆盖认证、连接、身份核验、入站、出站、附件上传、错误回复、WS 重连及反应操作；复用原生组日志路径与文件锁，不改其他平台、不初始化全局 tracing、不增加依赖。错误脱敏、JSON 单行、长度限制及轮转行为见规格与指南。新增两个同模块用例覆盖追加、重复错误、组隔离、换行/凭据脱敏、0600、状态清除后保留日志、轮转及文件写入失败不 panic。
- 本轮 IM runtime **180 passed、0 failed、1 ignored**（显式 live 用例不默认运行），15.24 秒；定向 clippy 退出 0，47.60 秒；CLI 程序构建退出 0，1 分 49 秒。同镜像启动检查成功后只替换独立测试实例，旧容器和二进制保留，原 CCCC/CAO 健康。本轮未重跑全部 workspace/Web 测试，不以历史结果冒充本轮全量结果。
- LOG04 恢复问答通过，真实 Codex 返回“LOG04 恢复正常：37”。容器更新后 Actor 需要显式启动，启动更新提示曾再次触发只读安装目录的 EACCES；未提权升级。给独立测试 Actor 使用既有关闭启动更新检查的命令后正常回复。最后仅重启桥接清除 last_error，订阅仍为 1，错误日志记录仍保留。
- LOG02 复用已绑定附件 ID 的尝试未被 MM 保留 file_ids，因此不计入故障复测；随后 LOG03 重新上传并核对实际大小、附件关联后才计为通过。下方 LOG01 保留为修复前失败证据，不代表当前仍缺失日志。
- **LOG01 真实错误日志验收失败**：在独立验收组发送 10 MiB + 1 字节的合成附件。MM 接受帖子后，连接器向用户发送失败提示，last_error 持久化为 `Mattermost attachment exceeds configured size limit`；临时开启开发者模式后 `im logs --lines 100` 退出 0，但其读取的组级 `state/im_bridge.log` 实际不存在、结果为空。Web/daemon 对应日志文件也不存在；Docker json-file 日志在本次触发时间之后为 0 行。错误状态可见不等于错误日志已落盘，不计为日志功能通过。
- LOG01 源码定位：入站失败返回 socket loop，后者调用 `tracing::warn!` 并更新 last_error；persist_error 只写 im_state，没有追加日志。当前组合 CLI Web 启动路径没有找到 tracing subscriber 初始化；独立 Web main 虽有初始化，但不能据此视为组合 CLI 已初始化。仓库只找到组级 im_bridge.log 的读取端，没有找到对应写入端。修复需同时核对实际启动路径与组级日志读取合同，不以手工写日志或仅放宽过滤级别掩盖缺口。
- LOG01 恢复：后续普通算术请求得到真实 Codex 回复“LOG01 恢复正常：17”，证明运行未持续中断；成功回复后 last_error 仍保留前次错误，不能声称自动清除。证据记录后仅 stop/start 测试组桥接，恢复 last_error=null、订阅 1；未重启容器或 Actor。开发者模式已恢复 false，其他 observability 设置未变，原 CCCC/CAO 健康。保留合成帖子及附件，未修改业务源码。
- 2026-09-08 授权后的补测：已通过正常 bootstrap 接口创建独立测试实例管理员令牌。创建前确认上游禁止删除最后一个管理员令牌，用户另行明确批准保留；不再描述为可撤销的临时令牌。CLI 的 set/config/start/stop/status/unset、pending/reject/bind/authorized/revoke 均真实执行成功；重复 bind 返回 400，重复 revoke 返回 false。此前 GET 成功不能解释写操作权限，显式端口也不能替代管理员身份。
- 公开频道补测通过：使用全新空白 Group、独立工作目录和单个临时 Codex Actor，只发送合成算术请求；真实回复“公开验收结果：56”到达主时间线。测试期间停止原 Group 的同凭据桥接，结束后撤销公开目标、停止并清除新组桥接配置、停止临时 Actor，再恢复原组桥接；原组订阅数 1、last_error=null。未把原私有组绑定到公开频道。
- 原生界面补测：实际键盘切换深色、英文和日文，Mattermost 地址、凭据引用说明及操作按钮可显示和滚动，未暴露真实 Token。检查后恢复中文、自动主题。发现通用 IM 桥接说明仍漏列 Mattermost，保留为文案待修项；不声称完整多语言审校完成。
- CLI 日志入口先返回 `developer_mode_required: developer mode is disabled`。用户明确批准后，通过正常 observability API 临时开启开发者模式，两组 `im logs --lines 5` 均退出 0、component=im、lines=[]。随后立即关闭并回读确认 false，其他 observability 配置完全相同；关闭后再次调用恢复拒绝。只验证接口及开关门禁，不将空结果当成实际日志内容或错误落盘已验收。未改运行中生产代码或提前推送。
- 2026-09-08 用户已明确反馈“测试没问题”，基础收发的用户亲试通过；随后开始剩余功能验收，不能将此反馈扩展成附件、线程、流式及故障用例全部通过。
- 真实客户端补测：`/help`、`/verbose on`、`/verbose off`、`/pause`、暂停期间普通 mention、`/resume`、空 `/send`、未知命令和 `/status` 均收到预期协议回复。末态 authorized=true、paused=false、verbose=false，回复均在频道主时间线。其余别名、目标和故障路径仍待验证。
- Linux Rust 回归已分段复验通过：workspace 非 daemon 部分串行通过；daemon 全部 lib/集成/文档测试通过；CLI `daemon_self_launch` 三项通过。首次非 root 并行运行的终端 WebSocket 失败未在单独/串行复验中重现；daemon 首次配置目录权限失败通过临时测试目录消除。保留失败记录，不声称原并行命令已经通过。

### 2026-09-08 本轮新增真实证据

- **权限与配对**：同一私聊重复 `/sub`、`/subscribe` 保持同一待审批键。原生 Web 拒绝后请求消失，再次申请生成新键；Web 批准后普通私聊文本交给 foreman 并得到真实回复。未授权正文只获得授权说明。无管理员凭据的 CLI 审批返回 401，未绕过；不把该 CLI 审批路径标为成功。
- **多目标语义**：私聊与频道批准绑定同一 Group 后，面向 user 的模型回复同时到达两处，符合原生共享语义。不能据此声称私聊上下文隔离。
- **路由**：新增临时 Codex peer 后，指定 peer 的正文引用 foreman 没有扩大收件人；实际只有 peer 回复。`@all` 得到两位真实 Actor 回执；`@peers` 只投递给 peer。未知 Actor 返回受控失败消息，未改投其他 Actor。
- **部署问题**：新 Actor 的升级弹窗曾消费投递回车，导致 npm 更新尝试被只读安装权限拒绝。未提权或成功更新；为临时 Actor 加入关闭启动更新检查的命令参数后重测成功。此属 CLI 启动条件，不修改连接器路由以掩盖失败。
- **线程**：频道已经授权时，新线程仍要求独立批准；未授权消息、配对说明及批准后的真实模型回复均保留原 root_id。主频道和其他已授权目标仍按原生共享规则收到公开外发。
- **文本文件双向**：原生 MM 上传合成 TXT，入站 Blob 与模型实际读取正确；模型通过文件工具回传后，下载字节数与 SHA-256 均等于原件，中文文件名保留。
- **多媒体**：合成 PNG、PDF、WAV 混合上传后，由实际 Actor 回传原文件；下载哈希逐一相同。第一份 WebM 仅含容器头，未计入通过；修正后带图像帧的 2,703 字节 WebM 经真实 Actor 回传，下载哈希与输入相同。不承诺模型识图、音视频转写或播放器效果。
- **仅附件及超限**：空正文、仅上传 TXT 的真实帖子被表示为 `[attachment]` 并附带本组 Blob；真实 Actor 成功读出合成资料。10 MiB + 1 字节的文件虽被 MM 接受，但被连接器拒绝，产生受控错误及失败反应，未转交模型。
- **生命周期与无效凭据**：停止桥接后 Actor 仍在运行；重启保留授权且不回灌断桥期间的旧帖子。使用无秘密的无效 Token 样本启动时明确返回 HTTP 401，恢复原环境变量引用后启动成功，没有轮换真实 Token 或修改访问策略。
- **长文本**：明确标识为协议注入、非模型回答的中文/Emoji/换行样本被拆成两条；16,383 与 1,671 个 Unicode 字符按顺序拼接后精确等于完整的带作者正文，没有丢字。
- **独立暂停**：只暂停私聊订阅时，协议样本到达主频道与已授权线程，不到达私聊；恢复私聊后没有补发旧样本。此为投递开关，不是上下文隔离。
- **群组私聊**：测试服允许 Bot 与两名已批准测试人员加入三人群组私聊；独立批准后，无 mention 的普通文本交给 foreman 并收到真实回复。
- **过期与撤销**：真实等待 600 秒后，未批准申请从列表消失，旧键批准返回 HTTP 400 `pending request not found`。`/unsub`、`/unsubscribe` 分别移除群组私聊和私聊订阅；Web 撤销线程成功，重复撤销返回 `revoked=false`，不影响主频道。当前仅保留原主测试频道订阅。
- **回归**：此前失败的 `branding_upload_rolls_back_staged_asset_when_daemon_commit_fails` 在 UID 1000 环境实际通过（1 passed），未降低断言。
- **Web 本轮回归**：283 个文件、1,419 项单测通过，耗时 536.55 秒；format、lint、TypeScript 全部通过。真实键盘清空站点后保存禁用，恢复站点后保存启用；切换至 Telegram 不带入 MM 凭据，切回 Mattermost 草稿恢复。本轮没有为此保存其他平台配置。
- **剩余 Rust 复验**：终端 WebSocket 文件两个用例串行独立通过，workspace 非 daemon 部分串行通过。daemon 第一次因测试使用默认不可写的 Claude 配置目录失败；配置仅用于测试的 `CLAUDE_CONFIG_DIR` 后，其 486 项 lib 测试及全部集成/文档测试通过；随后 CLI 三项自启动回归通过。没有修改真实 Claude 登录或给测试提权。
- **真实流式协议**：新增默认忽略、必须显式指定测试站点/频道及凭据文件才运行的同模块测试。测试通过真实 Bot 在主时间线和测试线程分别执行 start/update/end，逐阶段读取 MM 帖子确认正文及 ID 不变；最终 chat.message 没有重复创建。保留一条测试根帖与两条最终结果，明确标注“非模型输出”。此不是 PTY Actor 已产生 chat.stream 的证据。
- **最后定向回归**：真实流式测试 1 项通过；随后 IM runtime 175 项通过、0 失败，1 项需凭据的 live 测试按默认规则忽略（此前已单独实际运行通过）。新改动仅增加测试，没有修改运行中连接器业务代码。
- **窄屏**：390×844 下实际检查浅色 Mattermost 配置表单，地址、凭据引用与说明均在面板内；保留原生横向设置导航及滚动布局。深色及完整多语言人工验收尚未完成。

以上是局部路径证据，不将 T01–T20 的整行要求自动标绿。

### 用户最终确认前的边界记录（历史）

- 隔离故障补测已通过：WS HTTP 401 拒绝、握手后认证错误/非法 JSON 均返回明确且无 Token 的错误；模拟服务关闭连接后，经原有重连流程恢复，观察到持久化 last_error 从断线错误变为 null。流式首帖 HTTP 403、update 编辑失败后，完整最终消息仍发送；此前 end 失败及多目标兜底测试继续通过。仅模拟服务证据，不冒充对真实 MM 注入断网。
- 本轮 IM runtime：最终重跑 178 passed、0 failed、1 ignored（显式 live 测试，不默认访问外部站点），耗时 15.31 秒；`cargo clippy --locked -p cccc-pair-web --lib --tests -- -D warnings` 随后通过，退出 0。静态检查首次指出新增断言应使用 `expect_err()`，已修正并重跑，没有忽略警告。
- CLI 管理写操作的授权缺口已解除；LOG01 日志缺陷已修复并由 LOG03 真实复测确认。轮转、脱敏、组隔离和写入失败是自动化证据，不冒充真实服务器磁盘故障注入。
- 公开频道已用独立合成 Group 验证并解除绑定。各目标类型有真实收发证据，但不能推导为不同订阅拥有独立上下文。
- 深色英文/日文配置表单补查已完成，通用说明漏列平台待修；完整代理/TLS/WS 故障注入及全部流式失败分支仍需按用例核对。现有模拟或共享回归覆盖不等于每条真实故障都已复现。
- 用户仅确认基础收发；不替用户签署完整 T20 体验验收。不因自动化回归已通过而提前发布、推送或提上游 Issue。

### 早期阶段记录（非当前状态）

- 2026-09-08 最新：独立 Codex Actor 已接入并完成真实 MM 两轮问答。指定 Actor 的 `/send` 和普通 mention 默认 foreman 路由均得到真实模型回复，第二轮承接上一轮上下文，答案到达主时间线。只通过该 Actor 的基础路径，不等于 T07 全部路由或文件/流式验收通过。用户亲试仍待反馈，下方 Actor 数 0 为此前阶段记录。

- 已通过：Web 283 个文件、1,419 项测试；IM runtime 175 项、来源校验 8 项；开发程序构建；workspace clippy；Python 112 项及发布资产/安装器脚本测试。
- 未通过：Linux 全量 Rust 回归。第一轮缺少 Node；补齐 Node 后第二轮在 `branding_upload_rolls_back_staged_asset_when_daemon_commit_fails` 失败（实际 200，预期 400）。
- 该用例依赖目录权限 0555 令写入失败；此前测试容器以 root 运行，因此先在非 root 环境复验。当前这是源码支持的原因推断，尚非重跑后的结论，不修改业务逻辑或降低断言来通过测试。
- 误部署的开发实例已撤销，没有仍在聊天服务器运行的 CCCC 测试任务。后续只在独立 CCCC 应用服务器构建和部署。
- 已在正确应用服务器部署独立新实例，原生 Web 建组、中文切换、Mattermost 配置保存及 CLI 回读通过。已提供入口邀请用户亲试；真实 Mattermost、完整 UI 用例、用户亲试、部署回退及专用错误频道上报仍未验收，未提交 GitHub 或上游 Issue。
- 测试 Bot 已通过原生管理页面创建：用户完成正常密码和 MFA 登录，Bot Token 的 users/me 返回 200、is_bot=true、roles=system_user；未开启全频道发帖权限。尚未加入测试频道或启动 CCCC 桥接，不能将身份验证等同于收发验收。
- 后续已在独立 CCCC 实例注入 Token 并启动桥接，原生 API 显示 adapter_available=true、running=true、last_error=null，REST/WS 启动通过。Bot 加入团队被现有团队人数上限拒绝（上限 50、活动成员 50）；未变更配额或移除成员，待用户批准。测试频道未创建、订阅数为 0，收发与配对尚未验收。
- 2026-09-08 更新：用户明确批准调整测试服人数上限后，已完成配置保存与 API 回读。Bot 成功入队，独立私有频道已建立；真实客户端发送 subscribe、核对后批准、再次发送 status 均成功。订阅数 1，回复保留主时间线。只覆盖 T05/T08 的基础局部路径；Actor 数仍为 0，模型、附件、流式等完整验收未完成。前述人数阻断已解除。
- 下方日期记录按执行先后保留，包含当时的“正在运行/尚待编译”，这些历史描述不是当前状态。

## 1. 判定规则

- 先区分源码已有、API 可映射、模拟测试、真实 MM 通过及用户亲试，不能混作一种“支持”。
- F01–F34 全部可直接或等价实现，均是完成要求；N01–N03 是平台专属差异，不省略其对应的通用用途。
- 不要求七个平台重新真实登录，只回归被公共补丁触及的现有测试；真实 MM 通过也不能宣称七个平台全部联调通过。
- 测试使用独立 CCCC 数据、测试 Bot 和批准的测试人员。使用单一测试频道验证主链路后，再有意识批准额外 DM/线程，以验证共享 Group 语义，不能误泄露真实组内容。

## 2. 逐项验收

| 用例 | 覆盖 | 操作与通过标准 | 结果 |
|---|---|---|---|
| T01 | U01、U02、U03 | 对照 Slack/Telegram/Discord 逐文件审查：命名、可见性、入口、公共 helper、错误、测试和依赖符合现有风格；公共改动均有必要性说明，无额外服务/框架 | 通过源码审查；见下方公共改动说明 |
| T02 | U04、F01、F27、F33 | Web 平台可选；Token 引用、站点保存/回读、切换草稿、校验、启停和错误正确；CLI 同形；深浅色/窄屏/i18n 沿用现有 UI，用户亲试 | 自动化及真实浏览器通过：原生表单/草稿/校验、三语言、深浅色、窄屏及 UI02；用户体验确认统一保留在 T20 |
| T03 | F01、F24、F25、F34 | 有效启动成功；无效 Token、WS 认证失败、TLS/代理故障明确失败；无凭据泄露或身份降级；常规断线可恢复，辅助失败不假报模型完成 | 通过：真实 HTTPS/WSS/无效 Token，模拟 WS 认证/重连、NET01 HTTP 代理/NO_PROXY/TLS 错误；未破坏真实证书或网络 |
| T04 | F02、F26、F32 | start/stop/status/config/unset/logs 及 enabled 恢复遵循公共行为；停止桥接不停止 Actor；重启不回灌旧 ledger，不复活已弃用 backlog 开关 | 通过：配置/清除/启停/自动恢复有真实证据；LOG03 非空日志与 LOG05 follow 增量、无重复及正常退出通过 |
| T05 | F07、F08、F09、F26 | subscribe/sub→待审批→Web/CLI 批准可用；拒绝、过期、重复处理、撤销和 unsubscribe/unsub 正确；未批准不转发正文和附件 | 通过：真实 Web/CLI 配对管理、600 秒过期及别名记录见上；未授权附件不下载与线程保留由同模块模拟用例补充证明 |
| T06 | F03、F19 | 公共/私有频道、DM、可加入的群组 DM 和线程分别收发；线程必须保持原 root_id，频道目标到主时间线；不能跨目标串授权或把 DM 说成独立 Session | 通过：五类真实目标收发已验证，频道/线程分别授权；明确同组订阅共享上下文与公开外发 |
| T07 | F04、F05、F06 | 群 mention 和 DM 普通文本；默认 foreman、指定 Actor、@all/@peers；正文提及他人不误触发，无关消息/Bot 命令不触发，未知收件人按公共语义报错 | 通过：真实默认/指定 Actor、正文引用、@all/@peers 和未知目标；AUX01 补其他 Bot、无关及重复消息过滤 |
| T08 | F04、F07、F09、F10、F11、F12 | 在 MM 真客户端逐个执行完整文本命令与别名；用 @Bot 前缀避免平台 Slash 拦截，不把 WS 未收到的 /xxx 输入说成成功 | 通过：先前 subscribe/sub、unsubscribe/unsub、send、pause/resume、help/status、verbose on/off，加 CMD02 其余全部参数形式及非法值；保留 @Bot 前缀要求 |
| T09 | F10、F11、F12、F28 | 同 Group 多目标分别批准，验证默认 verbose、on/off、暂停/恢复、状态/帮助；只改变指定订阅，外发符合原生共享规则，不给它们隔离承诺 | 通过：真实多目标独立暂停/恢复、CMD02 全部 verbose 参数、状态/帮助；共享可见性断言见下 |
| T10 | F12、F13、F14、F21、F31 | 公共消息、Actor 间消息、公开/私有系统通知、标题回退、Markdown/链接/代码均正确；不泄露私有事件，不形成 Bot 回流 | 通过：共享通知隐私/定向过滤/标题映射断言，加 CODE01 真实 Markdown/代码/链接与作者回退；不转发终端私有内容 |
| T11 | F15、F16、F28 | 同一 stream 多目标逐渐更新；仅完整终态成功的目标抑制重复正文；无流、过长预览、start/edit/end 失败均保留完整最终兜底 | 通过：真实 MM 主帖/线程更新和去重；失败、超长兜底为模拟协议证据 |
| T12 | F16、F31 | 超长中文、Emoji、换行和代码按实际 MM 限额分段；全文可还原、作者可辨，不截断 Unicode，不因已有预览而漏段 | 通过：先前真实中文/Emoji 分段及模拟预览兜底，加 CODE01 真实长代码/链接 22,158 字符逐字还原 |
| T13 | F17、F18、F29、F30 | 文本+多文件、仅附件、图片/文本/PDF/音频/视频逐项双向传输；落本组 Blob，原目标收文件；不宣称模型已读懂或已转写 | 真实 MM 通过；合成 TXT/PNG/PDF/WAV/WebM，哈希核验 |
| T14 | F17、F18、F25、F27 | 超限/错误长度、无权限文件、路径穿越、跨站携密重定向、上传失败都有受控错误；公共安全约束有效；附件失败不伪装成已成功传给 Actor | 通过：真实超限拒绝，模拟未知长度/归属/重定向/路径校验、AUX01 跨组 Blob 和上传失败、FILE02 下载 403 无残留 |
| T15 | F19、F20、F21 | 重复 posted、请求重试、来自 Bot/系统的帖子，检查稳定来源/client_id 和去重；旧帖编辑不自动追加请求，接入不等待前一模型完成 | 通过：AUX01 编辑/重复/Bot 忽略，共享稳定 client_id 断言及真实源帖关联；只等 daemon 接收，不等模型完成 |
| T16 | F22 | 处理中反应创建、结果/失败关联及过期清理正确；仅修改 Bot 自己的反应；无权限/反应失败不阻塞消息，也不把清理当取消 CLI | 通过：真实处理反应，模拟按 event 关联、只删本 Bot 反应/过期清理，AUX01 403 不吞正文且错误入日志 |
| T17 | F23、F24、F25、F32 | 一次运行期 lag、断线、429、401/403 和进程重启；按公共合同恢复或报错，不承诺未实现的跨重启补发，禁止盲目重发非幂等创建请求 | 通过：共享运行期 lag 补读、真实重启边界，模拟 WS 重连/429/401/403；NET02 完整请求后响应丢失不重复创建 |
| T18 | F28、U03、U06 | 同组多订阅正常；不同组使用不同 Bot；文档清楚警告同组共享上下文，不引入跨组调度、会议业务或新的逐用户身份模型 | 通过：真实同组多目标，GROUP02 两个模拟 Bot 的凭据/授权/输出隔离；文档明确共享边界，无真实第二 Bot 联调声明 |
| T19 | U01、U03、U05 | CCCC 现有构建及相关 Rust/Web/打包回归通过；功能表每项有测试证据或具体平台限制说明；发布不是独立连接器包 | 通过：既有全工作区/前端/发布脚本记录，加日志修复后构建和最新 188 项 IM 回归、UI02 定向回归/构建、逐文件哈希核对；未重复冒称本轮全量回归或执行发布 |
| T20 | U04、U05、U06 | 用户从现有 Web 完成配置，再在 MM 体验点名、DM/线程、文件、流式及命令；报告仅陈述实际通过项 | 通过：2026-09-08 用户明确确认“我已经验收完了，都正常”；refs 引用未回传文件的原始现象与责任边界保留在当前状态，不冒称该次回传成功 |

群组 DM 若目标部署不允许 Bot 加入，应记录明确权限/API 响应及已有代码路径验证结果，不将权限限制写成平台永远不支持，也不要求管理员绕过限制。

### 2026-09-08 验收缺口闭环记录

以下记录各项原有缺口如何闭环，不增加新的验收要求；已验证部分无需无故重复。

- T02：三语言、深浅色、窄屏和配置流程已有证据；UI02 已补通用 IM 说明漏列 Mattermost，三语言真实页面均已验证。
- T03：正常 HTTPS/WSS、无效凭据、断连及 WS 认证已有证据；NET01 已补 HTTP 代理、NO_PROXY 和 TLS 协议错误。没有对真实证书/代理做破坏性故障注入。
- T07/T15：点名路由和默认 foreman 已实测；AUX01 的 `edited_unaddressed_and_bot_posts_are_ignored_and_duplicates_do_not_reply_twice` 已验证普通无关消息、其他 Bot、编辑事件忽略及重复 posted 只回复一次。稳定请求标识见下方共享测试映射。
- T09/T10：目标独立暂停与 verbose 命令已验证；公开/私有通知、Actor 定向过滤及作者回退已对照公共断言（见下）。CODE01 已补实际 Markdown/代码/链接、作者回退及分段还原。
- T12：CODE01 已补包含代码围栏、链接的长文本真实分段与还原，见当前状态。
- T14：已有超限、未知长度、重定向拒绝、Blob 安全与文件名校验；AUX01 已补非本组 Blob 不上传及上传 403 正文保留/失败提示/日志。FILE02 已补无权限元数据及正文下载，无 Blob 残留。
- T16/T17：真实反应及关联、过期清理、429/WS 重连、lag 补读和进程恢复已有证据；AUX01 已补反应 403 的 start/cleanup/finish 日志及正文保留。NET02 已验证服务器完整接收创建请求后断开连接时，只收到一次请求、客户端明确失败，不盲重试。
- T18：GROUP02 已补不同模拟 Bot 身份/凭据及组授权隔离，不是停掉同一真实 Bot 再切组。
- T19：相关 Rust/Web/构建/发布脚本及新增差异回归已核对；源码组织、依赖、工作流与文件哈希均完成复查，具体执行批次分别记录。
- T20：用户已明确确认完整验收结果正常；后续已授权公开 Fork 源码提交及上游 Issue，PR 和正式版本发布另行决定。

### 共享测试的具体覆盖映射（2026-09-08）

这些函数已在 NET01 的 182 项串行回归中执行通过；下列是断言级证据，不是新一轮真实聊天故障注入。

- `im_runtime::tests::actor_targeted_system_notifications_never_escape_to_im`：缺省通知不可外发；`actor_id`/`target_actor_id` 定向通知即使标记 public 也不可外发；明确 public 且 `to=[@all]` 可见。实际 worker 的 `deliver_outbound_event` 在取得订阅和发送前执行此过滤。
- `im_runtime::tests::non_verbose_targets_only_receive_user_facing_events`：Actor 间消息只送 verbose 目标；`@user` 消息送两类目标。注意该用例只验证目标筛选，通知隐私由上一条更早的过滤负责。
- `outbound_message::tests::prefers_trimmed_sender_title` 与 `falls_back_to_actor_id_for_missing_or_blank_title`：优先非空显示标题，缺失或空白回退 Actor ID；Mattermost 出站直接复用 `outbound_text(event, true)`。
- `outbound_message::tests::renders_system_notification_title_and_message` 与 `system_notification_accepts_legacy_text_payload`：通知标题/正文及旧 text 字段映射。
- `state::tests::inbound_metadata_adds_stable_idempotency_and_attachments`：共享入站参数含 source_message_id、thread、附件和稳定 client_id；该测试样本平台为 WeCom，不冒称 MM 专用实测。Mattermost 的调用点传入 post.id、root_id，已有真实收发 ledger 中的 MM 源帖对应关系另作证据。

### 公共改动必要性审查（2026-09-08）

- `im_runtime.rs` 只增加三个同级模块及 Mattermost start 分派；入口、入站、出站与 Slack/Telegram 同形，运行 worker、公开过滤、目标键、命令、分段及 Blob 继续复用现有实现。
- `im_state.rs` 增平台/站点字段、URL 规范化及凭据完备检查；daemon/Web IM 入口只扩大平台白名单；CLI 的参数和命令转换只透传站点字段。没有更改 Actor、Session、消息调度和存储合同。
- `SettingsModal`、`IMBridgeTab`、`imBridgeConfig`、API/类型和三份 locale：新平台、站点字段、原草稿与重置路径、原组件样式；测试覆盖旧平台字段隔离。导航和指南仅新增文档入口。
- `request_origin_tests.rs` 是额外的测试兼容修正：将 Rust 2024 不允许直接使用的进程全局环境修改改为隔离子进程；保持来源校验的生产代码与断言，不启用宽松来源策略。
- Cargo manifest、锁文件、依赖、发布工作流未改变；没有独立 crate、服务、MCP 网关或连接器安装器。测试站点、凭据和合成文件生成脚本留在本地运维目录，不随通用补丁发布。

终端并行失败的原因线索：两个 `terminal_ws` 用例在同一测试进程分别启动 daemon，结束时都调用 shutdown；`server_lifecycle::stop_every_runtime` 经 `actor_runtime::stop_all` 调用进程全局的 `cccc_runtime::stop_all`，没有按当前测试 Home 过滤。因此其中一个用例清理可能终止另一个用例的 PTY。此为源码支持的竞态解释，与单独/串行通过相符；未据此修改上游终端逻辑，也不声称已经通过追踪证明首次失败的全部时序。

## 3. 测试习惯与真实证据

- Rust 测试放在现有同类模块的测试组织中，使用当前异步测试/本地模拟服务；Web 延续现有 config 和 IMBridgeTab 测试方式。
- 用针对性用例验证路由、授权、线程、流终态和网络错误，不增加与小规模使用无关的压测或另一套测试框架。
- 模拟 chat.stream 通过与真实 Actor 是否产生 chat.stream 分开记录。实际运行时名称、版本和模型照实填写，不固定三家品牌或虚构流式覆盖。
- 真实测试日志记录构建提交、用例号、平台版本、预期/实际、脱敏证据和用户体验结论。真实 Token、聊天正文及可访问的私人帖子链接不进入公开仓库。
- 当前部署的权限和服务配置保持不变；测试前另核对测试环境身份，不凭旧记录操作正式服。

真实流式复验使用 `CCCC_MM_TEST_SITE`、`CCCC_MM_TEST_CHANNEL`、`CCCC_MM_TEST_TOKEN_FILE` 三个显式环境变量，凭据文件只读挂载；不得使用正式频道。命令：

```sh
cargo test --locked -p cccc-pair-web --lib \
  im_runtime::mattermost::tests::live_stream_updates_main_and_thread_without_duplicate_final \
  -- --ignored --exact
```

默认忽略此用例是为避免普通 CI 向外部站点发消息，不是忽略失败；真实凭据、站点和私有帖子链接不写入源码。

局部验证入口，编码后按具体测试名缩小范围：

```sh
cargo test -p cccc-pair-core im_state --locked
cargo test -p cccc-pair-web im_runtime --locked
cargo test -p cccc-pair-daemon --locked im -- --test-threads=1
npm -C web run check
npm -C web test
cargo fmt --all --check
```

发布前仍须按当前 CI 完成构建、lint 和相关完整回归；下面的局部证据不替代上表完整用例。

### 2026-09-07：配置接入阶段

- Rust 1.88.0 构建环境安装完成；沿用锁文件依赖，未新增依赖或改动上游发布管线。
- `cargo test -p cccc-pair-core im_state --locked`：8 项通过、0 项失败。包括新增的 Mattermost 站点规范化、必需字段、令牌引用以及旧平台状态回归。
- `cargo fmt --all --check`：通过。
- Web 针对性回归：`imBridgeConfig.test.ts`、`IMBridgeTab.revoke.test.tsx`、`imBridgeRevoke.test.ts`、`services/api/im.test.ts`，4 个文件共 14 项通过、0 项失败。覆盖配置透传、不混入其他平台凭据、先保存后启动、保存失败不启动、缺少站点/Token 禁止保存，以及原撤销授权/微信界面回归。
- `npm -C web run check`：格式、lint 和 TypeScript 检查通过（814 个格式检查文件、781 个 lint 文件，0 警告/错误）。
- `npm -C web run build`：通过；构建提示部分现有 bundle 超过 520 kB 和插件耗时，未为连接器修改全局分包策略。
- 收发 worker、真实 Mattermost 联调及用户亲试尚未完成，不把配置可保存等同于可正常桥接。

### 2026-09-07：收发 worker 实现与首次构建

- 新增 `mattermost.rs`、`mattermost_inbound.rs`、`mattermost_outbound.rs`，公共 runtime 仅增加模块声明与平台启动分支。没有新增 crate、SDK 依赖或独立连接器服务。
- 已写入 Bot 身份与 WS `hello` 校验、公共授权与文本命令、频道/线程/DM、来源标识与去重、同帖流式及最终兜底、Blob 文件双向传输、处理反应、重连及错误状态的代码，**尚待编译和测试证明**。
- 新增模拟协议用例覆盖子路径认证、线程保持、流式按目标确认、失败编辑和长 Unicode 最终兜底、文件上传、身份变化与 Token 轮换、未授权附件和重复配对请求；测试结果未取得前不记为通过。
- 本机缺少完整 Rust 构建所需的 pkg-config/OpenSSL 开发库，sudo 安装要求密码，未更改本机权限。已在授权的测试服务器启动独立、资源受限的 Rust 构建容器，使用上游 Dockerfile 同系列 `rust:1.88-bookworm` 镜像及专用缓存。
- 真实 Mattermost 服务与原有 CCCC/Discord 服务均未因本次构建被停止或覆盖；尚未完成真实测试、用户亲试、GitHub 提交或上游 Issue。

### 首次完整 Rust 编译发现的上游测试问题

- 首次构建退出码 101：原基线 `request_origin_tests.rs` 的 `any_origin_switch_is_opt_in` 直接调用 `std::env::set_var/remove_var`，在 workspace 指定的 Rust 2024 下触发三个 E0133 编译错误。
- 必要的额外公共改动仅在该测试文件：参考已有 `codex_voice/socket_tests.rs`，用 `Command.env/env_remove` 为隔离子进程设置变量并执行同一测试。不加入 unsafe，不禁用原测试，不修改生产跨域或认证策略。
- 修正后须重新运行连接器测试以及 `request_origin::tests`，未取得结果前不标为通过。

### 2026-09-07：全量 Web 回归与 Rust 首轮模拟协议结果

- `npm test -- --maxWorkers=2`：283 个测试文件、1,419 项测试全部通过，0 项失败。测试覆盖整个当前 Web 测试集，不等于真实浏览器验收。
- 独立 Linux 构建执行 `cargo test --locked -j 2 -p cccc-pair-web im_runtime::mattermost`：9 项通过。覆盖 Bot/WS 子路径认证、附件上传、线程保持、流式编辑失败兜底、长 Unicode、未授权附件不下载、配对去重、身份切换和入站过滤。
- 同一构建执行 `request_origin::tests`：8 项通过，包含修正后的隔离子进程环境开关测试；生产来源校验未修改。
- 上述 Rust 结果只对应首轮源码快照。随后补充的附件原帖归属校验、429/重定向和处理反应测试正在复测，不沿用旧结果冒充当前全部通过。
- 尚未完成真实 Mattermost 联调、可用实例交付、用户亲试和发布；T01–T20 的完整判定继续保留待验收。

### 2026-09-07：最新源码回归与程序构建

- 最新源码导出 SHA-256：`17530991fd122538486b4c39e3ba71361e1818d44054b4555b91a61717b31d5a`。该快照包含原帖附件归属校验和新增的限流、重定向及反应用例。
- `cargo test --locked -j 2 -p cccc-pair-web --lib im_runtime`：175 项通过、0 项失败，涵盖现有 IM 模块回归及新增 Mattermost 用例。
- `cargo test --locked -j 2 -p cccc-pair-web --lib request_origin::tests`：8 项通过、0 项失败。
- `cargo build --locked -j 2 -p cccc --bin cccc`：成功。这是开发构建，不等于 release 打包、workspace 全量 lint/测试或真实服务验收。
- 专用测试 Bot 的创建被 Mattermost 管理命令拒绝：`This command cannot be run in local mode`。已停止账号创建流程，未用修改数据库、权限或服务配置的方式绕过；需要正常的管理员登录路径或管理员预先创建测试 Bot。未生成 Bot/用户令牌。

### 2026-09-07：全工作区 lint 与独立 Web 开发实例

- `cargo clippy --workspace --all-targets --locked -j 2 -- -D warnings`：通过，退出码 0。首次导出没有包含根目录 `tests/fixtures`，检查因缺少 DeepSeek fixture 中断；补齐已跟踪测试资料后通过，未为此修改生产源码。
- 独立、非 root、仅宿主机回环端口发布的开发实例已启动，`cccc version` 返回 `0.4.37`，根页面 HTTP 200；保留原生首次管理员 bootstrap 验证。
- 开发二进制 SHA-256：`a5fc9f016fd769834e0b2acc93ee037fc51cbeab37066848a0eea77ef2f7b5e6`。debug 模式需要只读挂载构建时的 Web bundle；这是开发部署，不代替上游 release 打包验证。
- 浏览器命令出现 CDP 超时。agent-browser 安装及空白页启动诊断通过，但不能据此宣称测试页面能操作；真实界面检查与用户亲试仍待完成。
- Linux 全量 Rust 回归已按上游分组启动，运行进程及资源占用已核实，尚未取得最终结果。不得把已经通过的 IM 定向用例或 lint 结果当作全量测试结果。

### 2026-09-07：发布工具与安装脚本回归

- 本地 Python 3.12.3、pytest 9.0.3、Ruff 0.15.14；`python3 -m ruff check scripts tests` 通过，`python3 -m pytest -q` 112 项全部通过。当前上游 CI 选择 Python 3.14，本结果不冒充该版本上的执行。
- `bash scripts/tests/release_assets.sh`：退出码 0，结尾 `OK: release assets`；测试中的额外归档拒绝是预期负例。
- `bash scripts/tests/install_unix.sh`：退出码 0，结尾 `OK: Unix installer`。使用脚本自身生成的临时 fixture，覆盖校验、旧版本安装、所有权保护与失败回退等路径，不改变已有真实 CCCC 安装。
- 安装器模拟回退通过不等于本次测试服务器的部署回退及专用错误频道上报已经验收；后两项仍须实际部署验证。

### 2026-09-07：用户纠正部署边界，撤销误部署

- CCCC 应运行在独立应用服务器，连接 Mattermost 测试域名；不应把 CCCC 程序或构建任务部署到 Mattermost/Dify 测试服务器。
- 已先备份本次新增 CCCC 目录和容器日志，再删除专用容器、卷、镜像、缓存及目录，关闭对应本地 SSH 隧道；未重置数据库、从生产克隆或重新部署现有业务服务。
- 原有 11 个业务容器的 ID、镜像、启动时间及重启计数在撤销前后相同；Mattermost 内网及测试域名健康检查正常，Dify 网关健康检查正常。
- 此前 HTTP 200、编译与单测结果仅作为历史证据。误部署实例已不再提供访问，页面亲试未通过；两轮 Linux 全量回归均退出 101，不存在仍在聊天服务器运行的回归任务。后续在正确的 CCCC 服务器继续排查和验收。

### 2026-09-07：正确应用服务器的独立界面验证

- 在应用服务器部署独立非 root 容器，使用新数据、主目录和工作目录，未覆盖原 CCCC/CAO 或复用 CLI 登录。使用上述已验证开发程序及只读 Web bundle，尚非 release 构建。
- 原生浏览器实际完成建组、中文切换、进入 IM 桥接、选择 Mattermost、填写站点、保存配置。CLI 回读平台、站点和环境变量引用一致；待申请、已授权聊天、启动及删除配置入口正常显示。
- 本阶段仅保存未配置真实值的 Token 环境变量名，未启动桥接、未调用模型或连接真实 Bot。T02 取得局部证据，不标为完整通过。
- 已提供新界面入口并通知用户亲试；完整 Rust 非 root 复验及真实 MM 联调仍待继续。

### 2026-09-08：真实频道配对与状态命令

- 使用已登录测试服管理员正常操作；管理员明确批准人数配置调整后才入队，未提升 Bot 权限、绕过配额或关闭 MFA。
- 新私有频道仅加入管理员、测试用户本人及 Bot，绑定空白独立 Group，不转发原有业务工作组内容。
- Mattermost 原生输入框发送 `@cccc /subscribe`，由真实 WS 入站产生 pending，Bot 回复配对说明；核对频道与 Group 后调用原生 bind 接口批准。
- 再经原生输入框发送 `@cccc /status`，实际收到状态回复；authorized=true、paused=false、verbose=false、root_id 为空。CCCC 状态运行中、错误为空、订阅数 1。
- 组内 Actor 数 0，模型推理、模型回复路由、文件和流式尚未验收。已交付可亲试的频道入口，未把协议命令回复冒充模型回复。

### 2026-09-08：独立 Codex Actor 真实问答

- 用户明确授权只复制 Codex 登录凭据；未复制原配置或历史，不更换为付费 API。CLI 登录状态和 CCCC 原生会话均可用。
- 在真实 Mattermost 输入框发送 `/send @codex` 提问，CCCC ledger 记录对应入站事件、runtime 接收和 Actor 发出的 chat.message，频道收到模型答案。
- 第二条以普通 mention 提问，不含显式收件人或要求使用消息工具的额外提示；入站默认交给 foreman，真实模型承接上一条结果并回复。
- 两条模型回复都在频道主时间线显示，均无 root_id；桥接无错误、订阅 1、Actor 会话 usable。未以后台脚本直接写入模型答案代替验收。
- 本轮只证明单 Codex、指定收件人/默认 foreman 和连续问答；多 Actor 广播、未知收件人、附件、流式及故障恢复等用例仍待完整验证。已提供用户亲试入口，不提前标记整体完成。
