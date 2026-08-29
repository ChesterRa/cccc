# Features

Detailed feature documentation for CCCC.

## IM-Style Messaging

### Core Contracts

- Messages are first-class citizens: once sent, they're committed to the ledger
- Delivery intent is explicit: `mail`, `send`, or `request_reply`
- Inbox reads are consuming: `cccc_inbox_read` returns the next ordered batch
  and commits its read boundary
- Reply/quote are structured: `reply_to` + `quote_text`
- @mention enables precise delivery

### Sending Messages

```bash
# CLI
cccc send "Hello"                 # No --to: default recipient policy applies (default foreman)
cccc send "Hello" --to @foreman
cccc send "Background context" --to peer-a --mode mail
cccc send "Please answer" --to peer-a --mode request-reply
cccc send "Announcement" --to @all # Explicit broadcast
cccc tracked-send "Delegated work" --to assistant --title "Task title" --outcome "Done criterion"
cccc reply <event_id> "Reply text"

# MCP
cccc_message_send(text="Hello", to=["@foreman"], mode="send", insight="This direction may still be framed too narrowly.")
cccc_tracked_send(title="Task title", text="Delegated work", to=["assistant"], outcome="Done criterion", insight="The assignee should be free to reject the proposed approach.")
cccc_message_reply(reply_to="evt_xxx", text="Reply", insight="The original framing may be hiding a better route.")
```

Agents may add `suggested_user_message` when sending to `user`; CCCC Web shows it as an editable next-message suggestion in the composer and never sends it automatically.

Each message has one audience domain: `user` alone, or one/more agents. Mixed
human/agent recipient lists are rejected. Mail is agent-only; use Send or Send +
Reply for the human user.

### Mail Inbox Reads

- Agents call `cccc_inbox_read()` to fetch and consume the next Mail batch
- Bootstrap and Web polling use Mail-only internal peek semantics and do not consume it
- Read is cumulative in Mail append order; direct Send traffic is not replayed
- The schema-versioned Mail cursor is stored in `state/read_cursors.json`
- Past direct traffic is available explicitly through `cccc_message_history`

### Delivery Mechanism

```
Message written to ledger
    ↓
message_mode=mail → Inbox only
message_mode=send → claim and hand off to the runtime now
message_mode=request_reply → Send plus a concrete reply obligation
    ↓
Daemon records runtime.delivery for each attempted handoff
    ↓
Agent consumes deferred Mail with cccc_inbox_read
```

Delivery format:
```
[cccc] user → peer-a: Please implement the login feature
[cccc] user → peer-a (reply to evt_abc): OK, please continue
```

## IM Bridge

### Streaming Events

The `chat.stream` event type represents real-time streaming content from agents. The native IM worker progressively updates Telegram, Slack, Discord, and Feishu messages, DingTalk AI Cards, and WeCom native stream replies. Every stream frame carries an immutable sender-title snapshot when available, and all adapters render the same actor prefix used by the completed `chat.message`, so successful stream deduplication never removes the reply's author identity. Weixin currently uses completed-message delivery because its SDK has no stable editable-message contract. Stream events are **not** delivered to actor inboxes.

Progressive rendering is never the durability boundary. Every platform falls back to the completed `chat.message`; replies beyond a provider's single-message limit are split on Unicode-safe boundaries and delivered losslessly. A failed or truncated preview cannot suppress the final fallback.

| Event | Direction | Description |
|-------|-----------|-------------|
| `chat.stream` | Outbound (to IM) | Streaming content chunk for progressive display |

### Design Principles

- **1 Group = 1 Bot**: Simple, isolated, easy to understand
- **Explicit authorization**: Standard bot chats use `/subscribe`; the account confirming a Weixin QR login is authorized automatically
- **Ports are thin**: Only do message forwarding; daemon is the only state source

### Supported Platforms

