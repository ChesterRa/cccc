# CCCC 与 Agent 共同进化 PRD

## 1. 背景

CCCC 当前已经具备多 Agent 协作的核心控制面：

- `coordination`：共享目标、决策、交接。
- `tasks`：可追踪任务与生命周期。
- `agent_states`：单个 actor 的短期工作状态。
- `memory`：长期经验沉淀。
- `capability`：能力搜索、启用、导入、禁用与自生成 skill 管理。
- `agent_self_proposed` capsule skill：允许 Agent 提交窄作用域、可验证的流程性能力。

这些能力已经让 CCCC 能“组织 Agent 工作”。下一步不是做一个泛化的自动学习系统，而是让 CCCC 在真实协作中形成一个可控的进化闭环：

`任务执行 -> 证据沉淀 -> 候选改进 -> 试运行 -> 验证 -> 晋升/回滚`

本 PRD 目标是把这个闭环产品化，让 CCCC 和 Agent 能在不牺牲安全、可审计、可回滚的前提下共同进化。

## 2. 一句话目标

让 Agent 从每次任务中提炼可复用经验，并让 CCCC 以受控、可追踪、可验证的方式把这些经验转化为 skill、checklist、routing rule 或 memory，从而逐步提升后续协作质量。

## 3. 设计原则

- 简单优先：MVP 只做一条最短闭环，不引入复杂模型训练、自动改代码、自动全局发布。
- 当前所需：只沉淀已经在任务中被证据支持的经验，不为假想场景预留复杂规则引擎。
- 单一职责：Agent 负责发现和提出改进；CCCC 负责记录、验证、治理和分发。
- 可审计：每个进化项必须能追溯来源任务、提出者、证据、试运行结果和当前状态。
- 窄作用域默认：新能力默认只在当前 group 或 actor 试用，不自动全局启用。
- 可回滚：任何晋升后的能力都必须能停用、降级或替换。

## 4. 用户与角色

### 4.1 普通用户

希望 CCCC 越用越顺手，但不希望系统偷偷改变行为、污染记忆或自动安装不可信能力。

### 4.2 Foreman

希望在收尾时看到 Agent 的复盘候选，并能把高价值经验转成可复用流程。

### 4.3 Peer Agent

希望在遇到重复问题时能提交一个结构化改进，而不是每次靠自然语言提醒。

### 4.4 系统管理员

希望跨 group 看到自进化能力库，能治理来源、状态、风险和启用范围。

## 5. 核心问题

当前已有 `agent_self_proposed` skill 能力，但仍缺少完整产品闭环：

- Agent 什么时候应该提出进化项不够明确。
- 提案和任务证据之间没有一等关联。
- 缺少候选项生命周期：draft、trial、qualified、blocked、retired。
- 缺少“试运行结果”与“晋升条件”。
- UI 已有 Self-Evolving Skills 入口，但更像能力管理，不是完整进化工作台。
- memory、task、coordination、capability 之间没有统一的进化事件模型。

## 6. 目标

### 6.1 MVP 目标

实现“自进化候选项”的端到端闭环：

1. Agent 可在任务完成或关键转折时提交 `evolution.proposal`。
2. CCCC 将提案记录为可查询、可审计的候选项。
3. 候选项可以一键 dry-run 验证为 `agent_self_proposed` capsule skill。
4. 候选项可以在 actor 或 group 作用域进入 trial。
5. trial 使用后可记录 outcome。
6. 满足条件后可晋升为 qualified skill，或被 blocked / retired。
7. Web UI 能展示候选项、证据、状态、启用范围和操作。

### 6.2 非目标

MVP 不做：

- 不做自动修改 CCCC 源码。
- 不做自动全局启用。
- 不做模型权重训练或外部训练数据集。
- 不做复杂评分模型。
- 不做跨用户共享市场。
- 不做任意外部网页内容自动转 skill。
- 不做自动从所有聊天记录里无差别挖掘知识。

## 7. 术语

- Evolution Proposal：Agent 提出的结构化改进候选。
- Evolution Candidate：被 CCCC 持久化并进入生命周期管理的候选项。
- Evidence：支撑候选项的任务、消息、测试、错误、决策或结果链接。
- Trial：候选项在窄作用域启用后的试运行状态。
- Promotion：候选项通过验证后晋升为 qualified 能力。
- Capsule Skill：以 `skill:agent_self_proposed:<stable-slug>` 命名的轻量流程 skill。

