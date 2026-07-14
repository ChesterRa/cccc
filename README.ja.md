<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="web/public/logo-dark.svg">
  <img src="web/public/logo.svg" width="160" alt="CCCC logo" />
</picture>

# CCCC

永続的な Rust コントロールプレーンで複数のコーディング Agent を連携します。

[中文](README.zh-CN.md) | [English](README.md) | **日本語**

</div>

CCCC は Rust daemon、CLI、MCP Server、Web API、ターミナル runtime、および既存の React/TypeScript UI で構成されます。グループは追記専用 ledger、構造化 context、既読 cursor、添付、memory、capability、Actor lifecycle を共有します。

## インストール

GitHub Releases から対象 platform の archive を取得し、次の binary を `PATH` に配置します。

- `cccc`
- `ccccd`
- `cccc-mcp`
- `cccc-web`

ソースからビルドする場合：

```bash
npm ci --prefix web
npm -C web run build
cargo build --workspace --release --locked
```

ソースビルドには Rust 1.88+ が必要です。Web UI のビルドには Node.js 20+ が必要です。リリース archive の実行に Python や Node.js は不要です。

## クイックスタート

```bash
cd /path/to/project
cccc group create --title "My team"
cccc groups
cccc group use <group_id> .
cccc actor add foreman --runtime claude
cccc actor add implementer --runtime codex
cccc group start
cccc send "リポジトリを確認し、最初の具体的なタスクを報告してください。" --to foreman
cccc
```

<http://127.0.0.1:8848> を開きます。`cccc setup` は現在の Rust インストール用 MCP Server 設定を出力します。

## データ分離

Rust は `CCCC_RUST_HOME` のみを使用します。

```text
CCCC_RUST_HOME=${HOME}/.cccc-rust
```

既定値は `~/.cccc-rust` です。CCCC は `~/.cccc` とその配下を拒否します。Rust Home には `.cccc-rust-v1` marker が必要で、marker のない非空 custom directory も拒否されます。旧データは自動的に読み込み、変更、移行されません。

実装は branch で切り替えます。

```bash
git switch python  # 旧 Python 実装と ~/.cccc
git switch rust    # Rust 実装と ~/.cccc-rust
```

両方の実装を同じ directory に向けないでください。

## 主なコマンド

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

正確な引数は `cccc <command> --help` を参照してください。

## アーキテクチャ

```text
React/TypeScript UI     CLI     MCP     remote connector
          \              |       |             /
                    Rust Web/API
                         |
                    versioned daemon IPC
                         |
                      Rust daemon
          group state / ledger / runtime / memory
                         |
                  CCCC_RUST_HOME only
```

daemon が状態を書き込みます。CLI、Web API、MCP は同じ操作を呼び出し、独自状態を持ちません。

## 統合

- Web Model connector は remote MCP を単一の group と actor に固定します。
- Group Bridge は pairing、scoped credential、idempotent message/attachment、receipt、WebSocket、access-filtered remote MCP を提供します。
- Group Space は work/memory lane、ingest、source、job、local fallback search、NotebookLM browser login surface を提供します。
- Voice Secretary は document、lease、session、Browser ASR を提供し、local backend がない場合は `asr_unavailable` を返します。
- IM の設定と認可状態は Telegram、Slack、Discord、Feishu、DingTalk、WeCom、Weixin を扱います。外部 adapter が利用できない場合、実行中とは報告しません。

## Docker

```bash
docker volume create cccc-data
docker compose -f docker/docker-compose.yml up --build
```

container の Rust state は `CCCC_RUST_HOME=/data` に保存されます。

## 検証

```bash
scripts/pre_commit_checks.sh
scripts/build_package.sh
docker build -f docker/Dockerfile .
```

標準 gate は Web lint/typecheck/build、Rust format、Clippy、workspace tests を実行します。

## ドキュメント

- [Getting started](docs/guide/getting-started/index.md)
- [CLI reference](docs/reference/cli.md)
- [Architecture](docs/reference/architecture.md)
- [Operations](docs/guide/operations.md)
- [Rust migration](docs/rust-migration.md)

## License

Apache-2.0。詳細は [LICENSE](LICENSE) を参照してください。
