# Web UI 指南

CCCC Web UI 是一个移动优先的控制面板，用于管理你的 AI Agent。

## 访问 Web UI

启动 CCCC 后：

```bash
cccc
```

在浏览器中打开 http://127.0.0.1:8848/。

## 界面概览

Web UI 包含以下主要区域：

- **顶栏**：工作组选择器、设置、主题切换
- **侧边栏**：工作组列表和导航
- **标签页**：Chat 标签 + 每个 Agent 一个标签
- **主区域**：聊天消息或终端视图
- **输入框**：支持 @mention 的消息编辑器

## 管理工作组

### 创建工作组

1. 点击侧边栏的 **+** 按钮
2. 或使用 CLI：`cccc attach /path/to/project`

### 切换工作组

在侧边栏点击工作组即可切换。

### 工作组设置

1. 点击顶部的 **设置** 图标
2. 可配置：
   - 工作组标题
   - 引导文本（preamble/help）
   - 自动化规则和引擎策略
   - 消息投递和默认设置
   - IM Bridge 设置

## 管理 Agent

### 添加 Agent

1. 点击 **Add Actor** 按钮
2. 选择运行时（Claude、Codex 等）
3. 设置 Actor ID 和选项
4. 点击 **Create**

### 启动/停止 Agent

- 点击 **Play** 按钮启动 Agent
- 点击 **Stop** 按钮停止
- 使用 **Restart** 清除上下文并重启

### 查看 Agent 终端

点击 Agent 的标签页查看终端输出。

## 消息

### 发送消息

1. 在底部输入框中输入
2. 按 `Ctrl+Enter` / `Cmd+Enter`，或点击发送

### @提及

输入 `@` 触发自动补全：

- `@all` — 发送给所有 Agent
- `@foreman` — 发送给 Foreman
- `@peers` — 发送给所有 Peer
- `@<actor_id>` — 发送给特定 Agent

### 回复

点击消息上的回复图标进行引用回复。

## Context 面板

Context 面板展示共享的项目状态：

### Vision

一句话项目目标。Agent 应与此对齐。

### Sketch

执行计划或架构草图。静态内容，不含 TODO。

### Milestones（里程碑）

粗粒度的项目阶段（通常 2-6 个）。

### Tasks（任务）

带步骤和验收标准的详细工作项。

### Notes（笔记）

经验教训、发现、警告。

### References（参考）

有用的文件和 URL。

## 设置面板

通过齿轮图标访问：

### 自动化

- **规则**：创建基于间隔 / 循环计划 / 一次性计划的提醒。
- **动作**：
  - `Send Reminder`（常规提醒投递）
  - `Set Group Status`（运维操作，仅一次性）
  - `Control Actor Runtimes`（运维操作，仅一次性）
- **一次性行为**：一次性规则触发后自动完成，可从已完成列表中清理。
- **引擎策略**：配置内置推送（reply-required、attention ACK、unread）、Actor 空闲、keepalive、静默和帮助推送。

### IM Bridge

配置 Telegram、Slack、Discord、飞书或钉钉集成。

### 主题

在浅色、深色或跟随系统主题间切换。

## 移动端使用

Web UI 响应式设计，在移动端表现良好：

- 滑动切换标签页
- 下拉刷新
- 长按弹出上下文菜单
- 支持移动浏览器（Chrome、Safari）

## 远程访问

从局域网外部访问：

### Cloudflare Tunnel（推荐）

```bash
cloudflared tunnel --url http://127.0.0.1:8848
```

### Tailscale

```bash
CCCC_WEB_HOST=$(tailscale ip -4) cccc
```

### 安全

暴露 Web UI 时务必设置 `CCCC_WEB_TOKEN`：

```bash
export CCCC_WEB_TOKEN="your-secret-token"
cccc
```

然后认证一次以建立会话 Cookie：

- 打开 `http://YOUR_HOST:8848/?token=your-secret-token`（或 `.../ui/?token=...`）

之后即可正常使用 Web UI，无需 `?token=...`。
