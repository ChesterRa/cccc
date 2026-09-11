# CLI 管理规格

本文是当前有效合同，旧的阶段实现与测试证据单独保存在 [历史修订记录](cli-management-history.md)，历史内容不得覆盖本页。使用方法见 [CLI 管理指南](../guide/cli-management.md)，真实页面验收范围见 [Web 验收](cli-management-web-acceptance.md)。

## 范围、术语与原生边界

- 本功能最初基于上游 `0c1d10a1`，2026-09-11 为 PR 同步至 `22733e9a`（v0.4.39）；保留已公开功能提交，通过合并上游更新，不改写其历史。后台入口沿用原生 `resolve_operation/Operation/Policy`，只注册本功能的读取和全局写入操作，不恢复已被上游删除的 `dispatch_concurrency/operation_access.rs`。其余调度与锁策略保持上游行为，包括新上游的 WebSocket 来源检查。各基线验证与已部署实例的历史证据分别记录在 Web 验收文档，不互相替代。
- 全局设置提供 Agent CLI 安装、更新、受管卸载、CLI 自动更新计划和 CLI 操作记录。Runtime、Actor、模型、供应商登录与软件安装不是同一概念。
- 受管安装是 CCCC 负责生命周期的软件副本；外部安装仅通过原生探测使用，不更新、卸载或接管。统一用“受管”，不把它作为第二套来源选择配置。
- 原生 Runtime 清单为 19 项；CLI 管理页显示 17 个有已知安装来源的 CLI，排除 `web_model` 与 `custom`，不从通用目录或 Actor 配置删除它们。
- 前端复用原生设置组件、表单/保存/冲突模式、5 秒轮询及中英日 locale。新增详细指南使用中文；英文上游指南仅补英文导航，明确链接目标为中文，不插入中文正文。
- HTTP 是薄转发，Daemon 复用原生控制身份和错误合同，核心状态沿用文件锁及原子 JSON 写入。无新增调度框架、数据库、外部错误频道或通知 Webhook。
- 安装来源穷尽匹配当前 Runtime；新增 Runtime 时需重新核对，不自动猜测任意命令、脚本或包名。

## 安装来源与证据

