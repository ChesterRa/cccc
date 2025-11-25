# CCCC Pair — 双AI自主协作编排器

[English](README.md) | **中文** | [日本語](README.ja.md)

两个AI作为平等的伙伴**自主协作、自动推进任务**——你设定目标，它们自己沟通、规划、实现、互相review。你通过TUI或聊天工具随时掌控全局，但不需要持续介入。

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![PyPI](https://img.shields.io/pypi/v/cccc-pair)](https://pypi.org/project/cccc-pair/)
[![Python](https://img.shields.io/pypi/pyversions/cccc-pair)](https://pypi.org/project/cccc-pair/)
[![Telegram](https://img.shields.io/badge/Telegram-社区-2CA5E0?logo=telegram)](https://t.me/ccccpair)

---

![CCCC TUI 界面](./screenshots/tui-main.png)

### 运行时实景

![CCCC 运行时界面](./screenshots/tui-runtime.png)

> **四窗格布局**：左上TUI控制台（Timeline + 状态栏）、右上PeerA终端、右下PeerB终端。图中展示Foreman正在进行战略分析，两个Peer（均使用opencode）各自处理任务并自动协调。

---

## 为什么用双AI而不是单Agent？

### 单Agent的痛点

| 问题 | 表现 |
|------|------|
| **需要持续盯着** | 单Agent做一会儿就停了，必须不断给它新的提示才能继续 |
| **上下文丢失** | 跨session工作时，之前聊的都忘了，反复解释同样的事 |
| **缺乏验证** | 长篇大论说了一堆，但不知道对不对，没有真正跑过 |
| **决策不透明** | 出问题时难以追溯：什么时候改的？为什么改？谁批准的？ |

### CCCC的解决方案

| 特性 | 效果 |
|------|------|
| **自主推进** | 两个Peer之间自动沟通协调，单轮可持续运行10-15分钟；叠加Foreman定时唤醒，可实现接近不间断的持续运行 |
| **互相制衡** | 一个提方案，另一个挑毛病；更好的选项自然浮现，错误更早暴露 |
| **证据说话** | 只有测试通过、日志稳定、代码提交了才算"完成"，不是嘴上说说 |
| **全程可追溯** | 每个决策、每次交接都有记录，出问题能查，能回滚 |

---

## 核心功能

### 自主协作引擎

- **双Peer架构**：PeerA和PeerB作为平等伙伴，通过mailbox机制自动交换信息、推进任务
- **智能交接**：内置handoff机制，Peer之间自动传递上下文和工作成果
- **自我检查**：定期self-check确保方向正确，避免跑偏
- **保活机制**：Keepalive防止对话停滞，nudge提醒处理待办事项

### 零配置TUI

- **交互式设置**：首次启动显示Setup面板，↑↓选择、Enter确认，不用编辑配置文件
- **实时Timeline**：滚动查看所有对话流，PeerA/PeerB/System/You的消息一目了然
- **状态面板**：实时显示handoff计数、self-check进度、Foreman状态
- **命令补全**：Tab自动补全，Ctrl+R搜索历史，标准快捷键全支持

### 证据驱动工作流

- **POR/SUBPOR锚点**：战略板（POR.md）和任务单（SUBPOR.md）存在仓库里，所有人看到同一份真相
- **小步提交**：每个patch不超过150行，可review、可回滚
- **审计日志**：ledger.jsonl记录所有事件，出问题能追溯

### 多角色体系

| 角色 | 职责 | 必需？ |
|------|------|--------|
| **PeerA** | 主要执行者之一，与PeerB平等协作 | 是 |
| **PeerB** | 主要执行者之一，与PeerA平等协作 | 是 |
| **Aux** | 按需调用的辅助角色，处理批量任务、重型测试等 | 否 |
| **Foreman** | 定时运行的"用户代理"，执行周期性检查和提醒 | 否 |

> **自由搭配**：任何角色都可以使用任何支持的CLI，根据需要灵活配置。

### 多平台桥接

- **Telegram / Slack / Discord**：可选接入，把工作带到团队常用的地方
- **双向通信**：在聊天里发指令、收状态、审批RFD
- **文件互传**：支持双向文件交换——你可以上传文件给Peer处理，Peer生成的文件也会推送给你

**IM 聊天命令**：

| 平台 | 命令 | 说明 |
|------|------|------|
| 全平台 | `a: <消息>` / `b: <消息>` / `both: <消息>` | 路由到Peer |
| 全平台 | `a! <命令>` / `b! <命令>` | CLI直通（无包装直接输入） |
| 全平台 | `aux: <提示>` 或 `/aux <提示>` | 调用Aux执行一次 |
| 仅Telegram | `/pa` `/pb` `/pboth` | 群组中的直通命令 |
| 仅Telegram | `/help` `/whoami` `/status` `/subscribe` `/verbose` | 元命令和设置 |
| 仅Telegram | `/focus` `/reset` `/foreman` `/restart` | 控制命令 |

> Slack 和 Discord 命令支持较少，建议使用前缀语法（`a:`、`a!`、`aux:`）以确保全平台兼容。

---

## 关键配置文件：PROJECT.md 与 FOREMAN_TASK.md

这两个文件是你与AI沟通任务的核心入口，**务必认真编写**：

### PROJECT.md（项目描述）

位于仓库根目录，**自动注入给PeerA和PeerB**的系统提示词中。

**应该包含**：
- 项目背景和目标
- 技术栈和架构概述
- 代码规范和约定
- 当前阶段的重点任务
- 任何Peer需要知道的上下文

```markdown
# 项目简介
这是一个xxx系统，使用Python + FastAPI + PostgreSQL...

# 当前重点
1. 完成用户认证模块
2. 优化数据库查询性能

# 代码规范
- 使用type hints
- 每个函数需要docstring
- 测试覆盖率 > 80%
```

### FOREMAN_TASK.md（监工任务）

位于仓库根目录，**自动注入给Foreman**。Foreman每15分钟执行一次，读取此文件决定做什么。

**应该包含**：
- 需要定期检查的事项
- 常驻任务列表
- 质量门禁要求

```markdown
# Foreman 常驻任务

## 每次检查
1. 运行 `pytest` 确保测试通过
2. 检查 POR.md 是否需要更新
3. 查看是否有未处理的TODO

## 质量要求
- 不允许跳过失败的测试
- 新代码必须有对应测试
```

> **提示**：任务越复杂，这两个文件就越重要。写清楚你的意图，Peer们才能准确理解并自主推进。

---

## 支持的Agent CLI

CCCC不绑定特定AI，任何角色都可以使用以下任一CLI：

| CLI | 官方文档 |
|-----|----------|
| **Claude Code** | [docs.anthropic.com/claude-code](https://docs.anthropic.com/en/docs/claude-code) |
| **Codex CLI** | [github.com/openai/codex](https://github.com/openai/codex) |
| **Gemini CLI** | [github.com/google-gemini/gemini-cli](https://github.com/google-gemini/gemini-cli) |
| **Factory Droid** | [factory.ai](https://factory.ai/) |
| **OpenCode** | [github.com/opencode-ai/opencode](https://github.com/opencode-ai/opencode) |
| **Kilocode** | [kilo.ai/docs/cli](https://kilo.ai/docs/cli) |
| **GitHub Copilot** | [github.com/features/copilot/cli](https://github.com/features/copilot/cli) |
| **Augment Code** | [docs.augmentcode.com/cli](https://docs.augmentcode.com/cli/overview) |
| **Cursor** | [cursor.com/cli](https://cursor.com/en-US/cli) |

> 安装方式请参考各CLI的官方文档。任何遵循mailbox协议的CLI都可以接入。

---

## 快速开始

### 第一步：安装前置依赖

CCCC使用tmux管理多窗格终端布局，请先确保以下依赖已安装：

| 依赖 | 说明 | 安装方法 |
|------|------|----------|
| **Python** | ≥ 3.9 | 大多数系统已预装 |
| **tmux** | 终端复用器，用于多窗格布局 | macOS: `brew install tmux`<br>Ubuntu/Debian: `sudo apt install tmux`<br>Windows: 需使用WSL |
| **git** | 版本控制 | 大多数系统已预装 |
| **Agent CLI** | 至少安装一个 | 见下方说明 |

**Agent CLI安装**（至少安装一个）：
```bash
# Claude Code（推荐）
npm install -g @anthropic-ai/claude-code

# Codex CLI
npm install -g @openai/codex

# Gemini CLI
npm install -g @anthropic-ai/gemini-cli

# OpenCode
go install github.com/opencode-ai/opencode@latest
```

> **Windows用户**：CCCC需要在WSL（Windows Subsystem for Linux）环境下运行。请先[安装WSL](https://docs.microsoft.com/zh-cn/windows/wsl/install)，然后在WSL终端中进行后续操作。

### 第二步：安装CCCC

```bash
# 方式一：用pipx（推荐，自动隔离环境）
pip install pipx  # 如果没有pipx先安装
pipx install cccc-pair

# 方式二：用pip
pip install cccc-pair
```

### 第三步：初始化并启动

```bash
# 1. 进入你的项目目录
cd your-project

# 2. 初始化CCCC（创建.cccc/目录和配置文件）
cccc init

# 3. 检查环境是否就绪
cccc doctor

# 4. 启动！
cccc run
```

**启动后会看到**：
- tmux打开4窗格布局：左上TUI、左下日志、右上PeerA、右下PeerB
- 首次运行显示Setup面板，用↑↓选择CLI绑定到各角色
- 确认后Peer们自动启动，开始工作

> **提示**：如果 `cccc doctor` 报错，请根据提示安装缺失的依赖。聊天桥接（Telegram/Slack/Discord）可在TUI的Setup面板里配置。

---

## 常用命令

在TUI输入框里使用（Tab可补全）：

| 命令 | 作用 |
|------|------|
| `/a <消息>` | 发送给PeerA |
| `/b <消息>` | 发送给PeerB |
| `/both <消息>` | 同时发送给两个Peer |
| `/pause` | 暂停handoff循环 |
| `/resume` | 恢复handoff循环 |
| `/refresh` | 刷新系统提示词 |
| `/setup` | 打开/关闭设置面板 |
| `/foreman on\|off\|status\|now` | 控制Foreman |
| `/aux <提示>` | 调用Aux执行一次性任务 |
| `/verbose on\|off` | 开关详细输出 |
| `/help` | 查看所有命令 |
| `/quit` | 退出 |

**自然语言路由**（不用斜杠也行）：
```
a: 帮我review这个PR的安全性
b: 跑一下完整的测试套件
both: 我们来规划下一个milestone
```

---

## 键盘快捷键

| 快捷键 | 作用 |
|--------|------|
| `Tab` | 命令补全 |
| `↑ / ↓` | 浏览历史命令 |
| `Ctrl+R` | 反向搜索历史 |
| `Ctrl+A / E` | 跳到行首/行尾 |
| `Ctrl+W` | 删除前一个词 |
| `Ctrl+U / K` | 删除到行首/行尾 |
| `PageUp / PageDown` | 滚动Timeline |
| `Ctrl+L` | 清空Timeline |

---

## 高级功能

### Auto-Compact（上下文压缩）

长时间运行后自动压缩Peer的上下文，防止token浪费和思维混乱：
- 检测Peer空闲状态
- 满足条件时自动触发（默认：≥6条消息、间隔15分钟、空闲2分钟）
- 支持的CLI会收到compact指令

### Foreman（用户代理）

可选的定时任务角色，每隔一段时间（默认15分钟）执行一个预设任务：
- 编辑 `FOREMAN_TASK.md` 定义任务
- 用 `/foreman on|off` 控制开关
- 适合周期性检查、提醒更新POR等场景

### RFD（请求决策）

重大决策需要人工批准：
- Peer发起RFD卡片
- 聊天桥接显示审批按钮
- 用户批准后才继续执行

---

## 目录结构

```
.cccc/                          # 编排器域（默认gitignore）
  settings/                     # 配置文件
    cli_profiles.yaml           # 角色绑定、交付配置
    agents.yaml                 # CLI定义
    telegram.yaml / slack.yaml  # 桥接配置
  mailbox/                      # 消息交换
  state/                        # 运行时状态
    ledger.jsonl                # 事件日志
    status.json                 # 当前状态
  logs/                         # Peer日志
  rules/                        # 系统提示词

docs/por/                       # 锚点文档
  POR.md                        # 战略板
  T######-slug/SUBPOR.md        # 任务单

PROJECT.md                      # 项目简介（会织入系统提示词）
FOREMAN_TASK.md                 # Foreman任务定义
```

---

## 常见问题

### 两个Peer真的能自动协作吗？

是的。通过mailbox机制，PeerA和PeerB会自动交换消息、传递工作成果。你设定目标后，它们会自己讨论方案、分工实现、互相review。你可以随时介入，但不是必须的。

### 我需要一直盯着屏幕吗？

不需要。这是CCCC区别于单Agent的核心价值。设定好任务后，Peer们会自主推进。你可以通过Telegram/Slack/Discord随时查看进度，有需要人工决策的事项会通过RFD通知你。

### 哪个CLI更好？

取决于你的需求。每个CLI有不同特点，可以自由搭配到任何角色。建议先用默认配置试试，再根据实际体验调整。

### 需要编辑配置文件吗？

基本不需要。TUI的Setup面板支持点选配置。高级用户可以直接编辑 `.cccc/settings/` 下的YAML文件进行精细调整。

### 出问题了怎么排查？

1. 查看 `.cccc/state/status.json` 了解当前状态
2. 查看 `.cccc/state/ledger.jsonl` 查看事件日志
3. 查看 `.cccc/state/orchestrator.log` 查看运行日志
4. 运行 `cccc doctor` 检查环境

---

## 更多信息

详细的架构说明、完整配置参考、更多FAQ请查看 [英文文档](README.md)。

---

## 社区与支持

- **Telegram社区**: [t.me/ccccpair](https://t.me/ccccpair)
- **微信**: dodd85（添加时请备注"CCCC"，人多后会建群）
- **GitHub Issues**: [报告问题或建议](https://github.com/anthropics/cccc/issues)

---

## License

MIT
