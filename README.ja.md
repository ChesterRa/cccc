# CCCC Pair — デュアルAI自律協調オーケストレーター

[English](README.md) | [中文](README.zh-CN.md) | **日本語**

2つのAIが対等なパートナーとして**自律的に協力し、タスクを自動的に推進**します。あなたが目標を設定すれば、彼らは自分たちでコミュニケーションを取り、計画を立て、実装し、互いにレビューします。TUIやチャットツールでいつでも全体を把握できますが、常に介入する必要はありません。

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)
[![PyPI](https://img.shields.io/pypi/v/cccc-pair)](https://pypi.org/project/cccc-pair/)
[![Python](https://img.shields.io/pypi/pyversions/cccc-pair)](https://pypi.org/project/cccc-pair/)
[![Telegram](https://img.shields.io/badge/Telegram-コミュニティ-2CA5E0?logo=telegram)](https://t.me/ccccpair)

---

![CCCC TUI インターフェース](./screenshots/tui-main.png)

### ランタイム実行画面

![CCCC ランタイム画面](./screenshots/tui-runtime.png)

> **4ペインレイアウト**：左上にTUIコンソール（Timeline + ステータスバー）、右上にPeerA端末、右下にPeerB端末。スクリーンショットではForemanが戦略分析を行い、両Peer（opencodeを使用）がそれぞれタスクを処理し自動的に連携しています。

---

## なぜシングルAgentではなくデュアルAIなのか？

### シングルAgentの課題

| 問題 | 症状 |
|------|------|
| **常に監視が必要** | シングルAgentはしばらく動くと止まり、次のプロンプトを与え続けないと進まない |
| **コンテキストの喪失** | セッションをまたぐと以前の会話を忘れ、同じ説明を繰り返す必要がある |
| **検証の欠如** | 長々と説明するが、正しいかどうかわからず、実際に実行されていない |
| **意思決定の不透明性** | 問題発生時に追跡が困難：いつ変更された？なぜ？誰が承認した？ |

### CCCCのソリューション

| 特徴 | 効果 |
|------|------|
| **自律的な推進** | 2つのPeer間で自動的にコミュニケーション・連携し、1サイクル10-15分間継続実行可能。Foremanの定期起動と組み合わせることで、ほぼ中断のない継続運用を実現 |
| **相互チェック** | 一方が提案し、もう一方が問題点を指摘。より良い選択肢が自然に浮かび上がり、エラーが早期に発見される |
| **エビデンス重視** | テストが通り、ログが安定し、コードがコミットされて初めて「完了」。口だけでは完了にならない |
| **完全な追跡可能性** | すべての決定、すべてのハンドオフが記録され、問題発生時に調査・ロールバックが可能 |

---

## コア機能

### 自律協調エンジン

- **デュアルPeerアーキテクチャ**：PeerAとPeerBが対等なパートナーとして、mailboxメカニズムを通じて自動的に情報を交換しタスクを推進
- **インテリジェントハンドオフ**：内蔵のhandoffメカニズムにより、Peer間で自動的にコンテキストと成果物を受け渡し
- **セルフチェック**：定期的なself-checkで方向性を確認し、逸脱を防止
- **キープアライブ機構**：Keepaliveで会話の停滞を防ぎ、nudgeで保留タスクの処理を促す

### ゼロコンフィグTUI

- **インタラクティブセットアップ**：初回起動時にSetupパネルを表示、↑↓で選択・Enterで確定、設定ファイルの編集不要
- **リアルタイムTimeline**：すべての会話フローをスクロール表示、PeerA/PeerB/System/Youのメッセージが一目瞭然；メッセージ幅はターミナルサイズに自動適応
- **ステータスパネル**：handoffカウント、self-check進捗、Foremanステータスをリアルタイム表示；一時停止時には目立つPAUSED表示
- **コマンド補完**：Tab自動補完、Ctrl+R履歴検索、標準ショートカット完全対応

### エビデンス駆動ワークフロー

- **POR/SUBPORアンカー**：戦略ボード（POR.md）とタスクシート（SUBPOR.md）がリポジトリに存在し、全員が同じ真実を見る
- **小さなコミット**：各パッチは150行以下、レビュー可能・ロールバック可能
- **監査ログ**：ledger.jsonlがすべてのイベントを記録し、問題発生時に追跡可能

### マルチロールシステム

| ロール | 責務 | 必須？ |
|--------|------|--------|
| **PeerA** | 主要な実行者の1人、PeerBと対等に協力 | はい |
| **PeerB** | 主要な実行者の1人、PeerAと対等に協力 | はい |
| **Aux** | オンデマンドで呼び出される補助ロール、バッチタスクや重いテストなどを処理 | いいえ |
| **Foreman** | 定期実行される「ユーザー代理」、周期的なチェックとリマインダーを実行 | いいえ |

> **自由な組み合わせ**：どのロールも対応するどのCLIでも使用可能。必要に応じて柔軟に設定できます。

### マルチプラットフォームブリッジ

- **Telegram / Slack / Discord**：オプションで接続し、チームが普段使う場所に作業を持ち込む
- **双方向通信**：チャットでコマンド送信、ステータス受信、RFD承認
- **ファイル双方向転送**：ファイルをアップロードしてPeerに処理させたり、Peerが生成したファイルを受け取ったりできる

**IMチャットコマンド**：

**メッセージルーティング**（全プラットフォーム）：
- `a: <メッセージ>` / `b: <メッセージ>` / `both: <メッセージ>` — Peerにルーティング

**CLIパススルー**（全プラットフォーム）：
- `a! <コマンド>` / `b! <コマンド>` — ラッパーなしで直接CLI入力

**Telegramコマンド**（スラッシュプレフィックス）：
- `/a` `/b` `/both` — ルーティングエイリアス
- `/pa` `/pb` `/pboth` — パススルーエイリアス（グループ向け）
- `/aux <プロンプト>` — Auxを1回実行
- `/foreman on|off|status|now` — Foremanを制御
- `/restart peera|peerb|both` — Peer CLIを再起動
- `/pause` `/resume` — 配信を一時停止/再開
- `/status` — システムステータスを表示
- `/verbose on|off` — 詳細出力をオン/オフ
- `/whoami` `/subscribe` `/unsubscribe` — サブスクリプション管理
- `/help` — コマンドヘルプを表示

**Slack / Discordコマンド**（感嘆符プレフィックス）：
- `!aux <プロンプト>` — Auxを1回実行
- `!foreman on|off|status|now` — Foremanを制御
- `!restart peera|peerb|both` — Peer CLIを再起動
- `!pause` `!resume` — 配信を一時停止/再開
- `!status` — システムステータスを表示
- `!verbose on|off` — 詳細出力をオン/オフ
- `!subscribe` `!unsubscribe` — サブスクリプション管理
- `!help` — コマンドヘルプを表示

> **注意**：Telegramは `/` プレフィックス（ネイティブボットコマンド）を使用。Slack/Discordは `!` プレフィックス（プラットフォームコマンド干渉を回避）を使用。

---

## 重要な設定ファイル：PROJECT.md と FOREMAN_TASK.md

この2つのファイルはAIにタスクを伝える核心的な入口です。**丁寧に記述してください**：

### PROJECT.md（プロジェクト説明）

リポジトリルートに配置され、**PeerAとPeerBのシステムプロンプトに自動注入**されます。

**含めるべき内容**：
- プロジェクトの背景と目標
- 技術スタックとアーキテクチャの概要
- コーディング規約と慣例
- 現在のフェーズの重点タスク
- Peerが知っておくべきコンテキスト

```markdown
# プロジェクト概要
これはxxxシステムで、Python + FastAPI + PostgreSQLを使用...

# 現在の重点
1. ユーザー認証モジュールの完成
2. データベースクエリパフォーマンスの最適化

# コーディング規約
- type hintsを使用
- すべての関数にdocstringが必要
- テストカバレッジ > 80%
```

### FOREMAN_TASK.md（監督タスク）

リポジトリルートに配置され、**Foremanに自動注入**されます。Foremanは15分ごとに実行され、このファイルを読んで何をするか決定します。

**含めるべき内容**：
- 定期的にチェックすべき項目
- 常駐タスクリスト
- 品質ゲート要件

```markdown
# Foreman 常駐タスク

## 毎回のチェック
1. `pytest` を実行してテストが通ることを確認
2. POR.md が更新が必要かチェック
3. 未処理のTODOがないか確認

## 品質要件
- 失敗したテストをスキップしない
- 新しいコードには対応するテストが必要
```

> **ヒント**：タスクが複雑になるほど、これらのファイルが重要になります。意図を明確に書くことで、Peerたちが正確に理解し自律的に推進できます。

---

## 対応Agent CLI

CCCCは特定のAIに縛られません。どのロールも以下のいずれかのCLIを使用できます：

| CLI | 公式ドキュメント |
|-----|------------------|
| **Claude Code** | [docs.anthropic.com/claude-code](https://docs.anthropic.com/en/docs/claude-code) |
| **Codex CLI** | [github.com/openai/codex](https://github.com/openai/codex) |
| **Gemini CLI** | [github.com/google-gemini/gemini-cli](https://github.com/google-gemini/gemini-cli) |
| **Factory Droid** | [factory.ai](https://factory.ai/) |
| **OpenCode** | [opencode.ai/docs](https://opencode.ai/docs/) |
| **Kilocode** | [kilo.ai/docs/cli](https://kilo.ai/docs/cli) |
| **GitHub Copilot** | [github.com/features/copilot/cli](https://github.com/features/copilot/cli) |
| **Augment Code** | [docs.augmentcode.com/cli](https://docs.augmentcode.com/cli/overview) |
| **Cursor** | [cursor.com/cli](https://cursor.com/en-US/cli) |

> インストール方法は各CLIの公式ドキュメントを参照してください。mailboxプロトコルに従うCLIならどれでも接続可能です。

---

## クイックスタート

### ステップ1：前提依存関係のインストール

CCCCはtmuxを使用してマルチペイン端末レイアウトを管理します。以下の依存関係がインストールされていることを確認してください：

| 依存関係 | 説明 | インストール方法 |
|----------|------|------------------|
| **Python** | ≥ 3.9 | ほとんどのシステムにプリインストール済み |
| **tmux** | ターミナルマルチプレクサ、マルチペインレイアウト用 | macOS: `brew install tmux`<br>Ubuntu/Debian: `sudo apt install tmux`<br>Windows: WSLが必要 |
| **git** | バージョン管理 | ほとんどのシステムにプリインストール済み |
| **Agent CLI** | 少なくとも1つ必要 | 下記参照 |

**Agent CLIのインストール**（少なくとも1つ）：
```bash
# Claude Code（推奨）
npm install -g @anthropic-ai/claude-code

# Codex CLI
npm install -g @openai/codex

# Gemini CLI
npm install -g @anthropic-ai/gemini-cli

# OpenCode
go install github.com/opencode-ai/opencode@latest
```

> **Windowsユーザー**：CCCCはWSL（Windows Subsystem for Linux）環境で実行する必要があります。まず[WSLをインストール](https://docs.microsoft.com/ja-jp/windows/wsl/install)してから、WSL端末で以降の操作を行ってください。

### ステップ2：CCCCのインストール

```bash
# 方法1：pipx（推奨、環境を自動隔離）
pip install pipx  # pipxがなければ先にインストール
pipx install cccc-pair

# 方法2：pip
pip install cccc-pair
```

### ステップ3：初期化と起動

```bash
# 1. プロジェクトディレクトリに移動
cd your-project

# 2. CCCCを初期化（.cccc/ディレクトリと設定ファイルを作成）
cccc init

# 3. 環境が準備できているか確認
cccc doctor

# 4. 起動！
cccc run
```

**起動後の画面**：
- tmuxが4ペインレイアウトで開く：左上TUI、左下ログ、右上PeerA、右下PeerB
- 初回実行時はSetupパネルが表示され、↑↓でCLIを各ロールに割り当て
- 確定後、Peerたちが自動起動し作業開始

> **ヒント**：`cccc doctor` でエラーが出た場合は、指示に従って不足している依存関係をインストールしてください。チャットブリッジ（Telegram/Slack/Discord）はTUIのSetupパネルで設定できます。

---

## よく使うコマンド

TUI入力欄で使用（Tabで補完可能）：

| コマンド | 機能 |
|----------|------|
| `/help` | 完全なコマンドリストを表示 |
| `/a <メッセージ>` | PeerAに送信 |
| `/b <メッセージ>` | PeerBに送信 |
| `/both <メッセージ>` | 両方のPeerに同時送信 |
| `/pause` | handoff配信を一時停止（メッセージはinboxに保存） |
| `/resume` | handoff配信を再開（保留中にNUDGE送信） |
| `/restart peera\|peerb\|both` | Peer CLIプロセスを再起動 |
| `/quit` | CCCCを終了（tmuxをデタッチ） |
| `/setup` | 設定パネルを開く/閉じる |
| `/foreman on\|off\|status\|now` | Foremanを制御（有効な場合） |
| `/aux <プロンプト>` | Auxを呼び出して一度だけタスクを実行 |
| `/verbose on\|off` | Peer要約 + Foreman CCをオン/オフ |

**自然言語ルーティング**（スラッシュなしでもOK）：
```
a: このPRのセキュリティをレビューして
b: 完全なテストスイートを実行して
both: 次のマイルストーンを計画しよう
```

### クロスプラットフォームコマンド対照表

| カテゴリ | コマンド | TUI | Telegram | Slack | Discord |
|----------|----------|-----|----------|-------|---------|
| **ルーティング** | PeerAに送信 | `/a` | `/a` または `a:` | `a:` | `a:` |
| | PeerBに送信 | `/b` | `/b` または `b:` | `b:` | `b:` |
| | 両方に送信 | `/both` | `/both` または `both:` | `both:` | `both:` |
| **パススルー** | CLIをPeerAに | — | `a!` または `/pa` | `a!` | `a!` |
| | CLIをPeerBに | — | `b!` または `/pb` | `b!` | `b!` |
| | CLIを両方に | — | `/pboth` | — | — |
| **制御** | 配信を一時停止 | `/pause` | `/pause` | `!pause` | `!pause` |
| | 配信を再開 | `/resume` | `/resume` | `!resume` | `!resume` |
| | Peerを再起動 | `/restart` | `/restart` | `!restart` | `!restart` |
| | 終了 | `/quit` | — | — | — |
| **操作** | Foreman制御 | `/foreman` | `/foreman` | `!foreman` | `!foreman` |
| | Auxを実行 | `/aux` | `/aux` | `!aux` | `!aux` |
| | 詳細モード | `/verbose` | `/verbose` | `!verbose` | `!verbose` |
| **サブスクリプション** | chat IDを取得 | — | `/whoami` | — | — |
| | サブスクライブ | — | `/subscribe` | `!subscribe` | `!subscribe` |
| | サブスクライブ解除 | — | `/unsubscribe` | `!unsubscribe` | `!unsubscribe` |
| **ユーティリティ** | ステータス表示 | — | `/status` | `!status` | `!status` |
| | ヘルプ表示 | `/help` | `/help` | `!help` | `!help` |
| | 設定パネル | `/setup` | — | — | — |

> **凡例**：`/cmd` = スラッシュプレフィックス、`!cmd` = 感嘆符プレフィックス、`x:` = コロンルーティング、`x!` = パススルー、— = 非対応

---

## キーボードショートカット

| ショートカット | 機能 |
|----------------|------|
| `Tab` | コマンド補完 |
| `↑ / ↓` | 履歴コマンドを参照 |
| `Ctrl+R` | 履歴を逆方向検索 |
| `Ctrl+A / E` | 行頭/行末にジャンプ |
| `Ctrl+W` | 前の単語を削除 |
| `Ctrl+U / K` | 行頭/行末まで削除 |
| `PageUp / PageDown` | Timelineをスクロール |
| `Ctrl+L` | Timelineをクリア |

---

## 高度な機能

### Auto-Compact（コンテキスト圧縮）

長時間実行後、Peerのコンテキストを自動圧縮し、トークンの無駄と思考の混乱を防止：
- Peerのアイドル状態を検出
- 条件を満たすと自動トリガー（デフォルト：6メッセージ以上、15分間隔、2分間アイドル）
- 対応CLIにcompactコマンドを送信

### Foreman（ユーザー代理）

オプションの定期タスクロール、一定間隔（デフォルト15分）でプリセットタスクを実行：
- `FOREMAN_TASK.md` を編集してタスクを定義
- `/foreman on|off` でオン/オフを制御
- 定期チェック、POR更新リマインダーなどのシナリオに最適

### RFD（決定リクエスト）

重要な決定には人間の承認が必要：
- PeerがRFDカードを発行
- チャットブリッジに承認ボタンを表示
- ユーザーが承認後に実行を継続

---

## ディレクトリ構造

```
.cccc/                          # オーケストレータードメイン（デフォルトでgitignore）
  settings/                     # 設定ファイル
    cli_profiles.yaml           # ロールバインディング、配信設定
    agents.yaml                 # CLI定義
    telegram.yaml / slack.yaml  # ブリッジ設定
  mailbox/                      # メッセージ交換
  state/                        # ランタイム状態
    ledger.jsonl                # イベントログ
    status.json                 # 現在の状態
  logs/                         # Peerログ
  rules/                        # システムプロンプト

docs/por/                       # アンカードキュメント
  POR.md                        # 戦略ボード
  T######-slug/SUBPOR.md        # タスクシート

PROJECT.md                      # プロジェクト概要（システムプロンプトに織り込まれる）
FOREMAN_TASK.md                 # Foremanタスク定義
```

---

## よくある質問

### 2つのPeerは本当に自動で協力できるのですか？

はい。mailboxメカニズムを通じて、PeerAとPeerBは自動的にメッセージを交換し、作業成果を受け渡します。目標を設定すれば、彼らは自分たちで方策を議論し、分担して実装し、互いにレビューします。いつでも介入できますが、必須ではありません。

### 常に画面を見ている必要がありますか？

いいえ。これがCCCCがシングルAgentと異なる核心的な価値です。タスクを設定すれば、Peerたちは自律的に推進します。Telegram/Slack/Discordでいつでも進捗を確認でき、人間の判断が必要な事項はRFDで通知されます。

### どのCLIがベストですか？

ニーズによります。各CLIには異なる特徴があり、どのロールにも自由に組み合わせられます。まずデフォルト設定で試してみて、実際の体験に基づいて調整することをお勧めします。

### 設定ファイルを編集する必要がありますか？

基本的に不要です。TUIのSetupパネルでポイント＆クリック設定が可能です。上級ユーザーは `.cccc/settings/` 下のYAMLファイルを直接編集して細かく調整できます。

### 問題が発生したらどうやって調査しますか？

1. `.cccc/state/status.json` を確認して現在の状態を把握
2. `.cccc/state/ledger.jsonl` を確認してイベントログを参照
3. `.cccc/state/orchestrator.log` を確認してランタイムログを参照
4. `cccc doctor` を実行して環境をチェック

### 新しいタスクのために状態をリセットするには？

`cccc reset` を使用してランタイム状態をクリアし、最初からやり直します：

```bash
# 基本リセット：state/mailbox/logs/workをクリアし、POR/SUBPORファイルを削除
cccc reset

# アーカイブモード：POR/SUBPORをタイムスタンプ付きアーカイブに移動してからクリア
cccc reset --archive
```

使用シナリオ：
- 前のタスクを完了し、まったく新しいタスクを開始する場合
- 蓄積したinboxメッセージとランタイム状態をクリアする場合
- POR/SUBPORファイルをリセットして計画をやり直す場合

> **注意**：オーケストレーターが実行中の場合、確認が求められます。先に `cccc kill` を実行することをお勧めします。

---

## 詳細情報

アーキテクチャの詳細説明、完全な設定リファレンス、その他のFAQについては[英語ドキュメント](README.md)を参照してください。

---

## コミュニティとサポート

- **Telegramコミュニティ**: [t.me/ccccpair](https://t.me/ccccpair)
- **GitHub Issues**: [問題報告や提案](https://github.com/anthropics/cccc/issues)

---

## License

Apache 2.0