| Platform | Status | Token Config |
|----------|--------|--------------|
| Telegram | ✅ Complete | `token_env` |
| Slack | ✅ Complete | `bot_token_env` + `app_token_env` |
| Discord | ✅ Complete | `token_env` |
| Feishu/Lark | ✅ Complete | `feishu_app_id_env` + `feishu_app_secret_env` |
| DingTalk | ✅ Complete | `dingtalk_app_key_env` + `dingtalk_app_secret_env` (+ optional `dingtalk_robot_code_env`) |
| WeCom | ✅ Complete | Web-configured Bot ID / Secret flow |
| Weixin / WeChat | ✅ Complete | Web-configured account/login flow (direct chats only) |

### Configuration

```yaml
# group.yaml
im:
  platform: telegram
  token_env: TELEGRAM_BOT_TOKEN

# Slack requires dual tokens
im:
  platform: slack
  bot_token_env: SLACK_BOT_TOKEN    # xoxb-... Web API
  app_token_env: SLACK_APP_TOKEN    # xapp-... Socket Mode
```

### IM Commands

| Command | Description |
|---------|-------------|
| `/send <message>` | Send using group default (default: foreman) |
| `/send @<agent> <message>` | Send to a specific agent |
| `/send @all <message>` | Broadcast to all agents |
| `/send @peers <message>` | Send to non-foreman agents |
| `/subscribe` | Request chat authorization (not required for the Weixin QR-login account) |
| `/unsubscribe` | Unsubscribe |
| `/verbose [on\|off]` | Enable verbose delivery, or disable it with `off` |
| `/status` | Show group status |
| `/pause` / `/resume` | Pause/resume delivery for the current chat or thread |
| `/help` | Show help |

Notes:
- Confirming a Weixin QR login immediately authorizes the scanning account. The bridge repairs that authorization when restoring stored credentials, so no binding key, manual approval, or `/subscribe` step is required.
- In direct chats, and on group-capable platforms where the bot is @mentioned, plain text is treated as implicit send to the default recipient policy (default: foreman). Weixin currently supports direct bot chats only.
- A recognized CCCC slash command counts as an explicit bot address and may be used without @mention in group chats; ordinary group text and files still require @mention. The native worker applies the same rule across providers, including Feishu.
- Reserve `/send @all <message>` for true broadcasts, announcements, or urgent shared constraints.
- In channels (Slack/Discord), @mention the bot for plain text; a recognized CCCC slash command can address it directly.
- You can configure the default recipient behavior in Web UI: Settings → Messaging → Default Recipient.

### CLI Commands

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
cccc im stop
cccc im status
cccc im logs -f
```

## Agent Guidance

### Information Hierarchy

```
System Prompt (thin layer)
├── Who you are: Actor ID, role
├── Where you are: Working Group, Scope
└── What you can do: MCP tool list + key reminders (see cccc_help)

MCP Tools (protocol + execution interface)
├── cccc_help: On-demand CCCC protocol reference
├── cccc_capability_use: Invoke hidden tools without mounting every pack
├── cccc_inbox_read: Consume the next Mail batch
├── cccc_message_history: Inspect actor-visible chat history without consuming Mail
└── cccc_message_send / cccc_message_reply: Send/reply

