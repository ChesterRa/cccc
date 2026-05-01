# Features

Detailed feature documentation for CCCC.

## IM-Style Messaging

### Core Contracts

- Messages are first-class citizens: once sent, they're committed to the ledger
- Read receipts are explicit: agents call MCP to mark as read
- Reply/quote are structured: `reply_to` + `quote_text`
- @mention enables precise delivery

### Sending Messages

```bash
# CLI
cccc send "Hello"                 # No --to: default recipient policy applies (default foreman)
cccc send "Hello" --to @foreman
cccc send "Announcement" --to @all # Explicit broadcast
cccc tracked-send "Delegated work" --to assistant --title "Task title" --outcome "Done criterion"
cccc reply <event_id> "Reply text"

# MCP
cccc_message_send(text="Hello", to=["@foreman"])
cccc_tracked_send(title="Task title", text="Delegated work", to=["assistant"], outcome="Done criterion")
cccc_message_reply(reply_to="evt_xxx", text="Reply")
```

### Read Receipts

- Agents call `cccc_inbox_mark_read(event_id)` to mark as read
- Read is cumulative: marking X means X and all before are read
- Cursors stored in `state/read_cursors.json`

### Delivery Mechanism

```
Message written to ledger
    ↓
Daemon parses the "to" field
    ↓
For each target actor:
    ├─ PTY running → inject into terminal
    └─ Otherwise → leave in inbox
    ↓
Wait for agent to call mark_read
```

Delivery format:
```
[cccc] user → peer-a: Please implement the login feature
[cccc] user → peer-a (reply to evt_abc): OK, please continue
```

## IM Bridge

### Streaming Events

The `chat.stream` event type represents real-time streaming content from agents. Stream events are used only for user-facing progressive rendering (e.g., AI Card typewriter effect on DingTalk) and are **not** delivered to actor inboxes.

| Event | Direction | Description |
|-------|-----------|-------------|
| `chat.stream` | Outbound (to IM) | Streaming content chunk for progressive display |

### Design Principles

- **1 Group = 1 Bot**: Simple, isolated, easy to understand
- **Explicit subscription**: Chat must `/subscribe` before receiving messages
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
| Weixin / WeChat | ✅ Complete | Web-configured account/login flow |

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
| `/subscribe` | Subscribe, start receiving messages |
| `/unsubscribe` | Unsubscribe |
| `/verbose` | Toggle verbose mode |
| `/status` | Show group status |
| `/pause` / `/resume` | Pause/resume message delivery |
| `/help` | Show help |

Notes:
- In direct chats and in group chats where the bot is @mentioned, plain text is treated as implicit send to the default recipient policy (default: foreman).
- Reserve `/send @all <message>` for true broadcasts, announcements, or urgent shared constraints.
- In channels (Slack/Discord), mention the bot and then use `/send` (to avoid platform slash-commands).
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

MCP Tools (authoritative playbook + execution interface)
├── cccc_help: Operation guide (playbook)
├── cccc_project_info: Get PROJECT.md
├── cccc_inbox_list / cccc_inbox_mark_read: Inbox
└── cccc_message_send / cccc_message_reply: Send/reply

Ledger (complete memory)
└── All historical messages and events
```

### Core Principles

- **Do**: One authoritative playbook (`cccc_help`)
- **Do**: Kernel enforcement (RBAC by daemon)
- **Do**: Minimal startup handshake (Bootstrap)
- **Don't**: Write three versions of the same copy

### Agent Standard Workflow

```
1. Receive SYSTEM injection → Know who you are
2. Call cccc_inbox_list → Get unread messages
3. Process messages → Execute tasks
4. Call cccc_inbox_mark_read → Mark as read
5. Call cccc_message_reply → Reply with results
6. Wait for next message
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

### Built-in Automation

| Behavior | Config | Default | Description |
|----------|--------|---------|-------------|
| Nudge | `nudge_after_seconds` | 300s | Digest follow-up for pending unread or obligation items |
| Reply-required nudge | `reply_required_nudge_after_seconds` | 300s | Follow-up for required-reply obligations |
| Attention-ack nudge | `attention_ack_nudge_after_seconds` | 600s | Follow-up for attention messages lacking ACK |
| Unread nudge | `unread_nudge_after_seconds` | 900s | Reminder when unread backlog keeps accumulating |
| Actor idle | `actor_idle_timeout_seconds` | 0s | Optional actor idle notification to foreman; `0` disables it by default |
| Keepalive | `keepalive_delay_seconds` | 120s | Follow-up after an actor declares a next step and then goes quiet |
| Silence check | `silence_timeout_seconds` | 0s | Optional group-level silence review and idle transition; `0` disables it |
| Help nudge | `help_nudge_interval_seconds` / `help_nudge_min_messages` | 600s / 10 | Prompt actor to revisit `cccc_help` and refresh working context |

### Delivery Policy

| Config | Default | Description |
|--------|---------|-------------|
| `auto_mark_on_delivery` | `false` | Automatically advance the read cursor after a PTY delivery succeeds |

Low-level delivery throttling via `min_interval_seconds` remains supported in daemon/API settings for compatibility, but it is no longer exposed in the default Web settings UI.

## Runtime-Only Actor Secrets

CCCC supports per-actor private environment variables for runtime customization (different model/API stacks per actor).

- Stored in runtime state under `CCCC_HOME/state/secrets/actors/`
- Not written into the group ledger
- Not included in group templates/blueprints
- Visible as key metadata only (values are never returned by read APIs)

CLI surface:

```bash
cccc actor secrets <actor_id> --set KEY=VALUE
cccc actor secrets <actor_id> --unset KEY
cccc actor secrets <actor_id> --keys
```

## Blueprint / Group Template

CCCC Web supports blueprint export/import for portable group setup.

- Export captures actors, actor startup autoload baselines, group settings/feature toggles, automation rules/snippets, and guidance overrides.
- Import uses replace semantics (applies the incoming configuration as the new group setup).
- Ledger history is preserved (import does not rewrite historical events).
- Environment secrets are intentionally excluded.

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

| Runtime | Integration | Description |
|---------|-------------|-------------|
| amp | `amp` | Amp |
| auggie | `auggie` | Auggie (Augment CLI) |
| claude | `claude` | Claude Code |
| codex | `codex` | Codex CLI |
| droid | `droid` | Droid |
| gemini | `gemini` | Gemini CLI |
| kimi | `kimi --yolo` | Kimi CLI |
| neovate | `neovate` | Neovate Code |
| web_model | Remote MCP + browser delivery | ChatGPT Web conversation, including GPT-5.x/GPT-5.x Pro when selected in ChatGPT |
| custom | Custom | Any command |

CCCC first-class runtime support is the nine named runtimes above. `custom` remains the manual fallback for any other command.

### Setup Commands

```bash
cccc setup --runtime claude   # Configure MCP (auto)
cccc setup --runtime codex
cccc setup --runtime droid
cccc setup --runtime amp
cccc setup --runtime auggie
cccc setup --runtime neovate
cccc setup --runtime gemini
cccc setup --runtime kimi
cccc setup --runtime custom
```

`web_model` does not use `cccc setup`; create the single `ChatGPT Web Model` actor, copy its remote MCP URL from Web Settings, and bind one specific ChatGPT conversation.

### Runtime Detection

```bash
cccc doctor        # Environment check + runtime detection
cccc runtime list  # List available runtimes (JSON)
```