## 8. 产品闭环

### 8.1 提案生成

Agent 在以下时机可以提交提案：

- root task 完成前的收尾检查。
- 连续遇到同类问题。
- 修复了一个可复用的流程缺陷。
- 发现某类任务需要固定 checklist。
- 某个验证步骤显著降低返工。

提案必须包含：

- 问题：这次任务暴露了什么重复性问题。
- 适用场景：以后什么时候使用。
- 不适用场景：什么时候不要使用。
- 操作步骤：可执行流程。
- 风险：误用或过度泛化风险。
- 验证：如何判断它有效。
- 证据：至少一个任务、消息或测试引用。

### 8.2 候选持久化

CCCC 将提案保存为 `EvolutionCandidate`，初始状态为 `draft`。

候选项不是立即生效能力。它只是一个可审计记录。

### 8.3 Dry-run 验证

用户、foreman 或有权限 actor 可对候选项执行 dry-run：

- 将候选项转换为 `capability_import(dry_run=true)` 输入。
- 校验 capsule 模板完整性。
- 校验 capability id 命名空间。
- 校验来源为 `agent_self_proposed`。
- 返回 readiness preview 和诊断。

通过 dry-run 后状态可变为 `ready_for_trial`。

### 8.4 试运行

试运行只允许窄作用域：

- 默认 actor scope。
- 可选 group scope。
- 禁止 MVP 自动 global scope。

试运行通过 `capability_import(enable_after_import=true, scope=actor|group)` 完成。

### 8.5 试运行结果记录

每次使用后可记录 outcome：

- `helped`：确实减少返工或提升质量。
- `neutral`：没有明显效果。
- `harmful`：造成误导、噪声或失败。

记录包含：

- 使用任务。
- 使用 actor。
- 结果摘要。
- 是否需要修改 capsule。

### 8.6 晋升与回滚

晋升条件建议：

- 至少 2 次 helped outcome。
- 无 harmful outcome，或 harmful 已通过新版本修正。
- capsule 文本满足模板要求。
- 来源证据完整。

晋升后：

- `qualification_status=qualified`。
- 可在 Self-Evolving Skills Library 中作为推荐能力出现。
- 仍不自动全局启用。

回滚：

- `blocked`：风险过高或内容错误。
- `retired`：被更好的候选替代或不再适用。
- `revised`：复用同一个 capability_id 更新 capsule，不创建近似重复项。

## 9. 状态机

```text
draft
  -> ready_for_trial
  -> trialing
  -> qualified
  -> retired

draft
  -> blocked

ready_for_trial
  -> blocked

trialing
  -> revised
  -> qualified
  -> blocked
  -> retired

revised
  -> ready_for_trial
```

状态说明：

- `draft`：已记录，未验证。
- `ready_for_trial`：dry-run 通过，可试运行。
- `trialing`：已在 actor 或 group 窄作用域启用。
- `qualified`：通过验证，可作为稳定经验复用。
- `blocked`：不允许启用。
- `retired`：保留审计记录，但不再推荐使用。
- `revised`：同一候选已更新，等待重新验证。

## 10. 数据模型

### 10.1 EvolutionCandidate

```ts
type EvolutionCandidate = {
  id: string
  group_id: string
  origin_actor_id: string
  capability_id: string
  title: string
  problem: string
  capsule_text: string
  status:
    | "draft"
    | "ready_for_trial"
    | "trialing"
    | "qualified"
    | "blocked"
    | "retired"
    | "revised"
  source_task_id?: string | null
  evidence_refs: EvolutionEvidenceRef[]
  risk_level: "low" | "medium" | "high"
  qualification_reasons: string[]
  trial_scope?: "actor" | "group" | null
  trial_actor_id?: string | null
  imported_record_version?: string | null
  supersedes_candidate_id?: string | null
  created_at: string
  updated_at: string
  updated_by: string
}
```

### 10.2 EvolutionEvidenceRef

```ts
type EvolutionEvidenceRef = {
  kind: "task" | "message" | "decision" | "handoff" | "memory" | "test" | "file"
  ref_id: string
  summary: string
}
```

### 10.3 EvolutionOutcome

