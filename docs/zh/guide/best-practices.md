# 最佳实践

充分利用 CCCC 的实用技巧。

## 成功的准备工作

### 编写好的 PROJECT.md

在项目根目录放置 `PROJECT.md`。这是项目的"宪法"：

```markdown
# 项目名称

## 目标
一句话描述我们在构建什么。

## 约束
- 必须使用 TypeScript
- 遵循现有代码模式
- 未经批准不得引入外部依赖

## 架构
代码库结构的简要概述。

## 当前重点
我们目前正在做的事情。
```

Agent 通过 `cccc_project_info` 读取此文件来了解上下文。

### 自定义协作手册

协作手册是 Agent 遵循的协作契约。你可以按工作组自定义。

#### 文件优先级

CCCC 按以下优先级加载协作内容：

1. **工作组覆盖（CCCC_HOME）**：`CCCC_HOME/groups/<group_id>/prompts/CCCC_HELP.md`（最高优先级）
2. **内置默认**：`cccc.resources/cccc-help.md`（兜底）

要自定义，请编辑工作组提示词覆盖（推荐：Web UI → 设置 → 指导）。

你也可以从 Web UI 中查看文件路径（它会显示每个工作组的确切覆盖路径）。

#### 条件内容标记

你可以使用条件标记向不同角色展示不同内容：

```markdown
# 我的项目帮助

## 通用规则（所有角色可见）
- 遵循编码规范
- 为新功能编写测试

## @role: foreman
### 仅 Foreman 可见的部分
- 你负责协调团队
- 对架构做最终决策

## @role: peer
### 仅 Peer 可见的部分
- 专注于分配给你的任务
- 向 foreman 报告阻塞问题

## @actor: impl-agent
### 仅 impl-agent 可见的部分
- 你负责核心实现
```

**标记语法：**
- `## @role: foreman` - 仅 foreman 可见
- `## @role: peer` - 仅 peer 可见
- `## @actor: <actor_id>` - 仅特定 actor 可见
- 无标记的部分对所有角色可见

#### Agent 如何使用协作手册

Agent 通过 MCP 工具访问协作内容：

1. **`cccc_bootstrap`** - 在会话初始化时返回协作手册
2. **`cccc_help`** - 按需返回协作内容

返回格式为：
```json
{
  "markdown": "<根据角色/actor 过滤的内容>",
  "source": "CCCC_HOME/.../prompts/CCCC_HELP.md 或 cccc.resources/cccc-help.md"
}
```

Agent 读取 markdown 并语义化地遵循规则。没有特殊的解析——内容以清晰的命令式语言编写，AI 能自然理解。

#### 编写有效协作内容的技巧

- 使用命令式语言："必须使用 MCP" 而不是 "应该考虑使用 MCP"
- 具体明确："30 秒内回复" 而不是 "快速响应"
- 用清晰的标题结构方便导航
- 为复杂工作流提供示例
- 保持章节聚焦——每个部分一个主题

### 选择合适的 Agent 组合

| 场景 | 推荐配置 |
|------|----------|
| 单人开发 | 1 个 Claude agent |
| 编码 + 审查 | Claude（实现）+ Codex（审查） |
| 全栈项目 | 多个专业化 agent |
| 学习/探索 | 1 个 agent，交互模式 |

### 正确配置运行时

使用推荐的自主运行标志：

```bash
# Claude Code
cccc actor add impl --runtime claude
# 使用：claude --dangerously-skip-permissions

# Codex
cccc actor add review --runtime codex
# 使用：codex --dangerously-bypass-approvals-and-sandbox --search
```

## 有效沟通

### 具体明确

❌ "修复这个 bug"
✅ "修复在移动端 Safari 上登录按钮无响应的问题"

❌ "让它更快"
✅ "优化 getUserById 查询，目前耗时 500ms"

### 合理使用 @提及

- `@all` 用于公告或通用问题
- `@foreman` 用于协调决策
- `@specific-agent` 用于定向任务

### 提供上下文

包含相关信息：
- 错误消息
- 文件路径
- 预期行为 vs 实际行为
- 约束或偏好

### 使用回复保持对话有序

回复消息以保持对话组织性。Agent 能看到引用的上下文。

## 任务管理

### 拆分大型任务

不要这样："实现用户认证"

使用里程碑：
1. 用户数据库 schema
2. 注册接口
3. 登录接口
4. 会话管理
5. 密码重置流程

### 设定明确的完成标准

为每个任务定义"完成"：
- 测试通过
- 无 lint 错误
- 文档已更新
- 代码已审查

### 使用 Context 面板

- **Vision**：保持项目目标可见
- **Sketch**：记录技术方案
- **Milestones**：跟踪主要阶段
- **Tasks**：分解当前工作
- **Notes**：记录发现和教训

## 多 Agent 协作

### 定义清晰的角色

| 角色 | 职责 |
|------|------|
| Foreman | 协调、决策、执行工作 |
| 实现者 | 编写代码、遵循规格 |
| 审查者 | 审查代码、提出改进建议 |
| 测试者 | 编写测试、发现 bug |

### 避免冲突

- 将不同的文件/模块分配给不同的 agent
- 对共享资源使用顺序工作流
- 让 foreman 解决冲突

### 定期同步

定期检查：
- 所有人是否对齐目标？
- 有阻塞问题吗？
- 有冲突的修改吗？

## 故障排除技巧

### Agent 无响应

1. 检查终端标签页是否有错误
2. 验证 MCP 设置：`cccc setup --runtime <name>`
3. 尝试重启：在 Web UI 中点击"重启"
4. 检查 Daemon 健康状态和近期事件：`cccc daemon status` 和 `cccc tail -n 100`

### 消息未投递

1. 确保 agent 已启动（绿色指示灯）
2. 检查收件箱：`cccc inbox --actor-id <id>`
3. 验证 `to` 字段是否正确

### 上下文变得陈旧

如果 agent 看起来混乱：
1. 重启以清除上下文
2. 重新陈述当前目标
3. 明确引用相关文件

### 陷入循环

如果 agent 不断重复：
1. 停止该 agent
2. 清除任务
3. 用更清晰的指令重启

## 安全最佳实践

### 远程访问

- 远程访问时始终使用 `CCCC_WEB_TOKEN`
- 优先使用 Cloudflare Access 或 Tailscale，而非直接暴露
- 不要直接将 8848 端口暴露到互联网

### Token 管理

- 将 token 存储在环境变量中
- 不要将 token 提交到 git
- 定期轮换 token

### 审查 Agent 的变更

- 推送前检查提交
- 使用代码审查 agent
- 设置 CI/CD 安全防线
