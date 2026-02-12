# CCCC — 本地优先多智能体协作内核

[English](README.md) | **中文** | [日本語](README.ja.md)

[![Documentation](https://img.shields.io/badge/docs-online-blue)](https://dweb-channel.github.io/cccc/)
[![License](https://img.shields.io/badge/license-Apache--2.0-green)](LICENSE)

CCCC 不是“多个终端窗口里跑几个 agent”的临时玩法，而是一套可长期运营的多智能体协作底座。

你会得到：
- 可追溯的协作事实流（`ledger.jsonl`）
- Web/CLI/MCP/IM 统一控制面
- 明确的消息触达语义（read/ack/reply-required）
- 多运行时编排能力（Claude、Codex、Gemini、Copilot 等）

![CCCC Chat UI](screenshots/chat.png)

## 为什么需要 CCCC

多智能体开发常见痛点：
- 协作记录散落在终端滚动日志里，无法稳定回放
- 消息“发没发到”语义模糊，运维和排障成本高
- 启停、恢复、催办、提醒等操作分散在多个入口
- 手机/IM 远程值守体验脆弱

CCCC 的核心思路：
- **append-only ledger** 作为唯一事实源
- **daemon 单写者** 保证状态一致性
- **多端薄入口**（Web/CLI/MCP/IM）统一调用内核
- **本地优先运行时目录**（`CCCC_HOME`，默认 `~/.cccc`）

## 10 分钟快速上手

### 1) 安装

```bash
python -m pip install -U cccc-pair
```

如需验证指定 RC：

```bash
python -m pip install --index-url https://pypi.org/simple \
  --extra-index-url https://test.pypi.org/simple \
  cccc-pair==0.4.0rc20
```

### 2) 启动

```bash
cccc
```

打开 `http://127.0.0.1:8848/`。

### 3) 建立第一个多智能体协作组

```bash
cd /path/to/repo
cccc attach .
cccc setup --runtime claude
cccc actor add foreman --runtime claude
cccc actor add reviewer --runtime codex
cccc group start
cccc send "请先拆分任务并开始实现。" --to @all
```

## 产品能力总览

- **多智能体运行时编排**
  - actor 级 add/start/stop/restart
  - foreman + peer 角色模型与权限边界
- **持久化协作账本**
  - 所有消息与事件 append-only
  - 支持回放、排障、审计
- **IM 级消息语义**
  - `@all/@peers/@foreman/actor_id` 精确路由
  - 结构化回复、已读游标、attention ACK、reply-required
- **自动化与系统策略**
  - interval / recurring / one-time 触发
  - reminder 与受控运维动作
- **多入口运维**
  - Web UI 可视化控制
  - CLI 脚本化
  - MCP agent 自治
  - IM 桥接移动值守

## CCCC 的定位

| 需求 | CCCC 适配度 |
|---|---|
| 多 coding agents 的稳定协作底座 | 非常适合 |
| 人类 + 智能体的可追溯协作记录 | 非常适合 |
| 手机/IM 远程值守与轻运维 | 适合 |
| 强 DAG 编排与复杂调度编排 UI | 建议与外部编排器组合 |

CCCC 是 **协作内核（collaboration kernel）**，不是“全家桶编排平台”。

## 架构摘要

- 核心单位：Working Group
- 事实源：group ledger（append-only）
- 写入模型：daemon 单写者
- 入口设计：Web/CLI/MCP/IM 薄层
- 运行时目录：`CCCC_HOME`（默认 `~/.cccc`）

详见：
- `docs/reference/architecture.md`
- `docs/standards/CCCS_V1.md`
- `docs/standards/CCCC_DAEMON_IPC_V1.md`

## 文档导航

- 新手入口：`docs/guide/getting-started/index.md`
- 场景示例：`docs/guide/use-cases.md`
- 运维手册：`docs/guide/operations.md`
- 产品定位：`docs/reference/positioning.md`
- CLI 参考：`docs/reference/cli.md`
- 功能细节：`docs/reference/features.md`

在线文档：https://dweb-channel.github.io/cccc/

## 安全与运维建议

- Web UI 属高权限入口，远程访问必须设置 `CCCC_WEB_TOKEN`。
- 推荐使用 Cloudflare Access 或 Tailscale，避免裸露公网端口。
- 运行时状态放在 `CCCC_HOME`，不要混入项目仓库。
- 故障排查与恢复步骤见 `docs/guide/operations.md`。

## 从 0.3.x 升级

`0.4.x` 是新架构线，命令与行为有断代变化。

升级前建议：

```bash
pipx uninstall cccc-pair || true
pip uninstall cccc-pair || true
rm -f ~/.local/bin/cccc ~/.local/bin/ccccd
```

然后重新安装并执行 `cccc doctor`。

## 旧版本线路

tmux-first 老版本仓库：
- https://github.com/ChesterRa/cccc-tmux

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
- 丰富 MCP 工具面的控制平面
- Web 优先控制台
- IM 级消息体验

旧版：[cccc-tmux](https://github.com/ChesterRa/cccc-tmux)

</details>

---

## License

Apache-2.0