```ts
type EvolutionOutcome = {
  id: string
  candidate_id: string
  group_id: string
  actor_id: string
  task_id?: string | null
  result: "helped" | "neutral" | "harmful"
  summary: string
  created_at: string
  created_by: string
}
```

## 11. 后端需求

### 11.1 Kernel 存储

新增轻量存储模块：

- `src/cccc/kernel/evolution.py`

职责：

- CRUD `EvolutionCandidate`。
- CRUD `EvolutionOutcome`。
- 状态转移校验。
- 生成 capability import record。
- 汇总 candidate statistics。

持久化位置建议：

```text
CCCC_HOME/groups/<group_id>/state/evolution/candidates.json
CCCC_HOME/groups/<group_id>/state/evolution/outcomes.json
```

原因：

- 与 group 绑定，符合窄作用域默认。
- 不污染全局 capability catalog。
- 易于备份和迁移。

### 11.2 Daemon IPC

新增 op 组：`evolution_*`。

#### `evolution_proposal_create`

```ts
{
  group_id: string
  by: string
  title: string
  problem: string
  capsule_text: string
  source_task_id?: string
  evidence_refs: EvolutionEvidenceRef[]
  risk_level?: "low" | "medium" | "high"
}
```

返回：

```ts
{ candidate: EvolutionCandidate }
```

规则：

- `capsule_text` 必须包含 `When to use`、`Avoid when`、`Procedure`、`Pitfalls`、`Verification`。
- `capability_id` 由服务端基于 title 生成，格式为 `skill:agent_self_proposed:<stable-slug>`。
- `evidence_refs` 至少一条。
- `risk_level=high` 时只能进入 `draft`，不能直接 trial。

#### `evolution_candidate_list`

```ts
{
  group_id: string
  status?: string
  include_outcomes?: boolean
}
```

#### `evolution_candidate_dry_run`

```ts
{
  group_id: string
  by: string
  candidate_id: string
}
```

行为：

- 调用现有 capability import dry-run 逻辑。
- 不持久化 capability catalog。
- 成功后将 candidate 状态更新为 `ready_for_trial`。

#### `evolution_candidate_start_trial`

```ts
{
  group_id: string
  by: string
  candidate_id: string
  scope: "actor" | "group"
  actor_id?: string
}
```

行为：

- 调用 `capability_import(enable_after_import=true)`。
- `scope=actor` 时必须提供 `actor_id`。
- `risk_level=high` 必须拒绝。
- 成功后状态变为 `trialing`。

#### `evolution_outcome_add`

```ts
{
  group_id: string
  by: string
  candidate_id: string
  result: "helped" | "neutral" | "harmful"
  summary: string
  task_id?: string
}
```

#### `evolution_candidate_promote`

```ts
{
  group_id: string
  by: string
  candidate_id: string
}
```

规则：

- 至少 2 条 `helped` outcome。
- 没有未处理的 `harmful` outcome。
- 最近一次 capability import record 有效。
- 状态变为 `qualified`。

#### `evolution_candidate_block`

```ts
{
  group_id: string
  by: string
  candidate_id: string
  reason: string
}
```

#### `evolution_candidate_retire`

```ts
{
  group_id: string
  by: string
  candidate_id: string
  reason: string
}
```

### 11.3 MCP 工具

新增 MCP 能力组：

- `cccc_evolution`

Action 风格：

```ts
{
  action:
    | "proposal_create"
    | "candidate_list"
    | "candidate_dry_run"
    | "candidate_start_trial"
    | "outcome_add"
    | "candidate_promote"
    | "candidate_block"
    | "candidate_retire"
}
```

Agent 使用原则：

- Peer 可创建 proposal。
- Peer 可为自己使用过的 trial 添加 outcome。
- Foreman / user 可 dry-run、start trial、promote、block、retire。
- Peer 不可将能力启用到其他 actor。

### 11.4 与现有 capability 的关系

不要重复实现 skill 导入能力。Evolution 模块只做治理层：

- candidate -> import record 的转换。
- 状态与证据管理。
- 调用现有 `capability_import`。
- 读取现有 `capability_state.active_capsule_skills` 做验证。

## 12. 前端需求

### 12.1 Self-Evolving Skills 工作台

扩展当前 Self-Evolving Skills UI，拆成两个视图：

- Candidates：候选项生命周期管理。
- Library：已导入/已晋升 skill 管理。

