<div align="center">

<img src="https://raw.githubusercontent.com/ChesterRa/cccc/main/assets/readme/hero.svg" width="100%" alt="CCCC は各グループのローカル台帳と明確な配信状態を使い、エージェントと接続先グループの協調を支えます" />

# CCCC

### コーディングエージェントをグループチャットのように指揮する

**コーディングエージェントのためのローカルファーストな協調：永続的なグループ履歴、明確なメッセージ状態、
インスタンス間の連携、Web・ターミナル・IM からのアクセス。**

自分で動かしている Claude Code、Codex、Pi、OpenCode のセッションをつなぐか、CCCC にエージェントを動かしてもらい、ひとつの永続的なチームとして運用します。

インストールはコマンドひとつ。Rust ツールチェーン、データベースサーバー、メッセージブローカー、Docker は不要です。

[![PyPI](https://img.shields.io/pypi/v/cccc-pair?label=PyPI&color=232425)](https://pypi.org/project/cccc-pair/)
[![Rust 1.88+](https://img.shields.io/badge/Rust-1.88%2B-232425?logo=rust&logoColor=white)](Cargo.toml)
[![License](https://img.shields.io/badge/license-Apache--2.0-232425)](LICENSE)
[![Docs](https://img.shields.io/badge/docs-online-232425)](https://chesterra.github.io/cccc/)
[![Discord](https://img.shields.io/badge/Discord-join-5865F2?logo=discord&logoColor=white)](https://discord.gg/m8wuB7VSJ)

[English](README.md) | [中文](README.zh-CN.md) | **日本語**

[クイックスタート](#クイックスタート) · [ドキュメント](#ドキュメント) · [コミュニティ](#コミュニティ)

</div>

---

<div align="center">

<a href="screenshots/overview.webp?raw=1" title="デスクトップ画像を原寸で表示"><img src="screenshots/overview.webp" alt="デスクトップの CCCC Web UI" width="76%" align="top"></a>
&nbsp;
<a href="screenshots/iphone.webp?raw=1" title="モバイル画像を原寸で表示"><img src="screenshots/iphone.webp" alt="スマートフォンの CCCC Web UI" width="20%" align="top"></a>

</div>

## CCCC でできること

複数のコーディングエージェントで作業を分担するときは、誰がどのタスクを持っているか、メッセージが相手に届いたか、再起動しても何が残るかを把握する必要があります。CCCC はそれを一か所で管理します。

- **履歴が残るグループチャット**：各グループは、メッセージとイベントを追記専用の台帳に記録します。タスクと共有コンテキストも保存されます。
- **明確な配信状態**：メッセージは Send、Send + Reply、Mail のいずれかで送り、配信・既読・返信はそれぞれ別の事実として記録します。
- **自分のセッションも、CCCC のエージェントも**：自分で動かしているセッションをつなぐことも、CLI や Web の多くのランタイムで CCCC にエージェントを管理させることもでき、どちらも同じグループで使えます。
- **ひとつのコントロールプレーン**：Web UI、ターミナル画面、CLI、MCP ツール、IM ブリッジは、daemon の状態を共有します。
- **Chet**：文字でも音声でも話しかけられるグローバルアシスタントです。
- **CCCC Connect**：マシンやチームをまたいでグループをつなぎます。状態はそれぞれのインスタンスが持ちます。
- **ローカルファースト**：ランタイムの状態は、リポジトリの外の `CCCC_HOME`（既定は `~/.cccc/`）に置かれます。

## 0.4.49 の主な変更

- **Web Model への切り替えと戻し**：管理者は、ほかの実行方式と同じように、エージェントを Web Model に切り替えたり戻したりできます。ネイティブセッションから直接、または Profile 経由でも切り替えられます。Web Model に切り替えても何も起動しません。エージェントの設定で会話に接続するまで、メッセージは待機します。
- **メッセージは実際に動いているものへ**：実行中のエージェントに別の実行方式や Profile を保存しても、次の起動で置き換わるまで、そのターミナルがメッセージを受け取り続けます。ターミナルへの書き込みが途中で切れたメッセージは確認用に残り、自動で書き直されません。
- **Claude の読み取りを削減**：条件を満たすエージェント間の通常のメッセージは Claude セッションの通知に直接入るようになり、別途読み取る必要がなくなりました。
- **電子ペーパー風の外観**：インターフェースは、やわらかな紙色のライトテーマとグラファイトのダークテーマになりました。レイアウト、フォント、コンポーネントは変わりません。
- **繰り返しリマインダーの回数制限**：繰り返しリマインダーを指定回数で止められます。ルール一覧に進捗が表示されます。
- **止まったアップロードの停止**：**送信** の横にある停止ボタンで待機を終え、下書きを保てます。メッセージがすでに届いている可能性があるので、再送の前に会話を確認してください。
- **ネイティブ foreman**：ネイティブ foreman も、管理された foreman と同じ既定の機能を持つようになりました。
- **タスク**：エージェントは、誰も担当していない計画中または進行中のタスクを引き受けられます。

**アップグレード時の注意**：アップグレードの前に、送信中のメッセージや録音が終わるのを待ち、復元待ちの送信を復元するか破棄してください。これらは開いているページにしか保存されません。その後、開いている Web ページを再読み込みしてください。ネイティブセッションを使う場合は[各アプリで連携を再読み込み](https://chesterra.github.io/cccc/guide/local-sessions#upgrade-an-existing-integration)してください。開いている Claude セッションは再起動（例：`claude --resume`）すると新しい受信方式になります。それまでは通知が途中で切れ、メッセージを読むよう促すことがあります。

詳しくは [0.4.49 のリリースノート](https://chesterra.github.io/cccc/release/v0.4.49_release_notes)。

## クイックスタート

### インストール

```bash
# macOS / Linux
curl -fsSL https://chesterra.github.io/cccc/install.sh | sh

# Windows の CMD または PowerShell
powershell.exe -NoProfile -ExecutionPolicy Bypass -Command "[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12; Invoke-RestMethod 'https://chesterra.github.io/cccc/install.ps1' | Invoke-Expression"

# pip でも可：同じネイティブ実行ファイルを platform wheel で導入
python -m pip install -U "cccc-pair>=0.4.36"
```

CCCC は Linux x86-64（glibc 2.28 以降）、Apple Silicon の macOS 11 以降、Windows x86-64 で動きます。詳しくは[インストールとアップグレード](#installation-and-upgrades)をご覧ください。

### 起動

```bash
cccc
```

`cccc` は daemon と Web UI を起動します。対話的なターミナルでは、ステータスパネルも開きます。`/` を入力するとコマンドを使えます。`/quit` または Ctrl+C で、この起動で開始したものを停止します。すべての機能を備えた Web UI は **http://127.0.0.1:8848** で開けます。ローカルで使う場合、パスワードは不要です。

サーバー、コンテナー、スクリプトでは `cccc --headless`（`cccc web` と同じ）を使ってください。ポート 8848 が使用中なら、`cccc --port 9000` のように別のポートを指定します。`cccc doctor` は、インストールと、検出したエージェント CLI を確認します。

### エージェントの参加方法を選ぶ

| | 自分で動かしているセッション | CCCC が動かすエージェント |
|---|---|---|
| 誰が起動するか | あなた（Codex、Claude Code、Pi、OpenCode V2 で） | CCCC（選んだランタイムで） |
| 誰が管理するか | アプリがプロセス、会話、アカウント、モデル、承認、ターミナルを持ち続けます | CCCC が起動、停止、再起動します。大半のランタイムは Web UI にターミナルを表示します |
| 承認プロンプト | アプリ自身のプロンプトがそのまま使われます | 多くのランタイムは承認プロンプトを省略して起動します |
| 設定方法 | `/setup` または `cccc sessions enable` のあと、**セッション接続** | Web UI の **新規** と **エージェントを追加**、または `cccc attach` と `cccc actor add` |

どちらの種類も、同じグループで一緒に作業できます。手順は[自分で動かしているセッションをつなぐ](#connect-the-sessions-you-already-run)または[CCCC でエージェントを動かす](#let-cccc-run-agents)をご覧ください。

## <a name="connect-the-sessions-you-already-run"></a>自分で動かしているセッションをつなぐ

自分で起動した Codex、Claude Code、Pi、OpenCode V2 のセッションは、ローカルの daemon を通じてグループに参加し、互いに連絡できます。CCCC は履歴を取り込まず、プロバイダーの API キーも必要ありません。この機能は実験的です。Linux と WSL で検証済みですが、macOS とネイティブ Windows の受け入れテストはまだです。Antigravity のセッションは、まだこの方法では接続できません。

**必要な環境**：Codex 0.163.0、Claude Code 2.1.306、Pi 1.4.0、OpenCode V2 2.1.4 のいずれか（またはそれ以降）を、CCCC を動かしているマシンで、daemon と同じ OS ユーザー・同じ `CCCC_HOME` で使います。手元のセッションが、互いから、またこのインスタンスの管理者から作業を受け取ってよい場合にだけ有効にしてください。Codex には共有 app-server も必要です。`$CODEX_HOME/config.toml`（通常は `~/.codex/config.toml`）に次を追加してください。`[features]` テーブルがすでにある場合は、その中に追加します：

```toml
[features]
daemon_auto_start = true
```

1. **アプリごとに一度だけ有効にする**：`cccc` のステータスパネルで `/setup` と入力し、Enter でアプリにチェックを付けて **Continue**、続けて **Connect** を選びます。または `cccc sessions enable --runtime codex`（または `claude`、`pi`、`opencode`）を実行します。
2. **アプリ側で読み込む**：
   - **Codex**：再起動し、確認されたら CCCC の hooks を承認して（`/hooks`）、何かメッセージを送ります。
   - **Claude Code**：再起動し（`claude --resume` で続ける会話を選べます）、何かメッセージを送ります。Plugin Monitor を有効にしておく必要があります。
   - **Pi**：`/reload` を実行するか、Pi を起動します。
   - **OpenCode V2**：元のセッションを開き直します。空の新規セッションには、最初のメッセージが必要です。
3. **確認する**：`cccc sessions list` を実行します。今は受信できない登録済みのセッションも表示するには、`--include-offline` を付けます。
4. **グループにつなぐ**：Web UI で、グループ一覧の上にある **セッション接続** を選びます（サイドバーを折りたたんでいるときは、**新規** の下のアイコン）。**既存セッションを接続** で、**接続先グループ** を指定して **グループに接続** を選ぶか、**+ 新しいグループ** を指定して **作成して接続** を選びます。複数のセッションで新しいグループを作るときは、**新しいグループの初期設定** で **フォアマン** を選びます。アプリごとの設定手順は **接続するアプリ** にあります。セッションに **現在は受信不可** と表示されたときは、ダイアログに **トラブルシューティングを見る** が表示されます。

セッションの中から頼むこともできます。CCCC が両方のセッション用のグループを作成するか、既存のグループを使います：

> CCCC を使い、/path/to/project の Codex セッションに現在のタスクについて連絡してください。Send + Reply で送り、CCCC の Send で返信するよう依頼してください。複数該当する場合は、どのセッションか私に確認してください。

ツールの承認はアプリ側に残るため、CCCC のツール呼び出しが、アプリのターミナルであなたの承認を待つことがあります。グループに届くのは、セッションが CCCC を通じて送った内容だけで、通常のターミナル出力は届きません。**CCCC アクセスを一時停止** と **CCCC のアクセスを再開** は、アプリには触れずに、1 つのグループへの参加だけを切り替えます。`cccc sessions disable --runtime codex`（または `claude`、`pi`、`opencode`）は、この CCCC のインストールが登録したものだけを削除します。

CCCC をアップグレードしたら、[各アプリで連携を読み込み直してください](https://chesterra.github.io/cccc/guide/local-sessions#upgrade-an-existing-integration)。[ネイティブセッションのガイド](https://chesterra.github.io/cccc/guide/local-sessions)では、CLI コマンド、会話の再開、復旧、制限についても説明しています。

## <a name="let-cccc-run-agents"></a>CCCC でエージェントを動かす

先に、使いたいエージェント CLI をインストールしてログインしておきます。Web UI では次のようにします：

1. サイドバーの **新規** を選び、CCCC を動かしているマシン上の **プロジェクトディレクトリ** を指定して（**参照** を使うと便利です）、**グループの作成** を選びます。
2. メッセージ入力欄の上のメンバー行で **+**（**エージェントを追加**）を選び、検出された **ランタイム** を選んで **エージェント ID** を入力し、**エージェントを追加** を選びます。最初のエージェントがフォアマンになり、それ以降はピアになります。
3. ヘッダーにあるグループの状態ボタンを開き、**グループを起動** を選びます。
4. **メッセージ** で入力し、**送信** を選びます（Ctrl+Enter、Mac では Cmd+Enter）。

コマンドラインで行う場合は、CCCC を起動した状態で（たとえば別のターミナルで `cccc` を実行して）次のようにします：

```bash
cd /path/to/your/repo
cccc attach .                                # このフォルダーのグループを作成し、アクティブにする
cccc actor add lead --runtime claude         # 最初のエージェントがフォアマンになる
cccc actor add implementer --runtime codex   # ピア
cccc group start                             # グループの有効なエージェントを起動
cccc send "Please inspect the repo and propose the first safe task." --to "@foreman"
cccc tracked-send "Please take the first concrete task and reply with validation evidence." \
  --to implementer \
  --title "First concrete task" \
  --outcome "The change and validation evidence are reported"
```

Claude Code と Codex は、CCCC が起動するときに CCCC の MCP ツールを受け取るため、`cccc setup` の手順は不要です。CLI ではエージェントを `actor` と呼びます。PowerShell では、`@` をそのまま書くとスプラッティングが始まるため、`"@foreman"` は引用符で囲んでください。また、`tracked-send` は 1 行で入力してください。行末の `\` で行を続けられるのは POSIX シェルだけです。

**初回起動時**：Claude がワークスペースの信頼や、`--dangerously-skip-permissions` の免責事項への同意を求めることがあります。このとき、エージェントには **ターミナルでの確認を待っています** と表示されます。Web UI でそのエージェントのターミナルを開いて答えてください（**確認用ターミナルを開く** でそこへ移動できます）。CCCC が代わりに答えることはありません。拒否した場合は、**起動** または **再起動** でやり直せます。

**承認**：多くのランタイムは、`claude --dangerously-skip-permissions` のように承認プロンプトを省略して起動します。各エージェントの設定でコマンドを確認し、[自律実行の既定値](https://chesterra.github.io/cccc/guide/runtimes#autonomy-defaults)にも目を通してください。

**うまくいけば**、メンバー行に両方のエージェントが表示され、**メッセージ** にフォアマンの返信が届き、追跡付きの依頼にタスクが紐付いて表示されます。エージェントが起動しない場合は、そのエージェントを開いてエラーを読み、CLI がインストール済みでログインしているかを確かめて、`cccc doctor` を実行してください。[FAQ](https://chesterra.github.io/cccc/guide/faq) も参照してください。

## グローバルアシスタント Chet

Chet は CCCC インスタンス全体で 1 つのアシスタントで、どのグループのメンバー一覧にも含まれません。グループ一覧の下にある **Chet** から開いて入力するか、その横の **音声を開始** でライブ通話を始めます。送信時に選んでいるグループはコンテキストとして渡されますが、権限の境界にはなりません。

- **ランタイム**：**設定 → このインスタンス → 音声 → Chet → ランタイム** で設定します。Chet の入力、文字起こし、結果の履歴は、CCCC を動かしているマシンに保存されます。ただし、送った内容自体は、設定したモデルや音声のプロバイダーに渡ります。
- **音声（実験的）**：**Codex Voice** は `codex login` を使います。**Grok Voice** は `grok login` を使い、CCCC の PATH に `grok` が必要です。どちらも追加の API キーは不要です。報告されている Grok Voice の音の高さや声質が変わる問題は、修正を確認できていません。
- **グループ内**：メンバーは、宛先を `@chet` だけにして、Send または Send + Reply で Chet に連絡します。Mail は使えません。
- **制限**：すべてのグループのメッセージが、Chet の 1 つのセッションに入ります。グループごとに分けて扱うのはモデルへの指示であり、分離ではありません。Chet はグループ間で転送せず、CCCC Connect でも送信しません。

**音声秘書**（**設定 → このインスタンス → 音声 → 音声秘書**）は、録音を各グループ向けの文書、プロンプトの下書き、回答にまとめる、別のグローバルサービスです。会話はすべてのグループで共有されます。[Chet](https://chesterra.github.io/cccc/guide/web-ui#chet-global-assistant-experimental) と[音声秘書（Voice Secretary）のガイド](https://chesterra.github.io/cccc/guide/voice-secretary)を参照してください。

## 主な概念

| 用語 | 意味 |
|---|---|
| グループ | 専用の台帳、タスク、コンテキストを持つ共有の会話。複数のプロジェクトフォルダーにまたがることもできます |
| エージェント | エージェントランタイムで動く、またはそれに接続されたグループのメンバー。CLI では `cccc actor`、API では `actor_id` などのフィールド名のままです |
| フォアマンとピア | 最初のエージェントがフォアマンで、全体を取りまとめます。ほかはピアです。`@foreman`、`@peers`、`@all` で宛先に指定します |
| Send（送信）、Send + Reply（送信 + 返信）、Mail（メール） | Send はすぐに届けます。Send + Reply は返信も追跡します。Mail は作業を中断させずに、エージェントの受信箱で待ちます。1 件のメッセージの宛先は、あなたかエージェントのどちらか一方です |
| あなたの回答待ち | あなたの回答が必要な、エージェントからの依頼 |
| `CCCC_HOME` | ランタイムの状態を置く場所。既定は `~/.cccc/` |
| セッション接続、CCCC Connect | **セッション接続** はこのマシンのセッションをグループに加えます。CCCC Connect はインスタンス間でグループをつなぎます |

後でよい連絡には Mail、相手にすぐ知らせたいときは Send、回答が必要なときは Send + Reply を使います。担当者、期待する結果、証拠が必要な委任作業には `tracked-send` を使います。

## Web UI の使い方

- **メッセージ**：`@` でエージェントを、`#` でグループをメンションします。メッセージにポインターを合わせると **返信**、**転送**、コピーが表示されます。**その他** メニューには、クイック返信、**ピン留め**、**リンクをコピー** もあります。タッチ画面では、すべての操作が **その他** にまとまっています。
- **タブ**：**あなたの回答待ち**（その場で回答できる依頼）、**すべて**、**ピン留め**、**ユーザー関連** があります。フィルターで絞り込んだ表示では、**会話で表示** を選ぶと、メッセージを会話全体の中で開きます。
- **メッセージ** と **ターミナル**：グループのヘッダーで表示を切り替えます。ターミナル表示では 1 ページに最大 4 つのエージェントが並び、キーボード入力も受け付けます。
- **ファイル**、**Presentation**（固定スロット 4 つ）、**プロジェクトのコンテキスト**（**タスク**、**AGENTS.md**、**連携ログ**、**エージェント状態**、**スキル**）も使えます。
- **設定とその他** → **すべての設定**：**この作業グループ** にはガイダンス、自動化、メッセージ配信、メッセージルーティング、IMブリッジなどが、**このインスタンス** にはアカウント、Web アクセス、通知、音声などがあります。
- **表示**：**テーマ** はライト、ダーク、自動（システムのライト・ダーク設定に従う）から、言語は英語、中国語、日本語から選べます。**その他の表示設定** には、**本文フォント**、**メッセージ欄の幅**（幅 768px 以上の画面で、標準・ワイド・全幅）、**アニメーション**、**「あなたの回答待ち」の表示**、**文字サイズ**（70%、90%、100%、125%）があります。
- **通知**：ブラウザーごとに、HTTPS または localhost で開いたページで **設定 → このインスタンス → 通知 → 通知を有効にする** を選びます。iPhone や iPad（iOS／iPadOS 16.4 以降）では、先に CCCC をホーム画面に追加してください。
- **自動化**：未読の Mail、返信待ちの依頼、アイドル状態のエージェント、動きのないグループについて、回数を抑えたリマインダーを送ります。また、あなたが作った一定間隔・定期・1 回限りのルールを実行します。
- **グループをコピー**：グループの永続的な状態をエクスポート・インポートします。ワークスペースのファイルと、CCCC が保存している認証情報ファイルは含みません。ただし、含まれる履歴や添付ファイルに、機密性の高い内容が残っている場合があります。

## 対応ランタイム

エージェントごとに別のランタイムを使えます：

| CCCC での動かし方 | ランタイム（`--runtime` の ID） |
|---|---|
| ネイティブ端末 + CCCC が管理するセッション | Claude Code（`claude`）、Codex CLI（`codex`）、Grok Build（`grok`）、OpenCode V2 2.1.4 以降（`opencode`）、Kilo Code（`kilo`） |
| ネイティブ端末または公式 ACP | GitHub Copilot（`copilot`）、Cursor（`cursor`）、Devin（`devin`）、Kiro（`kiro`）、Antigravity（`antigravity`）、Droid（`droid`）、Auggie（`auggie`）、Hermes（`hermes`）、Kimi Code（`kimi`） |
| ネイティブ端末 | Cline（`cline`）、Amp（`amp`） |
| 構造化された接続（端末なし） | Pi 1.1.0 以降（`pi`）、DeepSeek Harness（`deepseek`、開発者プレビュー） |
| Web モデル | ChatGPT Web（`web_model`）、ChatGPT Dots（`chatgpt_dots`）、Grok Bot Web Model（`grok_web_model`） |
| 任意のコマンド | `custom` |

これらのランタイムの MCP は CCCC が自動で設定します。例外は 3 つです。Cursor のターミナルでは、`cccc setup --runtime cursor` が Cursor 内で実行するプロンプトを表示します。Web モデルはリモート MCP コネクターを使います。`custom` は手動で設定します。Antigravity の ACP モードでは、別に一度だけログインが必要です：`cccc setup --runtime antigravity --runtime-mode acp --login`。`cccc runtime list` で、この環境で使えるランタイムを確認できます（`--all` で `custom` も表示）。

Web モデルには、それぞれ専用の設定が必要です。ChatGPT Web は、ブラウザー経由の配信と、公開 HTTPS URL 上のリモート MCP コネクターを使います。実験的な **GPT Pro** モードでは、ごく小さな空白の PNG を添付します。ChatGPT Dots は、ChatGPT のイベントで起動するタスクを使います。Grok Bot Web Model は、既存の `https://grok.com/bot/<UUID>` の Webhook Routine を使います。[対応ランタイムのガイド](https://chesterra.github.io/cccc/guide/runtimes)、[ChatGPT Web](https://chesterra.github.io/cccc/guide/web-model-runtime)、[Dots](https://chesterra.github.io/cccc/guide/runtimes#chatgpt-dots)、[Grok Bot](https://chesterra.github.io/cccc/guide/grok-web-model-runtime) を参照してください。

## どこからでも CCCC を使う

### リモートアクセス

ローカルで直接使う場合、アクセストークンは不要です。Web UI をほかのマシンに公開する前に、**設定 → このインスタンス → Web アクセス** で **管理者アクセストークン** を作成してください。

- **LAN**：`cccc --host 0.0.0.0 --port 8848` を使うか、**Web アクセス** に保存したバインド設定を使います。WSL2 の既定の NAT 環境では、ミラーモードのネットワークを有効にするか、Windows 側に転送ルールを追加する必要もあります。
- **自分のトンネルやリバースプロキシ**：たとえば `cloudflared tunnel --url http://127.0.0.1:8848` を使って HTTPS で公開し、[Web UI ガイド](https://chesterra.github.io/cccc/guide/web-ui#security)の説明どおりに転送ヘッダーを保護してください。
- **管理されたリモートアクセス**（Linux と macOS）：**設定 → このインスタンス → アカウント** でインスタンスを連携し、**Web アクセス** で **リモートアクセス** をオンにします。または `cccc login` と `cccc reach on` を実行します。バージョンを固定した `cloudflared` を `CCCC_HOME` にインストールし、リポジトリも台帳もアップロードしません。

変更したバインド設定は、**今すぐ適用** を選ぶか、自分で使っているプロセス管理ツールを再起動すると反映されます。`--host` と `--port` は保存したバインド設定より優先され、保存した設定は `CCCC_WEB_HOST` と `CCCC_WEB_PORT` より優先されます。

### IM ブリッジ

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
```

Telegram、Slack、Discord、Mattermost、Feishu/Lark、DingTalk、WeCom、Weixin（ボットとの 1 対 1 のチャットのみ）に対応しています。チャットでは、通常のテキストか `/send @foreman <message>` を送ります。`/status` でグループの状態を確認でき、`/pause` と `/resume` でそのチャットの購読を一時停止・再開できます。[IM ブリッジのガイド](https://chesterra.github.io/cccc/guide/im-bridge/)を参照してください。

### CCCC Connect

CCCC Connect は CCCC インスタンス間でグループをつなぐ機能で、**セッション接続** とは別のものです。ここでの「アカウント」は CCCC のアカウントのことで、プロバイダーへのログインではありません。アカウントを使う場合は、各インスタンスに到達可能な HTTPS アドレスも必要です（**リモートアクセス** で設定するものなど）。アカウントを連携しただけでは、到達できるようにはなりません。

| 方法 | 範囲と設定 |
|---|---|
| **同じアカウント** | **設定 → このインスタンス → アカウント** でインスタンスを連携すると、それぞれのグループとエージェントが互いにメッセージを送れるようになります。 |
| **別のメンバー** | サイドバーにあるグループの **⋮** メニューか、グループ設定から **グループ接続** を開き、メンバー ID を招待します。双方のメンバーがアカウントサイトで確認します。 |
| **直接接続（アカウント不要）** | **グループ接続 → 直接接続** を開き、信頼できる経路で招待をやり取りして、グループの組を承認します。どちらか一方のインスタンスに、LAN、VPN などの経路で到達できる必要があります。 |

接続でやり取りできるのは、選んだグループ間のメッセージ、返信、小さなファイルです。ターミナルやワークスペースへのアクセスは含まれず、状態はそれぞれのインスタンスが持ちます。エージェントは `cccc_connect` で相手を見つけ、`dst_instance_id` と `dst_group_id` で宛先を指定します。[CCCC Connect のガイド](https://chesterra.github.io/cccc/guide/connect)を参照してください。

## セキュリティ

- **Web UI は強い権限を持ちます**：ローカル以外に公開する前に管理者アクセストークンを作成し、公開アクセスには HTTPS を使ってください。
- **daemon の IPC は、アプリケーション認証のない、信頼されたローカルインターフェースです**（Unix ソケットまたはループバック TCP）。外部に公開したり、信頼できないユーザーとランタイムのホームディレクトリを共有したりしないでください。
- **ネイティブセッションはこの OS ユーザーを信頼します**：有効にしたセッションは、互いから、またこのインスタンスの管理者から作業を受け取れます。送信元の ID は認証情報ではありません。
- **CCCC が管理するエージェントは、承認プロンプトを省略して動くことがよくあります**：機密性の高い作業を任せる前に、コマンドを確認してください。
- **IM の認証情報** は、環境変数の参照でも直接の値でも設定できます。参照を使うことをおすすめします。
- **接続の範囲は明示的です**：ほかのインスタンスのワークスペースを開くには、そのインスタンスの管理者アクセストークンが必要です。
- **Capability allowlist** は任意の MCP ツールを制御します。エージェントのプロセスのサンドボックスではありません。

脆弱性の報告方法は [SECURITY.md](SECURITY.md) をご覧ください。

## CLI 早見表

```bash
cccc                         # daemon + Web UI とステータスパネル（cccc --headless はパネルなし）
cccc doctor                  # インストールと環境のチェック
cccc daemon status           # ほかに start、stop

cccc attach .                # このフォルダーのグループを作成
cccc groups                  # グループの一覧。cccc use <group_id> でアクティブにする
cccc group start             # グループのエージェントを起動（ほかに stop）

cccc actor add <agent_id> --runtime <runtime>   # CLI ではエージェントは "actor"
cccc actor restart <agent_id>                   # ほかに start、stop。一覧は cccc actor list

cccc sessions enable --runtime codex            # または claude、pi、opencode
cccc sessions list
cccc sessions join --group <group_id> --sources <source_id>

cccc send "Please coordinate the next step." --to "@foreman"
cccc tracked-send "Implement the feature." --to <agent_id> --title "Feature" --outcome "Done and validated"
cccc reply <event_id> "Thanks, that looks good."
cccc tail -n 50 -f           # グループの台帳を追いかけて表示
cccc inbox --actor-id <agent_id>   # エージェントの次の Mail バッチを読み取り、消費する

cccc runtime list            # この環境で使えるランタイム（--all で custom も表示）
cccc setup --runtime <id>    # ランタイムの MCP 設定を明示的に準備
cccc im start                # ほかに stop、status
```

すべてのコマンドの説明は `cccc --help` で確認できます。[CLI リファレンス](https://chesterra.github.io/cccc/reference/cli)も参照してください。

## 連携する開発者向け

**MCP ツール**：エージェントには、常にコンパクトなコアが見えています。`cccc_bootstrap`、`cccc_help`、機能の検索と利用、Inbox、メッセージ、ファイル、`cccc_context_get`、`cccc_coordination`、`cccc_task`、`cccc_agent_state`、`cccc_connect` です。`cccc_tracked_send`、`cccc_memory`、`cccc_group`、`cccc_actor`、`cccc_repo`、`cccc_terminal`、`cccc_automation` などのほかの組み込みツールは、`cccc_capability_use` から使えます。ネイティブセッションには、ほかのセッションの一覧表示、メッセージ送信、グループ化を行う `cccc_session` もあります。

**SDK**：[Python、TypeScript、Rust のクライアント](https://github.com/ChesterRa/cccc-sdk)は、実行中の `cccc` に接続します。daemon は含まれていません。

```bash
pip install -U cccc-sdk
npm install cccc-sdk
cargo add cccc-sdk
```

**アーキテクチャ**

```mermaid
graph TB
    M["CCCC が動かすエージェント<br/>Claude Code · Codex · Grok Build · ... · custom"]
    N["自分で動かしているセッション<br/>Codex · Claude Code · Pi · OpenCode"]
    W["Web モデル<br/>ChatGPT Web · Grok Bot"]
    D["ChatGPT Dots"]
    Daemon["CCCC daemon · 単一ライター<br/>台帳 · タスク · コンテキスト · エージェントのライフサイクル"]
    Ports["Web UI · ターミナル画面 · CLI · MCP"]
    IM["IM ブリッジ"]
    Remote["接続済みの CCCC グループ"]

    M <-->|ターミナル / ランタイムのプロトコル<br/>MCP| Daemon
    N <-->|MCP + ネイティブの hooks、<br/>プラグインまたは拡張機能| Daemon
    W <-->|ブラウザー配信または Webhook<br/>リモート MCP| Daemon
    D <-->|MCP Events<br/>リモート MCP| Daemon
    Daemon <--> Ports
    Ports <-->|Web がホストする連携| IM
    Daemon <-->|CCCC Connect / Direct<br/>承認済みの接続| Remote
```

daemon は、エージェントのライフサイクル、配信、協調に関わる変更を受け持ちます。台帳は追記専用です。Web UI、ターミナル画面、CLI、MCP、IM ブリッジは、すべて daemon を経由します。Web のプロセスは、ブラウザー、音声、Chet、IM の連携も動かします。[アーキテクチャ](https://chesterra.github.io/cccc/reference/architecture)を参照してください。

## CCCC が向いている場面

CCCC が向いているのは、複数のエージェントが 1 つのコードベースを共有する場合、Claude + Codex + Kimi のようにランタイムを混在させる場合、スマートフォンや IM から見守る長時間稼働のグループ、マシンやチームをまたぐグループです。補助のエージェント 1 つだけでも使えますが、価値の大半は参加者が複数いるときに生まれます。また、DAG ワークフローのオーケストレーターではありません。ネイティブのエージェントチーム、並列タスクランナー、IM アシスタントゲートウェイを置き換えるのではなく、補完します。[FAQ の比較](https://chesterra.github.io/cccc/guide/faq#how-does-cccc-compare-to-native-agent-teams-and-other-tools)と[位置づけ](https://chesterra.github.io/cccc/reference/positioning)を参照してください。

## <a name="installation-and-upgrades"></a>インストールとアップグレード

**Web サイトのインストーラー**：GitHub Releases から、チェックサムを検証した実行ファイルをダウンロードします。対象は最新の安定版か、`CCCC_VERSION` で指定したリリースです。このインストーラーが管理していない `cccc` コマンドは上書きしません（そのコマンドをアンインストールするか、`CCCC_INSTALL_DIR` を設定してください）。既定のディレクトリに入れる場合は、新しいコマンドをユーザーの PATH の先頭に置きます。新しいターミナルで `cccc doctor` を実行すると、PATH で選ばれる `cccc` と、競合があればそれも表示されます。

**pip**（`cccc-pair>=0.4.36`）は、同じ実行ファイルを platform wheel で導入します。下限を指定するのは、pip が古い Python だけのリリースを選ばないようにするためです。Python パッケージやフォールバックはありません。

**アップグレード**：Web サイトのインストーラーで入れた場合は `cccc update`、pip で入れた場合は pip のコマンドを使います。`cccc update --check` は、何も変更せずに最新のリリースとインストールの管理元を表示します。pip でアップグレードする前に、`cccc daemon stop` を実行し、フォアグラウンドの CCCC プロセスを終了してください。pip から Web サイトのインストーラーに切り替えるときは、先に `python -m pip uninstall cccc-pair` を実行します。古い `cccc update` が 0.4.35 のままの場合は[アップグレードの FAQ](https://chesterra.github.io/cccc/guide/faq#why-does-an-older-cccc-update-stay-on-0-4-35) を、完全なチェックリストは[運用ランブック](https://chesterra.github.io/cccc/guide/operations)をご覧ください。

**ソースから**：Rust 1.88 以降、npm 付きの Node.js 24、Python 3.11 以降（アーカイブ作成の補助にのみ使用）が必要です：

```bash
git clone https://github.com/ChesterRa/cccc
cd cccc
./scripts/build_package.sh
./target/release/cccc --version
```

Windows では `x86_64-pc-windows-msvc` ツールチェーンを使い、`powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\scripts\build_package.ps1` を実行します。デバッグビルドには `cargo run --locked --features standalone -p cccc --bin cccc -- --port 0` を使います。

**Docker**：[`docker/`](docker/) で `docker compose up -d` を実行します。localhost の外に公開する前に、**設定 → このインスタンス → Web アクセス** で管理者アクセストークンを作成してください。イメージには Claude Code、Codex CLI、Factory CLI が入っています。

**旧バージョン**：Intel Mac 向けの最後のリリースは CCCC v0.4.37 です。tmux を中心にした 0.3.x 系は [cccc-tmux](https://github.com/ChesterRa/cccc-tmux) にアーカイブされています。0.4.x をインストールする前に、0.3.x をそのパッケージマネージャーでアンインストールしてください。再インストールしても 0.3.x のセッションはグループに変換されないため、データは残しておいてください。`cccc python`、`cccc rust`、`ccccd` エイリアスは廃止されました。`cccc daemon …` を使ってください。

## ドキュメント

| セクション | 内容 |
|---|---|
| [はじめに](https://chesterra.github.io/cccc/guide/getting-started/) | インストール、起動、最初のグループの作成 |
| [ネイティブセッション](https://chesterra.github.io/cccc/guide/local-sessions) | 自分の Codex、Claude Code、Pi、OpenCode V2 のセッションをつなぐ |
| [Web UI ガイド](https://chesterra.github.io/cccc/guide/web-ui) | Web UI、Chet、通知、リモートアクセス |
| [対応ランタイム](https://chesterra.github.io/cccc/guide/runtimes) | 各ランタイムの設定、既定値、動作 |
| [ユースケース](https://chesterra.github.io/cccc/guide/use-cases)と[ベストプラクティス](https://chesterra.github.io/cccc/guide/best-practices) | 実践的なシナリオとパターン |
| [CCCC Connect](https://chesterra.github.io/cccc/guide/connect) | アカウント経由の接続と直接接続、配信、権限 |
| [音声秘書（Voice Secretary）](https://chesterra.github.io/cccc/guide/voice-secretary) | 音声入力、文書、ライブ音声 |
| [IM ブリッジの設定](https://chesterra.github.io/cccc/guide/im-bridge/) | Telegram、Slack、Discord、Mattermost、Feishu、DingTalk、WeCom、Weixin |
| [Group Space](https://chesterra.github.io/cccc/guide/group-space-notebooklm) | NotebookLM とのナレッジ連携 |
| [Capability Allowlist](https://chesterra.github.io/cccc/guide/capability-allowlist) | MCP 機能の管理 |
| [FAQ](https://chesterra.github.io/cccc/guide/faq) と[運用ランブック](https://chesterra.github.io/cccc/guide/operations) | よくある質問、復旧、トラブルシューティング、アップグレード |
| [CLI リファレンス](https://chesterra.github.io/cccc/reference/cli) | すべてのコマンドのリファレンス |
| [リリースノート](https://chesterra.github.io/cccc/release/) | 各リリースの変更点 |
| [アーキテクチャ](https://chesterra.github.io/cccc/reference/architecture)と[機能](https://chesterra.github.io/cccc/reference/features) | 設計上の判断と機能の詳細 |
| [CCCS 標準](docs/standards/CCCS_V1.md)と [Daemon IPC 標準](docs/standards/CCCC_DAEMON_IPC_V1.md) | プロトコル仕様 |

## コミュニティ

ワークフローの共有や質問、ほかの CCCC ユーザーとの交流にご利用ください：

- **Discord**：[CCCC の Discord に参加](https://discord.gg/m8wuB7VSJ)
- **Telegram**：[t.me/ccccpair](https://t.me/ccccpair)
- **バグ報告・機能提案**：[GitHub Issues](https://github.com/ChesterRa/cccc/issues)

## コントリビューション

コントリビューションを歓迎します。次の点をお願いします：

1. 新しい Issue を作る前に、既存の [Issues](https://github.com/ChesterRa/cccc/issues) を確認してください
2. バグ報告には、`cccc --version`、OS、実行したコマンドそのもの、再現手順を含めてください
3. 機能の提案では、課題、提案する動作、運用への影響を説明してください
4. ランタイムの状態は `CCCC_HOME` に置き、リポジトリには決してコミットしないでください

開発環境の準備、品質ゲート、プルリクエストの流れは [CONTRIBUTING.md](CONTRIBUTING.md) をご覧ください。

## ライセンス

[Apache-2.0](LICENSE)
