<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="web/public/logo-dark.svg">
  <img src="web/public/logo.svg" width="160" alt="CCCC logo" />
</picture>

# CCCC

用一个持久化 Rust 控制平面协调多个编码 Agent。

**中文** | [English](README.md) | [日本語](README.ja.md)

</div>

CCCC 由 Rust daemon、CLI、MCP Server、Web API、终端运行时和现有 React/TypeScript 前端组成。协作组共享追加写账本、结构化上下文、已读游标、附件、记忆、能力包和 Actor 生命周期状态。

## 安装

从 GitHub Releases 下载对应平台归档，将以下二进制文件放入 `PATH`：

- `cccc`
- `ccccd`
- `cccc-mcp`
- `cccc-web`

从源码构建：

```bash
npm ci --prefix web
npm -C web run build
cargo build --workspace --release --locked
```

通过 Cargo 启动编译后的 Rust CLI。它使用 `CCCC_HOME`，默认目录是 `~/.cccc`：

```bash
cargo run --release -p cccc-cli --bin cccc
```

服务绑定成功后，终端会打印 Web 访问地址和端口。

`--` 后面的参数会传给 CCCC，例如：

```bash
cargo run --release -p cccc-cli --bin cccc -- doctor
cargo run --release -p cccc-cli --bin cccc -- groups
```

源码构建需要 Rust 1.88+；构建 Web UI 需要 Node.js 20+。运行发布归档不需要 Python 或 Node.js。

## 快速开始

```bash
cd /path/to/project
cccc daemon start
cccc group create --title "我的团队"
cccc groups
cccc group use <group_id> .
cccc actor add foreman --runtime claude
cccc actor add implementer --runtime codex
cccc group start
cccc send "检查仓库并报告第一个具体任务。" --to foreman
cccc
```

打开 <http://127.0.0.1:8848>。`cccc setup` 会输出当前 Rust 安装对应的 MCP Server 配置。

## 数据兼容

Python 和 Rust 使用同一个 `CCCC_HOME`：

```text
CCCC_HOME=${HOME}/.cccc
```

默认目录是 `~/.cccc`。Rust 会原地读取已有 registry、工作组、未压缩或 gzip 压缩的 ledger、状态数据，以及新旧两种 Python 访问令牌文档。Rust compact 生成 Python 可识别的段文件和 manifest。首次启动只会增加 `.cccc-rust-v1` 兼容标记，不会移动或删除已有数据。首次使用新实现前请备份 `CCCC_HOME`。

通过分支切换实现：

```bash
git switch python  # 旧 Python 实现，使用 ~/.cccc
git switch rust    # Rust 实现，使用同一个 ~/.cccc
```

切换分支前先停止当前 daemon，禁止 Python 和 Rust daemon 同时操作这个共享目录。

## 主要命令

```bash
cccc --help
cccc daemon start|stop|status|run
cccc group create|show|update|start|stop|use
cccc actor list|add|update|start|stop|restart|secrets
cccc prompt <actor_id>
cccc send|tracked-send|reply|inbox|read|tail|ledger
cccc im set|config|start|stop|status|bind|pending|authorized|reject|revoke
cccc space status|bind|unbind|sync|ingest|query|sources|jobs|auth
cccc runtime list
cccc doctor
cccc mcp
cccc web
```

具体参数以 `cccc <command> --help` 为准。

## 架构

```text
React/TypeScript UI     CLI     MCP     远程连接器
          \              |       |          /
                    Rust Web/API
                         |
                    版本化 daemon IPC
                         |
                      Rust daemon
          group 状态 / ledger / runtime / memory
                         |
                  仅 CCCC_HOME
```

daemon 是状态写入者。CLI、Web API 和 MCP 复用同一组操作，不各自维护状态。

Workspace crates：

```text
cccc-contracts  IPC 与事件契约
cccc-core       group、ledger、scope、memory、策略、Rust Home
cccc-runtime    PTY 与 headless 进程会话
cccc-client     daemon IPC 客户端
cccc-daemon     状态操作与运行时生命周期
cccc-mcp        MCP 目录、本地工具与 daemon 映射
cccc-web        HTTP、WebSocket、浏览器和内嵌 Web UI
cccc-cli        用户命令 cccc
```

## 集成能力

- Web Model 连接器把远程 MCP 严格绑定到一个 group 和 actor；Rust Home 下的 Chromium profile 会保留登录态。
- Group Bridge 支持双端配对、作用域凭据、幂等消息与附件、投递回执、WebSocket 会话和按访问级别过滤的远程 MCP。
- Group Space 提供 work/memory lane、幂等 ingest、source、job、本地降级检索以及可选的 NotebookLM 浏览器登录面。
- Voice Secretary 支持文档、lease、session 和 Browser ASR 转写；未配置本地转写后端时明确返回 `asr_unavailable`。
- IM 配置与授权状态覆盖 Telegram、Slack、Discord、飞书、钉钉、企业微信和微信。外部网络适配器未真实可用时，Rust 包不会伪报运行成功。

## Docker

```bash
docker volume create cccc-data
docker compose -f docker/docker-compose.yml up --build
```

容器通过 `CCCC_HOME=/data` 保存 Rust 状态，默认只把 Web UI 发布到本机地址。

## 验证

```bash
scripts/pre_commit_checks.sh
scripts/build_package.sh
docker build -f docker/Dockerfile .
```

标准门禁包含 Web lint/typecheck/build、Rust format、Clippy 和 workspace tests。

## 文档

- [开始使用](docs/guide/getting-started/index.md)
- [CLI 参考](docs/reference/cli.md)
- [架构](docs/reference/architecture.md)
- [运维](docs/guide/operations.md)
- [Rust 迁移与兼容](docs/rust-migration.md)

## 许可证

Apache-2.0，见 [LICENSE](LICENSE)。
