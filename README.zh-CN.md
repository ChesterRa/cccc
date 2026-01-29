# CCCC — 多智能体协作内核

[English](README.md) | **中文** | [日本語](README.ja.md)

> **状态**: 0.4.0rc17 (Release Candidate)

[![Documentation](https://img.shields.io/badge/docs-online-blue)](https://dweb-channel.github.io/cccc/)
[![License](https://img.shields.io/badge/license-Apache--2.0-green)](LICENSE)

CCCC 是一个**本地优先的多智能体协作内核**，像现代 IM 一样协调 AI 智能体。

**核心特性**：
- 🤖 **多运行时支持** — Claude Code、Codex CLI、Droid、OpenCode、Copilot 等
- 📝 **追加式账本** — 持久历史，唯一事实源
- 🌐 **Web 优先控制台** — 移动端友好
- 💬 **IM 级消息体验** — @mentions、reply/quote、已读回执
- 🔧 **MCP 工具面** — 38+ 工具，可靠的智能体操作
- 🔌 **IM 桥接** — Telegram、Slack、Discord、飞书、钉钉

![CCCC Chat UI](screenshots/chat.png)

---

## 快速开始

```bash
# 安装
pip install --index-url https://pypi.org/simple \
  --extra-index-url https://test.pypi.org/simple \
  cccc-pair==0.4.0rc17

# 启动
cccc
```

打开 `http://127.0.0.1:8848/` 访问 Web UI。

---

## 文档

📚 **[在线文档](https://dweb-channel.github.io/cccc/)** — 完整指南、参考和 API 文档。

---

## 安装

### 使用 AI 助手安装

复制以下提示词发送给你的 AI 助手（Claude、ChatGPT 等）：

> 请帮我安装并启动 CCCC（Claude Code Collaboration Context）多智能体协作系统。
>
> 执行以下步骤：
>
> 1. 安装 cccc-pair：
>    ```
>    pip install --index-url https://pypi.org/simple \
>      --extra-index-url https://test.pypi.org/simple \
>      cccc-pair==0.4.0rc17
>    ```
>
> 2. 安装完成后，启动 CCCC：
>    ```
>    cccc
>    ```
>
> 3. 告诉我访问地址（通常是 http://localhost:8848/ui/）
>
> 如果遇到任何错误，请帮我诊断并解决。

### 从旧版本升级

如果你已安装旧版本的 cccc-pair（如 0.3.x），必须先卸载：

```bash
# pipx 用户
pipx uninstall cccc-pair

# pip 用户
pip uninstall cccc-pair

# 如有残留，手动删除
rm -f ~/.local/bin/cccc ~/.local/bin/ccccd
```

> **注意**：0.4.x 版本的命令结构与 0.3.x 完全不同。旧版的 `init`、`run`、`bridge` 命令已被 `attach`、`daemon`、`mcp` 等替代。

### 从 TestPyPI 安装（推荐）

```bash
pip install --index-url https://pypi.org/simple \
  --extra-index-url https://test.pypi.org/simple \
  cccc-pair==0.4.0rc17
```

### 从源码安装

```bash
git clone https://github.com/dweb-channel/cccc
cd cccc
pip install -e .
```

### 使用 uv（推荐 Windows 用户）

```bash
uv venv -p 3.11 .venv
uv pip install -e .
uv run cccc --help
```

**运行要求**：Python 3.9+，macOS / Linux / Windows

---

## 核心概念

| 概念 | 说明 |
|------|------|
| **Working Group** | 协作单位，有持久历史（类似群聊） |
| **Actor** | 智能体会话（PTY 或 headless） |
| **Scope** | 绑定到 group 的目录 |
| **Ledger** | 追加式事件流 |
| **CCCC_HOME** | 运行时目录，默认 `~/.cccc/` |

---

## 运行时与 MCP

CCCC 支持多种智能体运行时：

```bash
cccc runtime list --all     # 列出可用运行时
cccc setup --runtime <name> # 配置 MCP
```

**自动配置 MCP**：`claude`、`codex`、`droid`、`amp`、`auggie`、`neovate`、`gemini`
**手动配置**：`cursor`、`kilocode`、`opencode`、`copilot`、`custom`

---

## 多智能体配置

在项目上配置多智能体协作：

```bash
# 绑定项目目录
cd /path/to/repo
cccc attach .

# 为运行时配置 MCP
cccc setup --runtime claude

# 添加 actors（第一个启用的自动成为 foreman）
cccc actor add foreman --runtime claude
cccc actor add peer-1  --runtime codex

# 启动 group
cccc group start
```

---

## Web UI

内置 Web UI 提供：

- 多 group 导航
- Actor 管理（add/start/stop/restart）
- Chat（@mentions + reply）
- 每个 actor 的内嵌终端
- Context 与自动化设置
- IM Bridge 配置

---

## IM 桥接

将工作组桥接到 IM 平台：

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
```

支持：**Telegram** | **Slack** | **Discord** | **飞书** | **钉钉**

---

## CLI 速查

```bash
cccc doctor              # 检查环境
cccc groups              # 列出 groups
cccc use <group_id>      # 切换 group
cccc send "msg" --to @all
cccc inbox --mark-read
cccc tail -n 50 -f
cccc daemon status|start|stop
```

---

## PROJECT.md

在 repo 根目录放置 `PROJECT.md` 作为项目宪法。智能体通过 `cccc_project_info` MCP 工具读取。

---

## 安全提示

Web UI 权限很高。远程访问时：
- 设置 `CCCC_WEB_TOKEN` 环境变量
- 使用访问网关（Cloudflare Access、Tailscale、WireGuard）

---

## 为什么重写？

<details>
<summary>历史：v0.3.x → v0.4.x</summary>

v0.3.x（tmux-first）验证了概念，但遇到了瓶颈：

1. **没有统一 ledger** — 消息分散在多个文件，延迟高
2. **actor 数量受限** — tmux 布局限制为 1–2 个 actor
3. **智能体控制能力弱** — 自主性受限
4. **远程访问不是一等体验** — 需要 Web 控制台

v0.4.x 引入：
- 统一的追加式 ledger
- N-actor 模型
- 38+ MCP 工具的控制平面
- Web 优先控制台
- IM 级消息体验

旧版：[cccc-tmux](https://github.com/ChesterRa/cccc-tmux)

</details>

---

## License

Apache-2.0