Candidates 列表字段：

- 标题。
- 状态。
- 风险等级。
- 来源 actor。
- 来源 task。
- outcome 计数：helped / neutral / harmful。
- 当前 trial scope。
- 更新时间。

详情抽屉：

- Problem。
- Capsule text。
- Evidence refs。
- Dry-run diagnostics。
- Outcomes。
- 操作按钮。

### 12.2 操作按钮

按状态显示：

- `draft`：Dry Run、Block、Retire。
- `ready_for_trial`：Start Actor Trial、Start Group Trial、Block、Retire。
- `trialing`：Add Outcome、Promote、Revise、Block、Retire。
- `qualified`：Retire。
- `blocked`：只读。
- `retired`：只读。

### 12.3 UX 约束

- 不做弹窗堆叠，使用现有 settings/context modal 风格。
- 高风险候选明确标记，禁用 trial 按钮。
- 所有破坏性操作需要确认。
- capsule 文本编辑必须保留五个必填 section。

## 13. Agent 交互规范

### 13.1 任务收尾时的复盘格式

Agent 完成重要任务时，可在最终收尾前内部判断是否提交提案。只有满足以下任一条件才提交：

- 该流程预计会重复出现。
- 这次经验能减少明显返工。
- 有可验证证据。
- 可以写成简短、明确的步骤。

不满足时只写普通 memory 或不写。

### 13.2 Proposal capsule 模板

```md
When to use:
- ...

Avoid when:
- ...

Procedure:
- ...

Pitfalls:
- ...

Verification:
- ...
```

### 13.3 不允许的行为

- 不允许把用户偏好、敏感信息或一次性上下文写成通用 skill。
- 不允许没有证据就提交 proposal。
- 不允许为了“看起来智能”在每个任务后都创建候选项。
- 不允许创建与已有 candidate 高度重复的新 capability_id，应更新原 candidate。

## 14. 权限模型

| 操作 | user | foreman | peer |
|---|---:|---:|---:|
| 创建 proposal | 是 | 是 | 是 |
| 查看 candidate | 是 | 是 | 是 |
| dry-run | 是 | 是 | 否 |
| actor trial 自己 | 是 | 是 | 是 |
| actor trial 他人 | 是 | 是 | 否 |
| group trial | 是 | 是 | 否 |
| 添加 outcome | 是 | 是 | 是 |
| promote | 是 | 是 | 否 |
| block / retire | 是 | 是 | 否 |

## 15. 验收标准

### 15.1 后端

- 可以创建包含证据的 evolution candidate。
- 缺少必填 capsule section 时创建失败。
- 缺少 evidence refs 时创建失败。
- dry-run 成功后状态从 `draft` 变为 `ready_for_trial`。
- high risk candidate 不能 start trial。
- actor trial 会调用 capability import 并让目标 actor 的 `active_capsule_skills` 可见。
- outcome 记录可追加并可在列表中聚合。
- 不满足晋升条件时 promote 失败并返回明确原因。
- 满足晋升条件时 promote 成功，状态变为 `qualified`。
- block / retire 后不允许 start trial。

### 15.2 MCP

- `cccc_evolution(action=proposal_create)` 可由 peer 调用。
- peer 不能将 trial 启用到其他 actor。
- foreman 可执行 dry-run、start trial、promote、block、retire。
- MCP 返回错误码可读，不吞掉 capability import diagnostics。

### 15.3 前端

- Self-Evolving Skills 中能看到 Candidates。
- 候选详情能展示 evidence、capsule、outcomes、diagnostics。
- 不同状态只展示合法操作。
- dry-run / start trial / outcome / promote 后 UI 能刷新状态。
- 高风险候选 trial 按钮不可用。

### 15.4 回归

- 现有 `capability_import agent_self_proposed` 测试全部通过。
- 现有 context/task/coordination 测试不受影响。
- 未启用 evolution 时，现有 capability state 输出保持兼容。

## 16. 推荐开发拆分

### Milestone 1：后端最小闭环

- 新增 `kernel/evolution.py`。
- 新增 candidate/outcome 存储。
- 新增 daemon ops。
- 接入 capability import dry-run 与 enable。
- 单元测试覆盖状态机和权限。

完成定义：

- 不依赖前端即可通过 IPC/MCP 跑完整 `create -> dry-run -> trial -> outcome -> promote`。

