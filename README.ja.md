# CCCC — マルチエージェント協調カーネル

[English](README.md) | [中文](README.zh-CN.md) | **日本語**

> **ステータス**: 0.4.x Release Candidate ライン

[![Documentation](https://img.shields.io/badge/docs-online-blue)](https://dweb-channel.github.io/cccc/)
[![License](https://img.shields.io/badge/license-Apache--2.0-green)](LICENSE)

CCCC は **local-first なマルチエージェント協調カーネル**です。モダンな IM のように AI エージェントを協調させます。

**主な機能**：
- 🤖 **マルチランタイム対応** — Claude Code、Codex CLI、Droid、OpenCode、Copilot など
- 📝 **追記型 ledger** — 永続的な履歴、唯一の事実源
- 🌐 **Web ファーストコンソール** — モバイルフレンドリー
- 💬 **IM グレードのメッセージング** — @mentions、reply/quote、既読確認
- 🔧 **MCP ツール面** — 豊富なコントロールプレーンツールで信頼性の高いエージェント操作
- 🔌 **IM ブリッジ** — Telegram、Slack、Discord、Feishu、DingTalk

![CCCC Chat UI](screenshots/chat.png)

---

## クイックスタート

```bash
# インストール
python -m pip install -U cccc-pair

# 起動
cccc
```

`http://127.0.0.1:8848/` を開いて Web UI にアクセス。

---

## ドキュメント

📚 **[ドキュメントを読む](https://dweb-channel.github.io/cccc/)** — 完全なガイド、リファレンス、API ドキュメント。

---

## インストール

### AI アシスタントでインストール

以下のプロンプトを AI アシスタント（Claude、ChatGPT など）にコピーしてください：

> CCCC（Claude Code Collaboration Context）マルチエージェント協調システムのインストールと起動を手伝ってください。
>
> 手順：
>
> 1. cccc-pair をインストール：
>    ```
>    python -m pip install -U cccc-pair
>    ```
>
> 2. インストール後、CCCC を起動：
>    ```
>    cccc
>    ```
>
> 3. アクセス URL を教えてください（通常は http://localhost:8848/ui/）
>
> エラーが発生した場合は、診断と解決を手伝ってください。

### 旧バージョンからのアップグレード

古いバージョンの cccc-pair（例：0.3.x）がインストールされている場合は、先にアンインストールが必要です：

```bash
# pipx ユーザー
pipx uninstall cccc-pair

# pip ユーザー
pip uninstall cccc-pair

# 残留ファイルがある場合は手動で削除
rm -f ~/.local/bin/cccc ~/.local/bin/ccccd
```

> **注意**：0.4.x のコマンド構造は 0.3.x と完全に異なります。旧版の `init`、`run`、`bridge` コマンドは `attach`、`daemon`、`mcp` などに置き換えられました。

### TestPyPI からインストール（RC 検証時）

```bash
python -m pip install --index-url https://pypi.org/simple \
  --extra-index-url https://test.pypi.org/simple \
  cccc-pair==0.4.0rc20
```

### ソースからインストール

```bash
git clone https://github.com/ChesterRa/cccc
cd cccc
pip install -e .
```

### uv を使用（Windows 推奨）

```bash
uv venv -p 3.11 .venv
uv pip install -e .
uv run cccc --help
```

**要件**: Python 3.9+、macOS / Linux / Windows

---

## コア概念

| 概念 | 説明 |
|------|------|
| **Working Group** | 永続履歴を持つ協調単位（グループチャットのようなもの） |
| **Actor** | エージェントセッション（PTY または headless） |
| **Scope** | グループに紐づくディレクトリ |
| **Ledger** | 追記型イベントストリーム |
| **CCCC_HOME** | ランタイムホーム、デフォルト `~/.cccc/` |

---

## ランタイムと MCP

CCCC は複数のエージェントランタイムをサポート：

```bash
cccc runtime list --all     # 利用可能なランタイムを表示
cccc setup --runtime <name> # MCP を設定
```

**MCP 自動設定**: `claude`、`codex`、`droid`、`amp`、`auggie`、`neovate`、`gemini`
**手動設定**: `cursor`、`kilocode`、`opencode`、`copilot`、`custom`

---

## マルチエージェント設定

プロジェクトでマルチエージェント協調を設定：

```bash
# プロジェクトディレクトリに紐付け
cd /path/to/repo
cccc attach .

# ランタイムの MCP を設定
cccc setup --runtime claude

# actors を追加（最初の enabled が foreman に）
cccc actor add foreman --runtime claude
cccc actor add peer-1  --runtime codex

# グループを起動
cccc group start
```

---

## Web UI

内蔵 Web UI の機能：

- マルチグループナビゲーション
- Actor 管理（add/start/stop/restart）
- Chat（@mentions + reply）
- actor ごとの埋め込みターミナル
- Context と自動化設定
- IM Bridge 設定

---

## IM ブリッジ

Working Group を IM プラットフォームにブリッジ：

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
```

対応: **Telegram** | **Slack** | **Discord** | **Feishu/Lark** | **DingTalk**

---

## CLI チートシート

```bash
cccc doctor              # 環境チェック
cccc groups              # グループ一覧
cccc use <group_id>      # グループ切り替え
cccc send "msg" --to @all
cccc inbox --mark-read
cccc tail -n 50 -f
cccc daemon status|start|stop
```

---

## PROJECT.md

リポジトリルートに `PROJECT.md` を配置し、プロジェクト憲法として扱います。エージェントは `cccc_project_info` MCP ツールで読み取ります。

---

## セキュリティ

Web UI は高権限です。リモートアクセス時：
- `CCCC_WEB_TOKEN` 環境変数を設定
- アクセスゲートウェイを使用（Cloudflare Access、Tailscale、WireGuard）

---

## なぜリライト？

<details>
<summary>歴史: v0.3.x → v0.4.x</summary>

v0.3.x（tmux-first）は概念を証明しましたが、限界に直面：

1. **統一 ledger がない** — メッセージが複数ファイルに分散、レイテンシ増加
2. **actor 数の制約** — tmux レイアウトは 1–2 actors に制限
3. **エージェント制御能力の弱さ** — 自律性が制限
4. **リモートアクセスが一級体験でない** — Web コントロールプレーンが必要

v0.4.x の導入：
- 統一された追記型 ledger
- N-actor モデル
- 豊富な MCP ツール面のコントロールプレーン
- Web ファーストコンソール
- IM グレードのメッセージング

旧版: [cccc-tmux](https://github.com/ChesterRa/cccc-tmux)

</details>

---

## License

Apache-2.0
