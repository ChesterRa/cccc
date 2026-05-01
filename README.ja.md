<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="web/public/logo-dark.svg">
  <img src="web/public/logo.svg" width="160" alt="CCCC logo" />
</picture>

# CCCC

### ローカルファースト・マルチエージェント協調カーネル

**軽量でありながら、インフラ級の信頼性を備えたマルチエージェントフレームワーク。**

チャットネイティブ、プロンプト駆動、双方向オーケストレーションを前提に設計。

複数のコーディングエージェントを**永続的で協調されたシステム**として運用 — バラバラのターミナルセッションではなく。

3 コマンドで開始。ゼロインフラ、プロダクション級のパワー。

[![PyPI](https://img.shields.io/pypi/v/cccc-pair?label=PyPI&color=blue)](https://pypi.org/project/cccc-pair/)
[![Python](https://img.shields.io/pypi/pyversions/cccc-pair)](https://pypi.org/project/cccc-pair/)
[![License](https://img.shields.io/badge/license-Apache--2.0-green)](LICENSE)
[![Docs](https://img.shields.io/badge/docs-online-blue)](https://chesterra.github.io/cccc/)

[English](README.md) | [中文](README.zh-CN.md) | **日本語**

</div>

---

## なぜ CCCC か

- **永続協調**: 作業状態はターミナルスクロールではなく、append-only ledger に残ります。
- **到達の可視化**: メッセージはルーティング、既読、ACK、reply-required 追跡を持ち、「送ったはず」で終わりません。
- **1 つのコントロールプレーン**: Web UI、CLI、MCP、IM ブリッジがすべて同じ daemon 状態を共有します。
- **マルチランタイム前提**: Claude Code、Codex CLI、ChatGPT Web、Gemini CLI などの主要ランタイムを 1 つのグループで混在運用できます。
- **ローカルファースト運用**: `pip install` ひとつで始められ、ランタイム状態は `CCCC_HOME` に置いたまま、必要時だけリモート監視へ広げられます。

## 課題

複数のコーディングエージェントを使う現実：

- **コンテキストの喪失** — 協調記録はターミナルのスクロールバッファに埋もれ、再起動で消える
- **到達保証なし** — エージェントがメッセージを*読んだ*かどうか確認できない
- **運用の断片化** — 起動/停止/復旧/エスカレーションがツールごとに分散
- **リモートアクセス不可** — 長時間稼働中のグループを外出先から確認できない

これらは些細な問題ではありません。マルチエージェント環境が「脆いデモ」から「信頼できるワークフロー」に進化できない根本原因です。

## CCCC の役割

CCCC は `pip install` 一つで導入完了、外部依存ゼロ — データベース不要、メッセージブローカー不要、Docker 必須ではありません。それでいて、壊れやすいマルチエージェント構成に足りない運用基盤を提供します：

| 機能 | 実現方法 |
|---|---|
| **唯一の事実源** | append-only ledger（`ledger.jsonl`）が全メッセージ・イベントを記録 — 再生可能、監査可能、喪失なし |
| **信頼性のあるメッセージング** | 既読カーソル、attention ACK、reply-required 義務追跡 — 誰が何を確認したか明確 |
| **統一コントロールプレーン** | Web UI、CLI、MCP ツール、IM ブリッジがすべて 1 つの daemon に接続 — 状態の分断なし |
| **マルチランタイム編成** | Claude Code、Codex CLI、ChatGPT Web、Gemini CLI など 9 種の主要ランタイムを混在利用でき、さらに `custom` も扱える |
| **ロールベース協調** | Foreman + Peer ロールモデル、権限境界と宛先ルーティング（`@all`、`@peers`、`@foreman`） |
| **ローカルファーストなランタイム状態** | ランタイムデータはリポジトリではなく `CCCC_HOME` に保持しつつ、Web Access と IM ブリッジで遠隔運用も可能 |

## CCCC の見た目
 
<div align="center">
 
<video src="https://github.com/user-attachments/assets/8f9c3986-f1ba-4e59-a114-bcb383ff49a7" controls="controls" muted="muted" autoplay="autoplay" loop="loop" style="max-width: 100%;">
</video>
 
</div>

## クイックスタート

### インストール

```bash
# 安定チャネル（PyPI）
pip install -U cccc-pair

# RC チャネル（TestPyPI）
pip install -U --pre \
  --index-url https://test.pypi.org/simple/ \
  --extra-index-url https://pypi.org/simple/ \
  cccc-pair
```

> **要件**: Python 3.9+、macOS / Linux / Windows

### アップグレード

```bash
cccc update
```

インストール種別と実行予定のコマンドを事前確認するには `cccc update --check` を使用してください。

### 起動

```bash
cccc
```

**http://127.0.0.1:8848** を開く — デフォルトで daemon とローカル Web UI が一緒に起動します。

### マルチエージェントグループの作成

```bash
cd /path/to/your/repo
cccc attach .                              # ディレクトリを scope として紐付け
cccc setup --runtime claude                # ランタイムの MCP を設定
cccc actor add foreman --runtime claude    # 最初の actor が foreman に
cccc actor add implementer --runtime codex # peer を追加
cccc group start                           # 全 actor を起動
cccc send "リポジトリを確認し、最初の安全なタスクを提案してください。" --to foreman
cccc tracked-send "最初の具体タスクを担当し、検証証拠を添えて返信してください。" \
  --to implementer \
  --title "最初の具体タスク" \
  --outcome "変更内容と検証証拠が報告されている"
```

これで 2 つのエージェントが永続グループ内で協調し、完全なメッセージ履歴、到達追跡、Web ダッシュボードを備えた状態になります。配信と協調は daemon が担い、ランタイム状態はリポジトリではなく `CCCC_HOME` に残ります。

## プログラマブル連携（SDK）

外部アプリやサービスから CCCC を連携する場合は、公式 SDK を利用してください:

```bash
pip install -U cccc-sdk
npm install cccc-sdk
```

SDK には daemon は含まれません。実行中の `cccc` 本体に接続して利用します。

## アーキテクチャ

```mermaid
graph TB
    subgraph Agents["エージェントランタイム"]
        direction LR
        A1["Claude Code"]
        A2["Codex CLI"]
        A3["ChatGPT Web<br/>GPT-5.x/GPT-5.x Pro"]
        A4["Gemini CLI"]
        A5["+ 5 種 + custom"]
    end

    subgraph Daemon["CCCC Daemon · 単一ライター"]
        direction LR
        Ledger[("Ledger<br/>append-only JSONL")]
        ActorMgr["Actor<br/>マネージャ"]
        Auto["オートメーション<br/>ルール · 催促 · Cron"]
        Ledger ~~~ ActorMgr ~~~ Auto
    end

    subgraph Ports["コントロールプレーン"]
        direction LR
        Web["Web UI<br/>:8848"]
        CLI["CLI"]
        MCP["MCP<br/>(stdio)"]
    end

    subgraph IM["IM ブリッジ"]
        direction LR
        TG["Telegram"]
        SL["Slack"]
        DC["Discord"]
        FS["Feishu"]
        DT["DingTalk"]
        WC["WeCom"]
        WX["Weixin"]
    end

    A1 <-->|MCP ツール<br/>PTY/headless| Daemon
    A2 <-->|MCP ツール<br/>PTY/headless| Daemon
    A3 <-->|ブラウザ配信<br/>Remote MCP| Daemon
    A4 <-->|MCP ツール| Daemon
    A5 <-->|MCP ツール| Daemon
    Daemon <--> Ports
    Web <--> IM

```

**設計上の重要な決定：**

- **Daemon は単一ライター** — すべての状態変更が 1 つのプロセスを経由し、競合状態を排除
- **Ledger は append-only** — イベントは不変、履歴は信頼性が高くデバッグ可能
- **ポートは薄い** — Web、CLI、MCP、IM ブリッジはステートレスなフロントエンド；daemon が全真実を保持
- **ランタイムホーム `CCCC_HOME`**（デフォルト `~/.cccc/`）— ランタイム状態はリポジトリの外に保持

## サポートランタイム

CCCC は 9 種の主要ランタイムでエージェントを編成し、残りは `custom` で扱えます。同一グループ内で各 actor が異なるランタイムを使用可能です。

| ランタイム | 連携方式 | コマンド / サーフェス |
|-----------|----------|------------------------|
| Claude Code | MCP 自動設定 | `claude` |
| Codex CLI | MCP 自動設定 | `codex` |
| ChatGPT Web | Remote MCP + ブラウザ配信 | `chatgpt.com` conversation |
| Gemini CLI | MCP 自動設定 | `gemini` |
| Droid | MCP 自動設定 | `droid` |
| Amp | MCP 自動設定 | `amp` |
| Auggie | MCP 自動設定 | `auggie` |
| Kimi CLI | MCP 自動設定 | `kimi` |
| Neovate | MCP 自動設定 | `neovate` |
| Custom | 手動設定 | 任意のコマンド |

```bash
cccc setup --runtime claude    # ランタイムの MCP を自動設定
cccc runtime list --all        # 利用可能なランタイムを表示
cccc doctor                    # 環境とランタイムの可用性を検証
```

Actor は **PTY**（埋め込みターミナル）または **headless**（ターミナルなしの構造化 I/O）モードで実行できます。Claude Code と Codex CLI は両モードに対応。headless モードでは daemon が配信とストリーミングをより精密に制御します。

### ChatGPT Web / GPT-5.x/GPT-5.x Pro ローカル開発

ChatGPT Web は外部チャットウィンドウではなく、実際の CCCC actor としてグループに参加できます。CCCC はブラウザ配信で明示的に紐付けた 1 つの ChatGPT 会話へグループメッセージを届け、ChatGPT はその actor に紐付いた単一の Remote MCP connector 経由で CCCC に接続します。

つまり **GPT-5.x/GPT-5.x Pro** を選択した ChatGPT 会話が、Claude Code や Codex と同じ協調レイヤーでローカル開発に参加できます。ルーティングされたメッセージの受信、CCCC 経由の可視返信、リポジトリの確認/編集、scope 内の shell/git 実行、peer agent との協調が可能です。

最短セットアップ:

1. `cccc web` を public HTTPS URL または tunnel で公開します。
2. `Settings > Global > Web Access` で Admin Access Token を作成します。
3. runtime が `ChatGPT Web Model` の actor を追加します。
4. `Settings > Global > ChatGPT Web Model` でその actor の単一 MCP URL を作成/コピーします。
5. ChatGPT Developer mode で custom MCP connector を作成し、その URL を貼り付け、`No Auth` を選択します。
6. 特定の `https://chatgpt.com/c/...` 会話を actor に紐付け、その ChatGPT 会話内で GPT-5.x/GPT-5.x Pro を選択します。

詳細な設定とトラブルシュート: [ChatGPT Web Model Runtime](https://chesterra.github.io/cccc/guide/web-model-runtime)。

## メッセージングと協調

CCCC は IM グレードのメッセージングセマンティクスを実装 — 「ターミナルにテキストを貼り付ける」だけではありません：

- **宛先ルーティング** — `@all`、`@peers`、`@foreman`、または特定の actor ID
- **既読カーソル** — 各エージェントが MCP 経由で明示的に既読をマーク
- **返信と引用** — 構造化された `reply_to` + 引用コンテキスト
- **Attention ACK** — 優先メッセージは明示的な確認が必要
- **Reply-required 義務** — 受信者が返信するまで追跡
- **自動ウェイク** — メッセージ受信時、無効化された agent を自動起動

通常の `send` はチャット、質問、軽い依頼に使います。明確な担当者、完了条件、証拠、引き継ぎ、受け入れ履歴が必要な委任作業には `tracked-send` を使ってください。`@all` は告知や緊急の共有制約には使えますが、具体タスクのデフォルト分配先にはしません。

メッセージは daemon が管理する配信パイプラインを通じて各 actor ランタイムへ届けられ、daemon が全メッセージの到達状態を追跡します。

## オートメーションとポリシー

内蔵ルールエンジンが運用面の懸念を処理し、手動監視を不要に：

| ポリシー | 機能 |
|----------|------|
| **催促（Nudge）** | 設定可能なタイムアウト後に未読メッセージを agent にリマインド |
| **Reply-required フォローアップ** | 必須返信が遅延した場合にエスカレート |
| **Actor アイドル検出** | agent が沈黙した際に foreman に通知 |
| **Keepalive** | foreman への定期的なチェックインリマインダー |
| **沈黙検出** | グループ全体が静かになった場合にアラート |

内蔵ポリシーに加え、カスタムオートメーションルールを作成可能：

- **インターバルトリガー** — 「N 分ごとにスタンドアップリマインダーを送信」
- **Cron スケジュール** — 「平日毎朝 9 時にステータスチェックを投稿」
- **ワンタイムトリガー** — 「今日 17 時にグループを一時停止」
- **運用アクション** — グループ状態の設定や actor ライフサイクルの制御（管理者のみ、ワンタイムのみ）

## Web UI

内蔵 Web UI `http://127.0.0.1:8848` の機能：

- **チャットビュー** — `@mention` オートコンプリートとリプライスレッド
- **actor ごとの埋め込みターミナル**（xterm.js）— 各 agent の作業状況をリアルタイムで確認
- **グループ & actor 管理** — 作成、設定、起動、停止、再起動
- **オートメーションルールエディター** — トリガー、スケジュール、アクションを視覚的に設定
- **Context パネル** — 共有ビジョン、スケッチ、マイルストーン、タスク
- **Group Space** — NotebookLM 統合による共有ナレッジ管理
- **ChatGPT Web Model 設定** — 1 つの ChatGPT Web 会話を CCCC actor として接続
- **IM ブリッジ設定** — Telegram/Slack/Discord/Feishu/DingTalk/WeCom/Weixin に接続
- **設定** — メッセージングポリシー、配信チューニング、ターミナルトランスクリプト制御
- **テキストスケール** — 90% / 100% / 125% フォントサイズ、ブラウザごとに永続化
- **ライト / ダーク / システムテーマ**

| チャット | ターミナル |
|:--------:|:----------:|
| ![Chat](screenshots/chat.png) | ![Terminal](screenshots/terminal.png) |

### リモートアクセス

localhost 外から Web UI にアクセスする場合：

- **LAN / プライベートネットワーク** — 全ローカルインターフェースにバインド：`CCCC_WEB_HOST=0.0.0.0 cccc`
- **Cloudflare Tunnel**（推奨）— `cloudflared tunnel --url http://127.0.0.1:8848`
- **Tailscale** — tailnet IP にバインド：`CCCC_WEB_HOST=$TAILSCALE_IP cccc`
- ローカル以外へ公開する前に、まず **Settings > Web Access** で **Admin Access Token** を作成し、その完了まではネットワーク境界で保護してください。
- **Settings > Web Access** で `127.0.0.1` はローカルのみ、`0.0.0.0` は localhost + LAN IP を意味します。CCCC が WSL2 のデフォルト NAT ネットワーク内で動作している場合、`0.0.0.0` は WSL 内部にのみ公開されます。LAN デバイスからのアクセスには WSL mirrored networking または Windows portproxy/ファイアウォールルールが必要です。
- `Save` はターゲットバインディングを保存します。Web が `cccc` または `cccc web` で起動された場合は、**Settings > Web Access** の `Apply now` で短い監視付き再起動を実行してください。Docker、systemd 等の外部スーパーバイザが管理している場合は、そのサービスを再起動してください。
- `Start` / `Stop` は Tailscale リモートアクセス専用で、既に稼働中の Web ソケットのリバインドは行いません。
- トークンポリシーは意図的に階層化されています：localhost のみの場合はシンプルに、LAN/プライベート公開ではデフォルトで Access Token が必要、公開 URL/トンネル公開では Access Token が必須です。

## IM ブリッジ

Working Group を IM プラットフォームにブリッジ：

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
```

| プラットフォーム | ステータス |
|-----------------|-----------|
| Telegram | ✅ 対応済み |
| Slack | ✅ 対応済み |
| Discord | ✅ 対応済み |
| Feishu / Lark | ✅ 対応済み |
| DingTalk | ✅ 対応済み |
| WeCom / 企業微信 | ✅ 対応済み |
| Weixin / 微信 | ✅ 対応済み |

> DingTalk と WeCom はストリーミング返信に対応（それぞれ AI Card と aibot ストリーミング）。他のプラットフォームは最終メッセージを配信。

任意の対応プラットフォームから、通常の調整にはプレーンテキストまたは `/send @foreman <メッセージ>` を使い、真のブロードキャストだけ `/send @all <メッセージ>` を使います。`/status` でグループ状態を確認し、`/pause` / `/resume` で運用を制御できます — すべてスマートフォンから。

## CLI リファレンス

```bash
# ライフサイクル
cccc                           # daemon + Web UI を起動
cccc daemon start|status|stop  # daemon 管理

# グループ
cccc attach .                  # カレントディレクトリを紐付け
cccc groups                    # 全グループを一覧
cccc use <group_id>            # アクティブグループを切り替え
cccc group start|stop          # 全 actor を起動/停止

# Actor
cccc actor add <id> --runtime <runtime>
cccc actor start|stop|restart <id>

# メッセージング
cccc send "メッセージ" --to foreman
cccc tracked-send "委任作業" --to implementer --title "タスクタイトル" --outcome "完了条件"
cccc send "告知" --to @all  # 明示的なブロードキャスト
cccc reply <event_id> "返信"
cccc tail -n 50 -f             # ledger をリアルタイム追跡

# 受信箱
cccc inbox                     # 未読メッセージを表示
cccc inbox --mark-read         # 全件既読にする

# 運用
cccc doctor                    # 環境チェック
cccc setup --runtime <name>    # MCP を設定
cccc runtime list --all        # 利用可能なランタイム

# IM
cccc im set <platform> --token-env <ENV_VAR>
cccc im start|stop|status
```

## MCP ツール

エージェントは、コンパクトな action-oriented MCP surface を通じて CCCC と対話します。コアツールは常時公開され、追加サーフェスは必要時のみ capability pack 経由で有効化されます。

| サーフェス | 例 |
|------------|----|
| **セッションとガイダンス** | `cccc_bootstrap`、`cccc_help`、`cccc_project_info` |
| **メッセージングとファイル** | `cccc_inbox_list`、`cccc_inbox_mark_read`、`cccc_message_send`、`cccc_message_reply`、`cccc_file` |
| **グループと actor 制御** | `cccc_group`、`cccc_actor` |
| **協調と状態** | `cccc_context_get`、`cccc_coordination`、`cccc_task`、`cccc_agent_state`、`cccc_context_sync` |
| **オートメーションと記憶** | `cccc_automation`、`cccc_memory`、`cccc_memory_admin` |
| **必要時のみの拡張** | `cccc_capability_*`、`cccc_space`、`cccc_terminal`、`cccc_debug`、`cccc_im_bind` |

MCP アクセスを持つエージェントは、権限境界の中で自己組織化できます。受信箱の確認、可視返信、タスク協調、自己状態更新、そして必要なときだけの追加能力有効化が可能です。

## CCCC の位置づけ

| シナリオ | 適合度 |
|----------|--------|
| 複数のコーディングエージェントが 1 つのコードベースで協調 | ✅ コアユースケース |
| 人間 + エージェントの協調、完全な監査証跡付き | ✅ コアユースケース |
| 長時間稼働グループをスマートフォン/IM でリモート管理 | ✅ 強い適合 |
| マルチランタイムチーム（例：Claude + Codex + Gemini） | ✅ 強い適合 |
| 単一エージェントのローカルコーディングヘルパー | ⚠️ 動作するが、CCCC の価値は複数参加者で発揮 |
| 純粋な DAG ワークフローオーケストレーション | ❌ 専用オーケストレーターを使用；CCCC は補完的に利用可能 |

CCCC は**協調カーネル** — 協調レイヤーを担い、外部の CI/CD、オーケストレーター、デプロイツールとの組み合わせを維持します。

## セキュリティ

- **Web UI は高権限。** ローカル以外へ公開する前に、まず **Settings > Web Access** で **Admin Access Token** を作成してください。
- **Daemon IPC は認証なし。** デフォルトで localhost にのみバインド。
- **IM ボットトークン** は環境変数から読み取り、設定ファイルには保存しない。
- **ランタイム状態** は `CCCC_HOME`（`~/.cccc/`）に保持、リポジトリ内には置かない。
- **Capability allowlist** がエージェントの有効化できるオプション MCP サーフェスを管理。ポリシーはパッケージ内のデフォルトと `CCCC_HOME/config/` のユーザーオーバーレイで構成。

詳細なセキュリティガイダンスは [SECURITY.md](SECURITY.md) を参照。

## ドキュメント

📚 **[完全なドキュメント](https://chesterra.github.io/cccc/)**

| セクション | 説明 |
|-----------|------|
| [クイックスタート](https://chesterra.github.io/cccc/guide/getting-started/) | インストール、起動、最初のグループ作成 |
| [ユースケース](https://chesterra.github.io/cccc/guide/use-cases) | 実践的なマルチエージェントシナリオ |
| [Web UI ガイド](https://chesterra.github.io/cccc/guide/web-ui) | ダッシュボードのナビゲーション |
| [IM ブリッジ設定](https://chesterra.github.io/cccc/guide/im-bridge/) | Telegram、Slack、Discord、Feishu、DingTalk、WeCom、Weixin の接続 |
| [Group Space](https://chesterra.github.io/cccc/guide/group-space-notebooklm) | NotebookLM ナレッジ統合 |
| [ChatGPT Web Model Runtime](https://chesterra.github.io/cccc/guide/web-model-runtime) | ChatGPT Web / GPT-5.x/GPT-5.x Pro を CCCC actor として接続 |
| [Capability Allowlist](https://chesterra.github.io/cccc/guide/capability-allowlist) | MCP 機能ガバナンス |
| [ベストプラクティス](https://chesterra.github.io/cccc/guide/best-practices) | 推奨パターンとワークフロー |
| [FAQ](https://chesterra.github.io/cccc/guide/faq) | よくある質問 |
| [運用ランブック](https://chesterra.github.io/cccc/guide/operations) | 復旧、トラブルシューティング、メンテナンス |
| [CLI リファレンス](https://chesterra.github.io/cccc/reference/cli) | 完全なコマンドリファレンス |
| [SDK（Python/TypeScript）](https://github.com/ChesterRa/cccc-sdk) | 公式クライアントでアプリ/サービスから daemon を利用 |
| [アーキテクチャ](https://chesterra.github.io/cccc/reference/architecture) | 設計決定とシステムモデル |
| [機能詳細](https://chesterra.github.io/cccc/reference/features) | メッセージング、オートメーション、ランタイムの詳細 |
| [CCCS 標準](docs/standards/CCCS_V1.md) | 協調プロトコル仕様 |
| [Daemon IPC 標準](docs/standards/CCCC_DAEMON_IPC_V1.md) | IPC プロトコル仕様 |

## インストールオプション

### pip（安定版、推奨）

```bash
pip install -U cccc-pair
```

### pip（RC 版、TestPyPI）

```bash
pip install -U --pre \
  --index-url https://test.pypi.org/simple/ \
  --extra-index-url https://pypi.org/simple/ \
  cccc-pair
```

### ソースから

```bash
git clone https://github.com/ChesterRa/cccc
cd cccc
pip install -e .
```

### uv（高速、Windows 推奨）

```bash
uv venv -p 3.11 .venv
uv pip install -e .
uv run cccc --help
```

### Windows ネイティブ

- ローカル開発には、リポジトリルートの `start.ps1` を推奨します。
- `cccc doctor` が `Windows PTY: NOT READY` を表示した場合は、先に `python -m pip install pywinpty` を実行するか、`uv pip install -e .` で再インストールしてください。
- Web バンドルには `scripts/build_web.ps1`、完全パッケージビルドには `scripts/build_package.ps1` を使用してください。

### Docker

```bash
cd docker
docker compose up -d  # その後 Settings > Web Access で Admin Access Token を作成してから公開
```

Docker イメージには Claude Code、Codex CLI、Gemini CLI、Factory CLI がバンドル済み。完全な設定は [`docker/`](docker/) を参照。

### 0.3.x からのアップグレード

0.4.x はゼロからの書き直しです。先にクリーンアンインストール：

```bash
pipx uninstall cccc-pair || true
pip uninstall cccc-pair || true
rm -f ~/.local/bin/cccc ~/.local/bin/ccccd
```

再インストール後、`cccc doctor` で環境を確認。

> tmux-first の 0.3.x は [cccc-tmux](https://github.com/ChesterRa/cccc-tmux) にアーカイブ済み。

## コミュニティ

Telegram コミュニティ: [t.me/ccccpair](https://t.me/ccccpair)

ワークフローの共有、課題の相談、他の CCCC ユーザーとの情報交換にご活用ください。

## コントリビューション

コントリビューションを歓迎します：

1. 新しい Issue を開く前に既存の [Issues](https://github.com/ChesterRa/cccc/issues) を確認
2. バグ報告：`cccc version`、OS、正確なコマンド、再現手順を含める
3. 機能リクエスト：問題、提案する動作、運用への影響を記述
4. ランタイム状態は `CCCC_HOME` に保持 — リポジトリにコミットしない

## License

[Apache-2.0](LICENSE)
