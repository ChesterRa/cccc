# CLI Quick Start (日本語)

::: info 日本語訳について
このページは [CLI Quick Start](./cli) の日本語訳です（2026年10月3日時点 / CCCC 0.4.41）。内容が異なる場合は[英語版](./cli)が正となります。
:::

コマンドラインから CCCC を使い始めます。

`--group` を受け付けるコマンドは、次の順で Group を解決します: 明示的なオプション → Actor の `CCCC_GROUP_ID` 環境変数 → `cccc use` で選択されたアクティブ Group。メッセージ送信の送信者は `--by` → `CCCC_ACTOR_ID` → `user` の順に解決されます。そのため、別のセッションがアクティブ Group を切り替えても、Actor は自分の Group を保持します。

## ステップ 1: プロジェクトへ移動

```bash
cd /path/to/your/project
```

## ステップ 2: Working Group を作成

```bash
cccc attach .
```

カレントディレクトリを「スコープ」としてバインドし、Working Group を作成します。

## ステップ 3: Runtime 連携を準備

```bash
cccc setup --runtime claude   # codex, droid, grok, kimi なども指定できます
```

指定した runtime の CCCC MCP 連携を準備または報告します。Claude Code、Codex、Grok Build、OpenCode の managed セッションは、CCCC が起動する際にスコープ付きの MCP エントリを自動的に受け取ります。その他の runtime では、永続設定またはプロンプト支援のセットアップを利用する場合があります。

## ステップ 4: 最初の Agent を追加

```bash
cccc actor add assistant --runtime claude
```

最初に有効化された Actor が、自動的に「foreman」（コーディネーター）になります。

## ステップ 5: Agent を起動

```bash
cccc group start
```

特定の Agent だけを起動することもできます:

```bash
cccc actor start assistant
```

## ステップ 6: メッセージを送信

```bash
cccc send "こんにちは。自己紹介をお願いします。"
```

## ステップ 7: 応答を確認

台帳をリアルタイムで監視します:

```bash
cccc tail -f
```

受信箱を確認することもできます:

```bash
cccc inbox --actor-id assistant
```

## Agent を追加する

2 体目の Agent を追加します:

```bash
cccc actor add reviewer --runtime codex
cccc actor start reviewer
```

特定の Agent に送信します:

```bash
cccc send "この機能を実装してください" --to assistant
cccc send "コードをレビューしてください" --to reviewer
cccc send "次のステップを取りまとめてください" --to @foreman
cccc send "チーム共通の制約です: CI が通るまでデプロイは一時停止" --to @all
```

チャットのコンテキストが切り替わっても作業を継続させたいときは、担当者・完了条件・完了証拠を伴うタスクベースの委任 (tracked-send) を使います:

```bash
cccc tracked-send "機能を実装し、検証の証拠を返信してください。" \
  --to assistant \
  --title "機能の実装" \
  --outcome "実装が完了し、検証の証拠が報告されていること"
```

## メッセージに返信

```bash
# イベント ID は cccc tail で確認できます
cccc reply evt_abc123 "ありがとうございます、問題ありません！"
cccc reply evt_abc123 "急ぎのフォローアップではありません" --mode mail
```

## よく使うコマンド

他のインスタンスを操作するときは、`cccc connect` でアクセス可能な Group を検出してから `cccc send --dst-instance ... --dst-group ...` を使います。返信には受信側のローカルイベント ID を使います。検出・送信・返信の完全なフロー（Direct 接続を含む）は [Agent collaboration](../connect.md#agent-collaboration) を参照してください。

### Group 管理

```bash
cccc groups              # すべての Group を一覧
cccc use <group_id>      # Group を切り替え
cccc active              # アクティブな Group を表示
cccc group show <group_id> # Group のメタデータを表示
cccc group start         # すべての Agent を起動
cccc group stop          # すべての Agent を停止
```

### Actor 管理

```bash
cccc actor list                    # Actor を一覧
cccc actor add <id> --runtime <r>  # Actor を追加
cccc actor start <id>              # Actor を起動
cccc actor stop <id>               # Actor を停止
cccc actor restart <id>            # Actor を再起動
cccc actor remove <id>             # Actor を削除
```

### メッセージ

```bash
cccc send "message"                # --to なし: 既定の宛先ポリシーを適用（既定: foreman）
cccc send "msg" --to assistant     # 特定の Actor へ
cccc send "msg" --to @foreman      # コーディネーターに依頼
cccc send "msg" --to @all          # 明示的な全員ブロードキャスト（既定のタスクディスパッチではありません）
cccc tracked-send "work" --to assistant --title "タスクのタイトル" --outcome "完了の判定基準"
cccc reply <event_id> "response"   # メッセージに返信
cccc inbox --actor-id assistant    # 特定 Actor の未読 Mail を読む
cccc tail -n 50                    # 最近のイベント
cccc tail -f                       # イベントをフォロー
```

### Daemon 操作

```bash
cccc daemon status    # 状態を確認
cccc daemon start     # Daemon を起動
cccc daemon stop      # Daemon を停止
```

## Web UI を起動（任意）

CLI を使っている最中でも、Web UI を開けます:

```bash
cccc   # Daemon + Web UI を起動
```

Daemon がすでに動いている場合は Web UI だけ:

```bash
cccc web
```

http://127.0.0.1:8848/ からアクセスできます。

## 環境変数

| 変数 | 既定値 | 説明 |
|----------|---------|-------------|
| `CCCC_HOME` | `~/.cccc` | ランタイムディレクトリ |
| `CCCC_WEB_PORT` | `8848` | Web UI のポート |
| `CCCC_WEB_READY_TIMEOUT_SECONDS` | `10` | 低速なマシン向けの Web 起動準備タイムアウト |
| `CCCC_LOG_LEVEL` | `INFO` | ログの詳細度 |

## 実行例

```bash
# セットアップ
cd ~/projects/my-app
cccc attach .
cccc setup --runtime claude
cccc actor add dev --runtime claude

# 作業
cccc group start
cccc send "最も小さく安全な認証タスクを計画してください。" --to @foreman
cccc tracked-send "最初の認証タスクを実装し、検証の証拠を返信してください。" \
  --to dev \
  --title "最初の認証スライスの実装" \
  --outcome "実装が完了し、検証の証拠が報告されていること"

# 監視
cccc tail -f

# 対話
cccc reply evt_123 "JWT トークンを使ってください"
cccc send "進捗はどうですか？" --to dev

# 後片付け
cccc group stop
```

## トラブルシューティング

### Daemon が起動しない

```bash
cccc daemon status
cccc daemon stop      # 停止したままのインスタンスがあれば停止
cccc daemon start
```

### Agent が応答しない

```bash
# Agent の状態を確認
cccc actor list

# Agent を再起動
cccc actor restart <actor_id>

# MCP のセットアップを確認
cccc setup --runtime <name>
```

### Group が見つからない

```bash
# すべての Group を一覧
cccc groups

# 必要なら再アタッチ
cd /path/to/project
cccc attach .
```

## 次のステップ

- [Workflows](/guide/workflows) — 協調パターンを学ぶ
- [CLI Reference](/reference/cli) — コマンドの完全なリファレンス
- [IM Bridge](/guide/im-bridge/) — モバイル/リモート運用のセットアップ
