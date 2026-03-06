# 使用场景

本页聚焦高投入产出比的真实 CCCC 工作流。

## 如何阅读本页

每个场景包含：
- 目标
- 最小化配置
- 执行流程
- 成功标准
- 常见失败点

## 场景 1：Builder + Reviewer 配对

### 目标

在不增加人工评审瓶颈的情况下提升交付质量。

### 最小化配置

```bash
cd /path/to/repo
cccc attach .
cccc setup --runtime claude
cccc setup --runtime codex
cccc actor add builder --runtime claude
cccc actor add reviewer --runtime codex
cccc group start
```

### 执行流程

1. 向 `@builder` 发送实现任务。
2. 向 `@reviewer` 发送评审标准（Bug 风险、回归风险、测试）。
3. 要求 `@builder` 回复变更文件列表 + 理由。
4. 要求 `@reviewer` 回复发现（严重程度 + 证据）。
5. 由人类做出最终合并决策。

### 成功标准

- 更快的实现反馈循环。
- 更少的遗漏回归。
- 评审输出是可操作的，而非泛泛而谈。

### 常见失败点

- 任务范围过大。
- 评审缺乏明确的验收标准。
- 团队在关键请求中跳过义务语义（`reply_required`）。

## 场景 2：Foreman 主导的多 Agent 交付

### 目标

将一个中型项目拆分为并行轨道，同时保持对齐。

### 最小化配置

```bash
cccc actor add foreman --runtime claude
cccc actor add frontend --runtime codex
cccc actor add backend --runtime gemini
cccc actor add qa --runtime copilot
cccc group start
```

### 执行流程

1. Foreman 在 Context 中定义共享目标（`vision`、`sketch`、`milestones`）。
2. 通过定向接收者分配聚焦任务。
3. 通过自动化规则强制检查点提醒。
4. Foreman 整合并解决冲突。
5. QA Agent 在交付前验证关键验收标准。

### 成功标准

- 并行执行，无重大返工。
- 每条轨道有清晰的所有权。
- 决策历史可在 Ledger 中追溯。

### 常见失败点

- 缺少共享的架构基线。
- Agent 在没有所有权规则的情况下编辑同一区域。
- 没有明确的集成检查点。

## 场景 3：通过 IM Bridge 移动运维

### 目标

通过手机操作长期运行的工作组，同时保持可靠的审计追踪。

### 最小化配置

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
```

然后在 IM 聊天中执行 `/subscribe`。

### 执行流程

1. 在 IM 中接收进度/错误通知。
2. 从手机向 `@foreman` 发送升级命令。
3. 仅在需要深度调试时切换到 Web UI。
4. 将所有关键决策保留在 CCCC 消息中（不仅仅是 IM 聊天记录）。

### 成功标准

- 无需笔记本电脑即可介入。
- 关键上下文保留在 Ledger 中。
- 减少通宵或外勤运维的停机时间。

### 常见失败点

- 暴露 Web UI 时未设置 Token/网关。
- 将 IM 作为唯一的事实来源。
- 没有重启/恢复预案。

## 场景 4：可重复的 Agent 基准测试框架

### 目标

运行可比较的多 Agent 会话，具备稳定的日志记录和可重放性。

### 最小化配置

1. 定义固定的任务提示和评估标准。
2. 每次运行使用相同的工作组模板和运行时配置。
3. 保持自动化策略的确定性。

### 执行流程

1. 创建基准工作组/模板。
2. 使用不同的运行时组合运行多次会话。
3. 收集 Ledger 和终端证据。
4. 评估结果质量和运维稳定性。

### 成功标准

- 可比较的运行，配置差异小。
- 可复现的证据集（`ledger`、状态产物、日志）。
- 清晰的模型/运行时权衡信号。

### 常见失败点

- 运行之间存在隐性 Prompt 漂移。
- 未受控的环境差异。
- 消息中缺少运行元数据。

## 推荐阅读

- [运维手册](/zh/guide/operations)
- [定位](/zh/reference/positioning)
- [功能特性](/zh/reference/features)