| Runtime | 原生命令 | 安装来源/处理路径 | 当前验证状态 |
|---|---|---|---|
| `amp` | `amp` | mise `amp`（npm `@ampcode/cli`） | Linux x64 0.0.1788881120-ge1f3b0 实际安装及同版本更新通过；仅本 CLI 版本查询包含官方 Git 后缀发行 |
| `antigravity` | `agy` | mise `antigravity-cli` | Linux x64 1.1.27 实际安装及同版本更新通过 |
| `auggie` | `auggie` | mise npm 后端 `@augmentcode/auggie` | Linux x64 0.36.0 实际安装及同版本更新通过 |
| `claude` | `claude` | mise `claude` | Linux x64 2.1.263 实际安装及同版本更新通过 |
| `cline` | `cline` | mise npm 后端 `cline`，保留 optional dependencies | Linux x64 3.0.61 实际安装及同版本更新通过 |
| `codex` | `codex` | mise `codex` | Linux x64 0.152.0 实际安装及同版本更新通过 |
| `copilot` | `copilot` | mise `copilot` | Linux x64 1.0.83 实际安装及同版本更新通过；修正版本句末标点误判，不放宽版本前后缀校验 |
| `cursor` | `cursor-agent` | mise `cursor-agent` | Linux x64 2026.09.08-6caf4ff 实际安装及同版本更新通过 |
| `deepseek` | `dsh-acp-demo` | mise 隔离 Node，复用 CCCC 已有固定版本组件安装和 ACP 配置 | Linux x64 0.1.0-rc.6 实际安装、完整就绪检查及同版本更新通过；真实 Actor/模型会话待测，不用 latest 绕过兼容合同 |
| `devin` | `devin` | 官方版本清单及分平台归档，清单含 SHA-256，mise HTTP 下载/解压 | Linux x64 3000.6.14 实际安装、版本校验及同版本更新通过；其他平台、跨版本更新和 Actor 会话待测 |
| `kiro` | `kiro-cli` | 官方 stable 清单与固定版本 Linux 归档，mise HTTP 下载/解压 | Linux x64 2.21.2 实际安装、版本校验及同版本更新通过；macOS DMG 适配、其他架构及 Actor 会话待测 |
| `kilo` | `kilo` | npm `@kilocode/cli`，CCCC 已有原生启动适配 | Linux x64 7.5.16 实际安装及同版本更新通过 |
| `droid` | `droid` | 官方分平台发行二进制与 `.sha256`；x64 区分 AVX2/baseline | Linux x64 0.215.1 实际安装、版本校验及同版本更新通过；首次实测的单文件 `bin_path` 错误已修正并复测；其他平台与 Actor 会话待测 |
| `grok` | `grok` | mise `grok` | Linux x64 1.0.24 实际安装及同版本更新通过 |
| `hermes` | `hermes` | NousResearch/hermes-agent，官方源码与独立 Python 环境 | Linux x64 v2026.9.7 官方 editable 安装、TUI 构建及同版本更新通过；无普通 wheel、Nix 保护绕过或未锁定依赖回退 |
| `kimi` | `kimi` | mise npm 后端 `@moonshot-ai/kimi-code`，Node >=22.19 | Linux x64 0.41.0 实际安装及同版本更新通过；不是旧 Python kimi-cli |
| `opencode` | `opencode` | mise `opencode` | Linux x64 1.18.29 实际安装及同版本更新通过 |
| `web_model` | 无本地 CLI | 不适用 CLI 软件管理 | 保留原有 Runtime 功能 |
| `custom` | 用户自定义 | 无可安全推导的通用安装包 | 保留原有 Runtime 功能，明确原因 |

以上 17 个 CLI 均已通过 Linux x86_64 的真实安装、选中版本持久化和同版本更新检查。它们不代表所有平台、跨版本更新、账户登录、真实模型调用或 Actor 协议集成已经验收；后续仍须分别验证。

