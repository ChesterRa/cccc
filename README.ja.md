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

Cargo からビルド済み Rust CLI を起動します。`CCCC_HOME` を使用し、既定値は `~/.cccc` です。

```bash
cargo run --release -p cccc-cli --bin cccc
```

サーバーの bind が成功すると、Web のアクセス先と port がターミナルに表示されます。

`--` より後の引数は CCCC に渡されます。例：

```bash
cargo run --release -p cccc-cli --bin cccc -- doctor
cargo run --release -p cccc-cli --bin cccc -- groups
```

ソースビルドには Rust 1.88+ が必要です。Web UI のビルドには Node.js 20+ が必要です。リリース archive の実行に Python や Node.js は不要です。

## クイックスタート

```bash
cd /path/to/project
cccc daemon start
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

## データ互換性

Python と Rust は同じ `CCCC_HOME` を使用します。

```text
CCCC_HOME=${HOME}/.cccc
```

既定値は `~/.cccc` です。Rust は既存の registry、group、非圧縮または gzip 圧縮 ledger、state、および新旧両方の Python access token document をそのまま読み込みます。Rust compact は Python 互換の segment 名と manifest を生成します。初回起動時には `.cccc-rust-v1` 互換 marker のみを追加し、既存データを移動または削除しません。新しい実装を初めて使う前に `CCCC_HOME` をバックアップしてください。

実装は branch で切り替えます。

```bash
git switch python  # 旧 Python 実装と ~/.cccc
git switch rust    # Rust 実装と同じ ~/.cccc
```

branch を切り替える前に現在の daemon を停止してください。Python と Rust の daemon を同じ Home に対して同時に実行しないでください。

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
                  CCCC_HOME only
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

container の Rust state は `CCCC_HOME=/data` に保存されます。

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