Ledger (complete memory)
└── All historical messages and events
```

### Core Principles

- **Do**: One compact protocol reference (`cccc_help`)
- **Do**: Kernel enforcement (RBAC by daemon)
- **Do**: Minimal startup handshake (Bootstrap)
- **Do**: Keep heuristic automation opt-in for new groups
- **Don't**: Write three versions of the same copy

### Minimal Protocol Loop (example)

```
1. Cold start or resume → Call cccc_bootstrap
2. Need deferred messages → Call cccc_inbox_read
3. Do the work with the agent runtime's normal tools and judgment
4. Reply visibly with cccc_message_reply
5. The returned Inbox batch is already marked read
```

## Automation

Automation in CCCC combines built-in automation and user-defined rules.

Built-in automation covers system-managed follow-ups and collaboration health loops.

Rules cover scheduled reminders and operational actions, with snippets as reusable message templates.

### Rule Triggers

| Trigger type | Web label | Protocol | Typical use |
|--------------|-----------|----------|-------------|
| Interval | Every N minutes | `every_seconds` | Standup/checkpoint reminders |
| Recurring schedule | Daily / Weekly / Monthly | `cron` | Fixed-time recurring reminders |
| One-time schedule | Countdown / Exact time | `at` | One-off reminders and operations |

Notes:
- Web UI intentionally hides raw cron expression editing by default.
- Operational actions are intentionally constrained to one-time trigger.

### Rule Actions

| Action | Who configures | Trigger support | Description |
|--------|----------------|-----------------|-------------|
| `notify` | Web + MCP | interval / recurring / one-time | Send system notification to selected recipients |
| `group_state` | Web (foreman/admin) | one-time only | Set group state (`active` / `idle` / `paused` / `stopped`) |
| `actor_control` | Web (foreman/admin) | one-time only | Start/stop/restart selected actor runtimes |

### One-Time Completion Semantics

- One-time rules auto-mark as completed after firing.
- Completed one-time rules are disabled (no repeated fire).
- UI supports clearing completed items for cleanup.

### Scheduling and Lifecycle Semantics

- A new interval rule starts its clock on the first scheduler tick; it does not
  fire immediately.
- Paused and stopped groups run no automation. Idle groups continue user rules
  but suppress the built-in standup reminder.
- Resume does not replay missed interval, cron, or one-time work. Future
  one-time rules remain scheduled.
- Notifications are durable ledger events for enabled matching recipients, so
  the recipient runtime does not need to be running when the rule fires.

### Built-in Delivery and Automation

| Behavior | Config | Default | Description |
|----------|--------|---------|-------------|
| Mail notice | `delivery.mail_notice_after_seconds` | 1800s | One content-free Inbox reminder for a concrete pending Mail batch; no repeat or escalation |
| Reply notice | `delivery.reply_notice_after_seconds` | 900s | One content-free reminder after an accepted `request_reply` remains unanswered |
| Actor idle | `actor_idle_timeout_seconds` | 0s | Optional actor idle notification to foreman; `0` disables it by default |
| Keepalive | `keepalive_delay_seconds` | 0s | Optional follow-up after an actor declares a next step and then goes quiet |
| Silence check | `silence_timeout_seconds` | 0s | Optional group-level silence review and idle transition; `0` disables it |
| Help nudge | `help_nudge_interval_seconds` / `help_nudge_min_messages` | 0s / 0 | Optional prompt to revisit `cccc_help` |

These are defaults written for newly created groups. Heuristic steering stays
off by default. Mail and reply notices are bounded delivery semantics, not
periodic automation: paused/stopped actors are not woken, notices never include
message bodies, and no universal runtime-idle detector is assumed.

### Delivery Policy

| Config | Default | Description |
|--------|---------|-------------|
| `min_interval_seconds` | `0` | Optional per-actor spacing between runtime handoffs; `0` disables throttling |

Runtime handoff and Inbox read are separate facts. A successful
`runtime.delivery` never advances the Inbox cursor.

## Runtime-Only Actor Secrets

CCCC supports per-actor private environment variables for runtime customization (different model/API stacks per actor).

- Stored in runtime state under `CCCC_HOME/state/secrets/actors/`
- Not written into the group ledger
- Not included in Copy Groups packages
- Visible as key metadata only (values are never returned by read APIs)

CLI surface:

```bash
cccc actor secrets <actor_id> --set KEY=VALUE
cccc actor secrets <actor_id> --unset KEY
cccc actor secrets <actor_id> --keys
```

## Copy Groups

CCCC Web supports Copy Groups export/import for durable group copy, migration, and backup.

- Export creates a zip package with durable CCCC group state: ledger history, actors, context, blobs, memory, assistants, automation, and settings.
- Workspace repository/project files are not included. Users provide or remap the workspace root during import.
- System credentials, browser profiles, provider auth, live runtime state, locks, and rebuildable caches are excluded. Copy packages still contain user content such as ledger history, memory, and attachments, so they should be handled as sensitive data.
- Imported groups start idle with actors stopped. If the packaged group id already exists, import creates a new copy and does not steal the existing workspace default mapping.
- Copy Groups replaces the former group-template Web path; durable group features should be carried by Copy Groups unless explicitly blacklisted as unsafe or runtime-only.

### MCP Management Surface

```text
cccc_automation_state
cccc_automation_manage(op=create|update|enable|disable|delete|replace_all, ...)
```

`cccc_automation_manage` is optimized for reminder management by agents:
- Foreman can manage all notify reminders and full replace.
- Peer can manage only own-personal or shared notify reminders.
- Operational actions (`group_state`, `actor_control`) stay Web/Admin-facing.

## Web UI

### Agent-as-Tab Mode

- Each agent is a tab
- Chat tab + Agent tabs
- Click tab to switch view
- Mobile: swipe to switch

### Main Features

- Group management (create/edit/delete)
- Actor management (add/start/stop/edit/delete)
- Message sending (@mention autocomplete)
- Message reply (quote display)
- Embedded terminal (xterm.js)
- Context panel (vision/sketch/tasks)
- Settings panel (automation config)
- IM Bridge configuration

### Theme System

- Light / Dark / System
- CSS variables define all colors
- Terminal colors adapt automatically

### Remote Access

Recommended options:

- **Cloudflare Tunnel + Cloudflare Access (Recommended)**
  - Best experience: access directly from mobile browser
  - Strongly recommend Access for login protection
  - Quick (temporary URL): `cloudflared tunnel --url http://127.0.0.1:8848`
  - Stable (custom domain): Use `cloudflared tunnel create/route/run`