依据：[mise 注册表](https://mise.jdx.dev/registry)、[mise 安装与激活语义](https://mise.jdx.dev/cli/install.html)、[Auggie 官方安装](https://docs.augmentcode.com/cli/overview)、[Cline 官方安装](https://docs.cline.bot/getting-started/installing-cline)、[CCCC Runtime 指南](../guide/runtimes.md)。上线前须核对实际部署 mise 所含注册表，而非假设线上文档等于安装版本。

补充依据：[Devin 官方安装](https://cli.devin.ai/reference/commands)、[Devin 安装脚本](https://cli.devin.ai/install.sh)、[Kiro 安装脚本](https://cli.kiro.dev/install)、[Droid 安装脚本](https://app.factory.ai/cli)、[Hermes 官方安装](https://hermes-agent.nousresearch.com/docs/getting-started/installation)、[Kimi Code 官方安装](https://www.kimi.com/code/docs/en/kimi-code-cli/guides/getting-started)。脚本仅作为发行格式的调查依据，不是批准将其原样执行。


## 默认启动与持久化

- `CCCC_HOME/cli-management/state.json` 保存 `revision`、`rules`、`installations`、`jobs`；持有 `state.lock` 完成读改写并原子保存。不保存供应商凭据。
- 默认启动使用有效受管记录；无记录才沿用原生探测。Actor 显式命令不覆盖，环境和命令选择只发生在启动边界，不改保存的 Actor、原生会话或已运行进程。
- 受管文件/依赖损坏、管理状态读取失败均明确拒绝受影响的默认启动，不静默回落外部版本。普通 Actor、DeepSeek 及内建 Voice Analyst 接入同一选择规则。
- 安装/更新先解析具体版本，在每个操作独立目录安装并验证，成功才写入选择。健康的同版本可复用；损坏则隔离修复，失败保留旧选择。
- `Operation::Update`、HTTP 新请求与响应统一为 `update`。旧 `upgrade` 只作 Serde 读取别名；旧记录重试保留原编号，不重复入队。新格式回退到旧程序不是双向兼容，不能盲目降级。
- `/api/v1/runtimes` 读取管理状态失败仍返回 HTTP 200，完整保留原生探测的目录字段、可用性、路径和 `available` 列表；另以 `cli_management_error="cli_management_state_error"` 和 CLI 项的同名 `managed_error` 错误码报告管理状态故障。该目录不是启动授权，不得把读取失败扩散为既有 CLI 不可用。状态健康时顶层错误为 null；不会重置坏状态。
- 单项受管依赖损坏只使该项不可用，其余照常返回。目录展示降级不改变启动时的严格检查。

### DeepSeek 原有路径与输出兼容性

- `ensure_with` 到 `ensure_at_with` 仅提取安装目录参数，旧入口仍使用原有固定版本目录；原有首次安装、重复执行、失败重试、代理设置和配置迁移的测试保留。无受管记录时，公共 `ensure` 与旧入口的返回值、环境修改、错误及资料副作用必须相同。
- 仅在 CLI 管理创建了受管记录后选择隔离目录。受管文件损坏时明确失败，不修改受管软件，也不回落或修补原有固定版本目录；这是新增受管状态的合同，不是对旧用户安装的迁移。
- `DeepSeekSetupOutcome.packages_installed/profile_created` 表示**本次调用是否安装软件包/创建配置**，不是软件当前是否存在。受管版本已由 CLI 安装操作准备完成，因此只读 `ensure` 返回 false/false；原有已就绪重复调用同样如此。
- 既有消费者 `crates/cccc-cli/src/commands/setup.rs::setup_deepseek` 的 JSON 字段、类型和上述含义不变；`dsh_home/profile` 指向本次验证的安装。这不是“未安装”的状态。未调查外部脚本，不宣称没有外部消费者，也不更改原有 JSON 消费代码。

## 操作、卸载与故障恢复

- 安装、更新和卸载均提交后台操作；相同请求编号和参数返回已有记录，不同参数复用编号拒绝。同一 Runtime 的活动操作不能重叠，实例内逐项执行。
- 操作状态为 `queued/running/succeeded/failed/interrupted`，记录 Runtime、操作、手动或计划来源、创建/开始/结束时间及错误。入队或关闭网页不等于完成。
- 卸载两个 HTTP 入口进入同一实现，不是占位。仅删除可由该 Runtime 安装操作记录证明归属的 `versions/<操作编号>` 目录，包括旧版本；拒绝越界、目录链接、归属不明及被其他 Runtime 引用的目录。
- 外部软件、供应商登录、原生会话、Group、Actor、计划和日志全部保留。全部删除成功后才原子移除受管记录；后续默认启动恢复原生探测，自动计划跳过该 CLI。显式旧路径不自动修正。
- 使用原生 fs2 文件锁：CCCC 启动者持共享锁，卸载尝试独占锁；活动 Actor/内建助手须先停止，不自动杀掉它们。配置改变不解除已启动进程的归属。其他终端直接运行的程序不在这一使用保护范围。
- 使用登记锁中毒时，启动保留已取得的共享句柄；破坏性卸载保守拒绝，不清空登记或中毒标志。
- 卸载失败/中断可能已删除部分文件，保留受管记录并报错，不承诺文件级回滚；可重试卸载或更新修复，不允许静默回落。安装/更新失败不切换旧选择。
- 重启恢复把未完成的 Running 标记 Interrupted，不自动重放；存储故障恢复后重新核对，不永久卡住旧 Running。
- Worker 按原生后台服务组织为异步循环、逐次 `spawn_blocking` 和异步等待。关闭先发 stop，最多等待 6 秒，超时中止异步调度任务并记录明确警告，不再无上限 await。
- 执行中的安装命令每 25ms 检查 stop，并通过 OwnedProcessTree 终止所属进程树；退出回收最多等待 5 秒。已开始的阻塞调用无法被 Tokio abort 强制停止；6 秒仅是 Worker.finish 等待上限，不是磁盘/内核卡死时整个进程退出保证。警告不表示阻塞收尾已完成，不能据此立即复用同一 Home 启动第二个 Worker。
- 不提供手动版本回切、定期旧版本清理或任意命令安装接口，不修改原有 CLI 的登录与计费方式。

## CLI 自动更新计划

- 规则属于实例，仅为已安装的受管 CLI 创建 Update；未安装和外部软件不自动接管。多条规则，初始关闭，周期默认 03:00。
- 复用 `interval`（至少 60 秒）、`cron`（五段）和 `at` 合同。界面每天/每周单日/每月，多个星期使用多条规则；复杂表达式保留原值，不能无声变成每天。
- 保存固定时区，下次时间按浏览器本地时间显示；月末不存在的日期不执行，一次性不重复。计划版本冲突拒绝覆盖并保留草稿。
- 星期转换仅在 CLI 管理私有入口复制 trigger 后进行：Web 0/7 周日、1 周一转换为 cron 库编号并补秒。保存校验、首次计算和到期续排程均走此入口；API/持久化表达式不变。
- 公共 `automation_schedule.rs` 与上游基线完全相同，工作组自动化仍保持旧解释；不在本功能修复原有 Web/工作组后端星期差异，不新增公共开关。
- 到期领取和操作入队原子保存；停机错过周期至多合并一次，不重放全部历史。未变规则保留下次时间，重启不重复领取；恢复/保存不要求迁移既有 CLI 计划。

## HTTP、Daemon 与返回数据

所有成功响应沿用 `{ok:true,result:...}`。

| HTTP 入口 | Daemon 操作 | 输入与结果 |
|---|---|---|
| GET /api/v1/cli-management | cli_management_get | 返回 `{runtimes,state}` |
| POST /api/v1/cli-management/jobs | cli_management_submit | `runtime,operation,request_id`；返回 `{job}`，三种操作全部支持 |
| POST /api/v1/cli-management/uninstall | cli_management_uninstall | `runtime,request_id`；等价于通用入口的 `operation=uninstall`，返回 `{job}` |
| PUT /api/v1/cli-management/schedules | cli_management_schedules_update | `revision,rules`；返回 `{state}` |
| GET /api/v1/cli-management/jobs/{job_id}/log?offset=0 | cli_management_log | `job_id,offset`；返回日志页 |

`runtimes[]` 合同：

| 字段 | 类型与含义 |
|---|---|
| name / display_name / command | Runtime 标识、显示名、原生命令 |
| external_available / external_path | boolean / string 或 null；原生外部探测结果，不代表默认来源 |
| source | 已知安装来源：`{kind:mise,tool,node}`、`{kind:official,distribution}`、`{kind:deepseek}` 或 `{kind:not_applicable,reason}`；不是默认使用来源配置 |
| installation | 受管安装对象或 null |
| managed_error | string 或 null；受管文件与依赖验证错误 |
| uninstall_available / uninstall_reason | 有受管记录时 true/null，否则 false/`cli_not_managed`；true 不绕过运行中和目录归属检查 |

`state` 合同：

| 字段 | 类型与含义 |
|---|---|
| revision | 非负整数，计划配置版本 |
| rules | `{id,enabled,trigger,next_run_at}` 数组；下次时间为 RFC3339 或 null |
| installations | 按 Runtime 索引的 `{version,executable,bin_paths,installed_at}` |
| jobs | 按操作 ID 索引的 `{id,runtime,operation,status,created_at,started_at,finished_at,source_rule,error}`；未开始/结束时间、手动来源和无错误为 null |

日志页为 `{entries,next_offset,has_more}`；entries 至少包含 `ts,stream,text`，旧分片记录可能带 `continuation`。offset 为非负字节游标，不接受任意路径。

## 权限、错误与日志

- 全部 CLI 管理 HTTP 入口属于原生全局管理员权限，保留认证/CSRF。Daemon 要求 `by=user` 并校验字段白名单；本机可信 IPC 不是同 OS 用户间的安全隔离。
- 未知 Runtime、任意命令/路径/包名、不适用 Runtime 拒绝。操作 ID 只接受最多 64 字符的字母/数字/下划线/连字符。
- 计划过期版本返回 `cli_schedule_revision_conflict`，并发操作返回 `cli_operation_busy`，参数错误 `invalid_args`；未知日志 `cli_job_not_found`；读写故障 `cli_management_state_error`；后台故障 `cli_worker_unavailable`。卸载执行失败通过操作 Failed 和日志反馈，不返回“未实现”。
- 传输结果不确定时保留草稿和请求编号，刷新核实再重试；错误不被伪装为成功。CLI 管理页读取故障仍明确报错，只有既有通用 Runtime 列表做上述受控降级。
- 原始 stdout/stderr 直接写每条命令的文件，不按行积存在内存，不设额外单行大小阈值；Unix 文件 0600、目录 0700，完整原始输出不宣称脱敏。
- 阶段/退出记录使用脱敏 JSONL。查询子进程输出沿用原生 tail：64 KiB 分块、最多 8 MiB/200 行，返回前脱敏；版本/路径查询单独限制 1 MiB，不是日志写入上限。
- 日志创建失败不启动命令；同步、解析、进程收尾失败不能标记成功。不保证发现子进程自行忽略的每次文件 write 错误，不向外部 Webhook 发送通知；旧通知文件不读取也不擅自删除。

## 当前验收门禁

| 范围 | 必须验证 |
|---|---|
| 来源与安装 | 17 项来源清单完整；隔离安装/更新、同版本修复及失败保留 |
| 启动与卸载 | 受管优先、无记录回落、损坏拒绝；实际删除仅限受管目录，外部软件及登录/会话不变；活动使用拒绝 |
| 调度 | 全星期/复杂表达式/日/月末/时区、保存重载、续排程、重复领取；工作组旧行为不变 |
| Worker | 正常关闭、活动进程树回收、独立进程不受影响、6 秒超时分支、存储故障恢复 |
| HTTP | 权限、幂等、冲突、两个卸载入口、日志、坏状态下目录降级及修复后恢复 |
| Web | 原生风格与三语，17 项清单、中文术语、操作记录、日志、计划交互及故障恢复 |
| 发布 | 文档治理与 CLI 功能分支分开；仅获准的开发实例部署，标准版不动；推送需独立授权和完整历史秘密扫描 |

2026-09-10 前次审阅修订（旧基线部署历史）：核心库 205、Daemon 24、Actor 3、HTTP/Runtime 5、前端 18，共 255 项自动化测试通过，1 项真实供应商测试按约定忽略；当时完成构建、格式、术语、分支拆分及开发部署/浏览器影响面复核，尚未提交或推送。该记录不覆盖后续 Runtime 故障隔离修正和上游基线整理；各轮边界见 [验收记录](cli-management-web-acceptance.md)。此前 56 项同样仅为历史证据。

供应商 Linux x64 安装证据按上表，不冒称本轮重新下载所有软件或模型调用。其他平台、完整 OS 故障、外部终端使用保护不在已测范围。

术语检查：本次统一受管安装、CLI 更新、CLI 自动更新计划、CLI 操作记录；没有改变 Actor/Runtime/原生会话定义。词汇表的规划标题保留制定时含义，不充当当前完成状态。