### Milestone 2：MCP 工具

- 新增 `cccc_evolution` handler。
- 补充 toolspec。
- 覆盖 peer/foreman 权限测试。

完成定义：

- Agent 能通过 MCP 提交 proposal，foreman 能通过 MCP 晋升。

### Milestone 3：Web 工作台

- 扩展 Self-Evolving Skills tab。
- 增加 Candidate list/detail。
- 接入 dry-run、trial、outcome、promote、block、retire。
- 补充 i18n。

完成定义：

- 用户可在 UI 完成候选项治理，不需要命令行。

### Milestone 4：Agent 收尾引导

- 在 help / system prompt 中加入低噪声提案规则。
- 明确“不是每个任务都要提案”。
- 增加任务收尾 coverage check：若发现可复用经验，提交 proposal 或说明不提交原因。

完成定义：

- Agent 行为有章可循，不产生大量低质量候选。

## 17. MVP 示例流程

1. Peer 修复一次复杂部署问题。
2. Peer 发现“部署前必须检查 GitLab CI 变量和 Nginx runtime wiring”是可复用流程。
3. Peer 调用：

```ts
cccc_evolution({
  action: "proposal_create",
  title: "GitLab release preflight",
  problem: "部署类任务多次因为 CI 变量或 Nginx runtime wiring 漏检返工。",
  source_task_id: "task-123",
  evidence_refs: [
    { kind: "task", ref_id: "task-123", summary: "修复 release flow 时发现变量漏配。" },
    { kind: "test", ref_id: "tests/test_release_flow.py", summary: "新增回归覆盖。" }
  ],
  capsule_text: "When to use:\n- ...\n\nAvoid when:\n- ...\n\nProcedure:\n- ...\n\nPitfalls:\n- ...\n\nVerification:\n- ..."
})
```

4. Foreman 在 UI 点击 Dry Run。
5. Dry Run 通过后，Foreman 对负责 deploy 的 actor 开启 trial。
6. 下次 deploy 任务中该 actor 使用该 skill，并记录 `helped` outcome。
7. 两次 helped 后，Foreman promote。
8. 该 skill 进入 Self-Evolving Skills Library，后续可被同类 actor 启用。

## 18. 风险与控制

### 18.1 低质量候选过多

控制：

- 必须有 evidence。
- 必须包含完整 capsule sections。
- Agent 指南强调低噪声。
- UI 默认按状态和 outcome 过滤。

### 18.2 错误经验被固化

控制：

- 默认 draft，不直接启用。
- dry-run 与 trial 分离。
- high risk 禁止 trial。
- harmful outcome 阻止 promote。

### 18.3 能力污染全局

控制：

- MVP 禁止自动 global scope。
- group/actor 窄作用域默认。
- 晋升只代表 qualified，不代表自动启用。

### 18.4 与现有 capability 重复

控制：

- 生成 candidate 时搜索同 group 现有 capability_id/title。
- 相似度高时提示 revise existing。
- re-import 必须复用同一 capability_id。

## 19. 指标

MVP 可观测指标：

- 每周 proposal 数。
- draft -> trial 转化率。
- trial -> qualified 转化率。
- harmful outcome 比例。
- 每个 qualified skill 的使用次数。
- 使用 skill 的任务返工率变化。
- 被 retired / blocked 的原因分布。

## 20. 开发注意事项

- Evolution 不应成为第二套 capability 系统。
- 状态机必须集中在 kernel 层，前端只负责展示合法动作。
- capability import diagnostics 要完整透传，方便 Agent 和用户理解失败原因。
- `capability_id` 必须稳定，避免同一经验生成多个近似 skill。
- 所有写操作都要记录 `updated_by` 和时间。
- 文档和测试要同步更新 IPC / MCP contract。

## 21. Scope Coverage Check

- `done`：定义了共同进化的产品闭环。
- `done`：定义了 MVP 和非目标，避免过度设计。
- `done`：定义了数据模型、状态机、权限和 API。
- `done`：定义了与现有 capability_import / agent_self_proposed 的复用关系。
- `done`：定义了前端工作台需求。
- `done`：定义了验收标准和开发拆分。
- `deferred(reason)`：自动挖掘经验、全局发布、跨用户市场、源码自修改属于后续阶段，MVP 不做。
