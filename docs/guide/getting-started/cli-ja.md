# CLI クイックスタート

コマンドラインから CCCC を使い始めます。

[English](./cli) | [简体中文](./cli-zh) | 日本語

## 始める前に

[インストールガイド](./#installation)に従って CCCC をインストールしてください。
この手順では Claude Code を使います。Claude Code の CLI をインストールし、
ログインと、必要に応じてプロジェクトの信頼確認を済ませておいてください。
設定済みの[別の対応ランタイム](../runtimes)を使うこともできます。

ターミナルで CCCC を実行できることを確認します。

```bash
cccc --version
cccc doctor
```

`doctor` はインストール状況、検出したランタイム、daemon の状態を表示します。
ランタイムの検出は、プロバイダーへのログイン確認ではありません。
この時点では daemon が停止していても問題ありません。次の手順で起動します。

## ステップ 1: daemon を起動

```bash
cccc daemon start
cccc daemon status
```

互換性のある daemon が起動済みなら、`daemon start` はそれを利用します。
`attach`、`actor`、`send` などのコマンドには、起動中の daemon が必要です。

## ステップ 2: 作業グループを作成

```bash
cd /path/to/your/project
cccc attach .
```

新しい Group を作成し、現在のディレクトリをプロジェクトの作業場所
（「スコープ」）として関連付け、その Group をアクティブにします。
出力には `group_id` が含まれます。既存の Group に戻るには、`cccc groups` と
`cccc use <group_id>` を使ってください。`cccc attach .` を再実行すると、別の Group が作成されます。

## ステップ 3: 最初のエージェントを追加

```bash
cccc actor add assistant --runtime claude
```

最初の有効な Actor が、自動的に「foreman」（調整役）になります。

この Claude Code セッションでは、CCCC が起動時に CCCC MCP 連携を設定するため、
別途 `cccc setup` を実行する必要はありません。他のランタイムでは設定が必要な場合があります。
詳しくは[対応ランタイム](../runtimes)と
[`cccc setup`](/reference/cli#cccc-setup)を参照してください。

## ステップ 4: エージェントを起動

```bash
cccc group start
```

`group start` は Group 内の有効な Actor を起動します。特定の Actor だけを起動する場合は、次を使います。

```bash
cccc actor start assistant
```

`cccc actor list` で状態を確認できます。起動時に対話操作が必要な場合は、
[Web UI](#web-ui-を起動-任意)でその Actor のターミナルを開いてください。

## ステップ 5: メッセージを送信

```bash
cccc send "こんにちは。自己紹介をお願いします。" --to assistant
```

## ステップ 6: 応答を確認

Group に記録されたメッセージやその他のイベントを JSON で追跡します。

```bash
cccc tail -f
```

エージェントからの `chat.message` イベントを確認してください。
ネイティブターミナルの出力は Web UI で確認できます。
**Ctrl+C** で追跡を終了しても、エージェントは動き続けます。

`cccc inbox` は、Actor の未読 Mail を読み取り、**既読として消費する**コマンドです。
応答の確認には `tail` または Web のチャットを使ってください。

## エージェントを追加

Codex のインストールとログインが済んでいれば、2 番目のエージェントを追加できます。

```bash
cccc actor add reviewer --runtime codex
cccc actor start reviewer
```

宛先を指定してメッセージを送信します。

```bash
cccc send "機能を実装してください" --to assistant
cccc send "コードをレビューしてください" --to reviewer
cccc send "次の作業を調整してください" --to "@foreman"
cccc send "チーム全体への指示: CI が通るまでデプロイを保留してください" --to "@all"
```

会話の切り替え後も担当者・成果・検証結果を追跡したい作業には、`tracked-send` を使います。

```bash
cccc tracked-send "機能を実装し、検証結果を添えて返信してください。" \
  --to assistant \
  --title "機能の実装" \
  --outcome "実装が完了し、検証結果が報告されている"
```

この複数行の例は Bash の行継続を使っています。PowerShell では 1 行で入力してください。

## メッセージに返信

`<event_id>` を、`tail` に表示された返信先メッセージの `id` に置き換えます。

```bash
cccc reply <event_id> "ありがとうございます。その内容でお願いします。"
cccc reply <event_id> "急ぎではない補足です" --to assistant --mode mail
```

Mail はエージェント宛てに使い、受信したエージェントを即座に起動・実行させません。

## Group と送信者の選択

`--group` を受け付けるコマンドは、明示的なオプション → `CCCC_GROUP_ID` 環境変数 →
`attach` または `cccc use` で選択したアクティブ Group の順に対象を決定します。
ディレクトリを移動するだけでは Group は切り替わりません。
送信者は `--by` → `CCCC_ACTOR_ID` → `user` の順に決定します。
別のセッションがアクティブ Group を切り替えても、Actor は自分の Group を保持します。

## よく使うコマンド

別のインスタンスに送信する場合は、`cccc connect` でアクセス可能な Group を調べ、
`cccc send --dst-instance ... --dst-group ...` を使います。
返信には、受信したメッセージのローカルイベント ID を使ってください。
Direct 接続を含む手順は[エージェント間の連携](../connect.md#agent-collaboration)を参照してください。

### Group の管理

```bash
cccc groups                # 全 Group を表示
cccc use <group_id>        # 対象の Group を選択
cccc active                # アクティブ Group を表示
cccc group show <group_id> # Group の情報を表示
cccc group start           # この Group の有効なエージェントを起動
cccc group stop            # この Group のエージェントを停止し、履歴は保持
```

### Actor の管理

```bash
cccc actor list                  # Actor を一覧表示
cccc actor add <id> --runtime <r> # Actor を追加
cccc actor start <id>            # Actor を起動
cccc actor stop <id>             # Actor を停止
cccc actor restart <id>          # Actor を再起動
cccc actor remove <id>           # Actor を削除
```

### メッセージ

```bash
cccc send "メッセージ"             # --to なし: 既定の宛先ポリシーを使用（初期値は foreman）
cccc send "メッセージ" --to assistant # 特定の Actor に送信
cccc send "メッセージ" --to "@foreman"  # 調整役に送信
cccc send "メッセージ" --to "@all"      # 明示的な全員送信。通常のタスク割り当てとは別
cccc tracked-send "作業内容" --to assistant --title "タスク名" --outcome "完了条件"
cccc reply <event_id> "応答"       # メッセージに返信
cccc inbox --actor-id assistant   # Actor の未読 Mail を読み取り、既読として消費
cccc tail -n 50                   # 最近のイベントを表示
cccc tail -f                      # 新しいイベントを追跡
```

### daemon の操作

```bash
cccc daemon status # 状態を確認
cccc daemon start  # 起動
cccc daemon stop   # daemon と、各 Group の管理対象プロセスを停止
```

## Web UI を起動（任意） {#web-ui-を起動-任意}

別のターミナルで Web UI を起動します。

```bash
cccc web
```

サブコマンドなしの `cccc` も同じ動作です。どちらも起動済みの daemon を利用し、
必要なら起動します。既定の設定では http://127.0.0.1:8848/ を開いてください。
アドレスやポートを変更している場合は、起動時に表示される URL を使います。
別のポートを指定するには `cccc web --port 9000` を使います。

## 環境変数

| 変数 | 既定値 | 説明 |
|------|--------|------|
| `CCCC_HOME` | `~/.cccc` | ランタイムデータの保存先 |
| `CCCC_WEB_PORT` | `8848` | 保存済みの設定や `--port` が優先されない場合の Web UI ポート |

同じ Group やランタイム状態を使うコマンドでは、同じ `CCCC_HOME` を指定してください。
Web のアドレス・ポート設定と優先順位は[CLI リファレンス](/reference/cli#environment-variables)を参照してください。

## 作業を終える

**Ctrl+C** で `tail -f` を終了した後、現在の Group のエージェントを停止します。

```bash
cccc group stop
```

Group と記録済みの履歴は残ります。`cccc group start` で Actor を再び起動できます。
他の Group の管理対象プロセスも含めてインスタンス全体を停止する場合は、`cccc daemon stop` を使います。

## トラブルシューティング

### daemon が起動しない

```bash
cccc daemon status
cccc doctor
```

停止している場合は `cccc daemon start` を実行してください。
応答しない場合は、表示されたエラーを確認してから停止・再起動を判断してください。
daemon の再起動は、各 Group の管理対象プロセスを中断します。

### エージェントが応答しない

```bash
# Actor の状態を確認
cccc actor list

# 記録されたエラーやメッセージを確認
cccc tail -n 50
```

Web UI の Actor ターミナルで、ログイン・信頼確認・承認待ちを確認してください。
[ランタイム連携](../runtimes)も確認します。原因を解消した後、
`cccc actor start <actor_id>` で起動するか、必要なら
`cccc actor restart <actor_id>` で再起動してください。

### Group が見つからない

```bash
# 全 Group を表示
cccc groups

# 既存の Group を選択
cccc use <group_id>
cccc active
```

ターミナルが想定どおりの `CCCC_HOME` を使っているか確認してください。
`attach` は、新しい Group を作る場合、または対象を明示して既存の Group にスコープを追加する場合に使います。

## 次のステップ

以下の関連ガイドは英語です。

- [ワークフロー](/guide/workflows) — 連携の進め方
- [CLI リファレンス](/reference/cli) — コマンドの詳細
- [IM ブリッジ](/guide/im-bridge/) — モバイルからのアクセス