- **Tailscale (VPN)**
  - Clear security boundary (Tailnet ACL)
  - Recommend binding to tailnet IP only: `CCCC_WEB_HOST=$TAILSCALE_IP cccc`

## Multi-Runtime Support

### Supported Runtimes

| Runtime | Entrypoint / Surface | Description |
|---------|----------------------|-------------|
| amp | `amp` | Amp |
| auggie | `auggie` | Auggie (Augment CLI) |
| claude | `claude` | Claude Code |
| cline | `cline` | Cline CLI PTY TUI |
| codex | `codex` | Codex CLI |
| copilot | `copilot` | GitHub Copilot CLI |
| cursor | `cursor-agent` | Cursor CLI |
| devin | `devin` | Devin CLI |
| kiro | `kiro-cli` | Kiro CLI |
| kilo | `kilo` | Kilo Code CLI |
| antigravity | `agy` | Antigravity CLI |
| droid | `droid` | Droid |
| grok | `grok` | Grok Build |
| hermes | `hermes` | Hermes Agent |
| kimi | `kimi` | Kimi CLI |
| opencode | `opencode` | OpenCode |
| web_model | ChatGPT Web conversation | ChatGPT Web conversation with CCCC MCP access; optional experimental GPT Pro delivery attaches a tiny blank PNG but does not select the model or guarantee connector availability |
| custom | Any command | Any command |

These entries show stable runtime entrypoints or surfaces, not every runtime-specific launch flag. CCCC applies launch defaults automatically and actor/profile commands can be reviewed or customized in settings.

CCCC first-class runtime support is the named runtimes above. `custom` remains the manual fallback for any other command.

### Setup Commands

```bash
cccc setup --runtime claude   # Configure MCP (auto)
cccc setup --runtime cline
cccc setup --runtime codex
cccc setup --runtime droid
cccc setup --runtime amp
cccc setup --runtime auggie
cccc setup --runtime grok
cccc setup --runtime hermes
cccc setup --runtime kimi
cccc setup --runtime opencode
cccc setup --runtime cursor       # Prompt-assisted setup inside Cursor CLI
cccc setup --runtime kilo         # Prompt-assisted setup inside Kilo Code CLI
cccc setup --runtime antigravity  # Prompt-assisted setup inside Antigravity
cccc setup --runtime custom
```

`web_model` does not use `cccc setup`; create the single `ChatGPT Web Model` actor from the CCCC Web group, then use Web Settings to sign in to ChatGPT, copy its remote MCP URL, and bind one specific ChatGPT conversation.

### Runtime Detection

```bash
cccc doctor        # Environment check + runtime detection
cccc runtime list  # List available runtimes (JSON)
```
