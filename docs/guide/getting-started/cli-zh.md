# CLI 快速入门

通过命令行开始使用 CCCC。

[English](./cli) | 简体中文 | [日本語](./cli-ja)

## 开始之前

先按[安装指南](./#installation)安装 CCCC。本教程以 Claude Code 为例：
请先安装它的 CLI，完成登录，并在项目目录中处理必要的工作区信任确认。
也可以使用已经配置好的[其他受支持的 Runtime](../runtimes)。

在终端中确认 CCCC 可用：

```bash
cccc --version
cccc doctor
```

`doctor` 会报告安装情况、检测到的 Runtime 和 daemon 状态。检测到 Runtime 不代表
已经完成供应商登录。此时 daemon 尚未运行也没有关系，下一步会启动它。

## 第 1 步：启动 daemon

```bash
cccc daemon start
cccc daemon status
```

如果已有兼容的 daemon 正在运行，`daemon start` 会复用它。
`attach`、`actor`、`send` 等命令都需要 daemon 已启动。

## 第 2 步：创建工作组

```bash
cd /path/to/your/project
cccc attach .
```

这会创建一个新 Group，将当前目录关联为它的项目工作区（称为“scope”），
并将它设为当前 Group。输出中包含 `group_id`。要回到已有 Group，请使用
`cccc groups` 和 `cccc use <group_id>`；再次执行 `cccc attach .` 会另建一个 Group。

## 第 3 步：添加第一个 Agent

```bash
cccc actor add assistant --runtime claude
```

第一个启用的 Actor 会自动成为“foreman”（领班，负责协调工作）。

CCCC 启动这个 Claude Code 会话时会自动注入 CCCC MCP 配置，本例不需要单独执行
`cccc setup`。其他 Runtime 可能需要额外设置，详见
[受支持的 Runtime](../runtimes) 和 [`cccc setup`](/reference/cli#cccc-setup)。

## 第 4 步：启动 Agent

```bash
cccc group start
```

`group start` 会启动当前 Group 中已启用的 Actor。也可以只启动一个 Actor：

```bash
cccc actor start assistant
```

用 `cccc actor list` 查看 Actor 状态。如果启动过程中需要交互确认，
请在 [Web UI](#启动-web-ui-可选) 中打开该 Actor 的终端。

## 第 5 步：发送消息

```bash
cccc send "你好，请介绍一下自己。" --to assistant
```

## 第 6 步：查看回复

以 JSON 形式持续查看 Group 中记录的消息及其他事件：

```bash
cccc tail -f
```

关注 Agent 发出的 `chat.message` 事件。原生终端输出可在 Web UI 中查看。
按 **Ctrl+C** 结束查看，Agent 会继续运行。

`cccc inbox` 用于读取并**消费** Actor 的未读 Mail，将这批消息标为已读。
查看 Agent 的回复请使用 `tail` 或 Web 聊天界面。

## 添加更多 Agent

如果已安装 Codex 并完成登录，可以添加第二个 Agent：

```bash
cccc actor add reviewer --runtime codex
cccc actor start reviewer
```

指定消息的接收方：

```bash
cccc send "请实现这个功能" --to assistant
cccc send "请审查代码" --to reviewer
cccc send "请协调下一步工作" --to "@foreman"
cccc send "团队共同约束：CI 通过之前暂停部署" --to "@all"
```

如果一项工作需要跨会话持续跟踪负责人、预期成果和完成证据，可以使用 `tracked-send`：

```bash
cccc tracked-send "请实现这个功能，并回复验证结果。" \
  --to assistant \
  --title "实现功能" \
  --outcome "功能已实现，并已报告验证结果"
```

这个多行示例使用 Bash 的续行语法。在 PowerShell 中，请将命令写在同一行。

## 回复消息

将 `<event_id>` 替换为 `tail` 中要回复的那条消息的 `id`：

```bash
cccc reply <event_id> "谢谢，这样可以。"
cccc reply <event_id> "不紧急的补充说明" --to assistant --mode mail
```

Mail 仅支持 Agent 收件人，不会立即触发对方执行。

## 选择 Group 和发送者

支持 `--group` 的命令按以下顺序确定目标：显式选项 → `CCCC_GROUP_ID` 环境变量 →
通过 `attach` 或 `cccc use` 选中的当前 Group。仅切换目录不会切换 Group。
消息发送者按 `--by` → `CCCC_ACTOR_ID` → `user` 的顺序确定。
因此，即使其他会话切换了当前 Group，Actor 仍使用自己的 Group。

## 常用命令

给其他实例发送消息时，先用 `cccc connect` 查找可访问的 Group，
再使用 `cccc send --dst-instance ... --dst-group ...`。
回复时使用收到的消息在本地的事件 ID。包括 Direct 连接在内的完整流程见
[Agent 协作](../connect.md#agent-collaboration)。

### Group 管理

```bash
cccc groups                # 列出所有 Group
cccc use <group_id>        # 选择 Group
cccc active                # 显示当前 Group
cccc group show <group_id> # 显示 Group 信息
cccc group start           # 启动本组已启用的 Agent
cccc group stop            # 停止本组 Agent，保留历史
```

### Actor 管理

```bash
cccc actor list                  # 列出 Actor
cccc actor add <id> --runtime <r> # 添加 Actor
cccc actor start <id>            # 启动 Actor
cccc actor stop <id>             # 停止 Actor
cccc actor restart <id>          # 重启 Actor
cccc actor remove <id>           # 移除 Actor
```

### 消息

```bash
cccc send "消息"                   # 未指定 --to：使用默认收件策略（初始为 foreman）
cccc send "消息" --to assistant    # 发给指定 Actor
cccc send "消息" --to "@foreman"     # 发给领班
cccc send "消息" --to "@all"         # 显式广播，不是默认的任务分派方式
cccc tracked-send "工作内容" --to assistant --title "任务标题" --outcome "完成标准"
cccc reply <event_id> "回复内容"    # 回复消息
cccc inbox --actor-id assistant   # 读取并消费 Actor 的未读 Mail
cccc tail -n 50                   # 最近的事件
cccc tail -f                      # 持续查看新事件
```

### daemon 控制

```bash
cccc daemon status # 检查状态
cccc daemon start  # 启动 daemon
cccc daemon stop   # 停止 daemon 及其管理的各组进程
```

## 启动 Web UI（可选） {#启动-web-ui-可选}

在另一个终端中启动 Web UI：

```bash
cccc web
```

不带子命令的 `cccc` 作用相同。两者都会复用已运行的 daemon，需要时则自动启动它。
默认配置下访问 http://127.0.0.1:8848/；如果已修改监听地址或端口，
请使用启动时显示的地址。可以用 `cccc web --port 9000` 指定其他端口。

## 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `CCCC_HOME` | `~/.cccc` | 运行数据目录 |
| `CCCC_WEB_PORT` | `8848` | 没有优先级更高的已保存设置或 `--port` 时使用的 Web UI 端口 |

需要共享 Group 和运行状态的命令应使用同一个 `CCCC_HOME`。
完整的 Web 监听选项及优先级见 [CLI 参考](/reference/cli#environment-variables)。

## 结束工作

按 **Ctrl+C** 退出 `tail -f` 后，停止当前 Group 的 Agent：

```bash
cccc group stop
```

Group 和已记录的历史会保留。用 `cccc group start` 可以重新启动这些 Actor。
若要停止整个实例，包括其他 Group 的托管进程，请使用 `cccc daemon stop`。

## 排查问题

### daemon 无法启动

```bash
cccc daemon status
cccc doctor
```

如果 daemon 已停止，执行 `cccc daemon start`。
如果它没有响应，先查看报错，再决定是否停止并重启；重启 daemon 会中断各 Group 的托管进程。

### Agent 没有回复

```bash
# 检查 Actor 状态
cccc actor list

# 查看记录的错误和消息
cccc tail -n 50
```

在 Web UI 的 Actor 终端中检查是否有登录、信任或工具审批提示，
并确认[对应 Runtime 的配置](../runtimes)。修正原因后，可以用
`cccc actor start <actor_id>` 再次启动，必要时用 `cccc actor restart <actor_id>` 重启。

### 找不到 Group

```bash
# 列出所有 Group
cccc groups

# 选择已有 Group
cccc use <group_id>
cccc active
```

确认当前终端使用了预期的 `CCCC_HOME`。
需要创建新 Group，或明确指定已有 Group 以附加 scope 时，才使用 `attach`。

## 后续阅读

以下相关指南为英文。

- [工作流](/guide/workflows) — 了解协作方式
- [CLI 参考](/reference/cli) — 完整命令说明
- [IM 桥接](/guide/im-bridge/) — 配置移动端访问
