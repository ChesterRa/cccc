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
cccc_message_send(text="Hello", to=["@foreman"], insight="This direction may still be framed too narrowly.")
cccc_tracked_send(title="Task title", text="Delegated work", to=["assistant"], outcome="Done criterion", insight="The assignee should be free to reject the proposed approach.")
cccc_message_reply(reply_to="evt_xxx", text="Reply", insight="The original framing may be hiding a better route.")
```

Agents may add `suggested_user_message` when sending to `user`; CCCC Web shows it as an editable next-message suggestion in the composer and never sends it automatically.

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
| Telegram | Rust text adapter (`teloxide`) | `bot_token_env` |
| Slack | Rust text adapter (official Socket Mode/Web API) | `bot_token_env` + `app_token_env` |
| Discord | Rust text adapter (`serenity`) | `bot_token_env` |
| Feishu/Lark | Rust text adapter (`lark-channel`) | `feishu_app_id` + `feishu_app_secret` |
| DingTalk | Rust text adapter with outbound image/file delivery (`dingtalk-stream`) | `dingtalk_app_key` + `dingtalk_app_secret` |
| WeCom | Built-in Rust adapter with streaming, media upload/download, and official AI Bot long-connection compatibility | Web-configured Bot ID / Secret flow |
| Weixin / WeChat | Rust text adapter (`weixin-agent`) | SDK QR login and persisted bot token |

No vendor currently publishes an official Rust SDK for these seven platforms. CCCC uses established Rust SDKs that implement the official platform protocols where available; Slack uses the official Socket Mode and Web API directly. The built-in WeCom client follows the wire behavior of WeCom's official Node and Python AI Bot SDKs; it is not presented as an official WeCom Rust SDK.

DingTalk outbound attachments are resolved only from validated `state/blobs/*` paths, limited to 10 MiB, uploaded with the SDK, and delivered through the robot OpenAPI to authorized DingTalk conversations. Text replies continue to use the active session webhook. DingTalk inbound attachments and attachment delivery on the other IM adapters remain outside the current Rust feature set.

Local Web chat uploads accept up to 100 MiB total per message. Rust streams
multipart chunks into content-addressed blob storage instead of buffering the
whole request in memory. Cross-group remote attachments remain limited to
10 MiB because that transport currently carries Base64 content.

### Configuration

```yaml
# group.yaml (normally written by `cccc im set` or Web settings)
im_bridge:
  config:
    platform: telegram
    bot_token_env: TELEGRAM_BOT_TOKEN

# Slack requires dual tokens
im_bridge:
  config:
    platform: slack
    bot_token_env: SLACK_BOT_TOKEN    # xoxb-... Web API
    app_token_env: SLACK_APP_TOKEN    # xapp-... Socket Mode
```

WeCom must have the AI Bot WebSocket subscription enabled in the WeCom administration console. CCCC sends the subscription as the first application frame, starts heartbeat only after the subscription acknowledgement, and reports authentication errors instead of treating a bare WebSocket handshake as a running bot.

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

MCP Tools (protocol + execution interface)
├── cccc_help: On-demand CCCC protocol reference
├── cccc_capability_use: Invoke hidden tools without mounting every pack
├── cccc_inbox_list / cccc_inbox_mark_read: Inbox
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
2. Need the full unread queue → Call cccc_inbox_list
3. Do the work with the agent runtime's normal tools and judgment
4. Reply visibly with cccc_message_reply
5. Mark handled inbox items read
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
| Nudge | `nudge_after_seconds` | 0s | Optional digest follow-up for pending unread or obligation items |
| Reply-required nudge | `reply_required_nudge_after_seconds` | 300s | Reliability follow-up for required-reply obligations |
| Attention-ack nudge | `attention_ack_nudge_after_seconds` | 600s | Reliability follow-up for attention messages lacking ACK |
| Unread nudge | `unread_nudge_after_seconds` | 0s | Optional reminder when unread backlog keeps accumulating |
| Actor idle | `actor_idle_timeout_seconds` | 0s | Optional actor idle notification to foreman; `0` disables it by default |
| Keepalive | `keepalive_delay_seconds` | 0s | Optional follow-up after an actor declares a next step and then goes quiet |
| Silence check | `silence_timeout_seconds` | 0s | Optional group-level silence review and idle transition; `0` disables it |
| Help nudge | `help_nudge_interval_seconds` / `help_nudge_min_messages` | 0s / 0 | Optional prompt to revisit `cccc_help` |

These are defaults written for newly created groups. Heuristic steering stays off by default, while explicit reply/attention obligations retain reliability reminders. Existing groups are not migrated; legacy groups that omit these fields retain the daemon's compatibility fallbacks until explicitly changed.

### Delivery Policy

| Config | Default | Description |
|--------|---------|-------------|
| `auto_mark_on_delivery` | `true` | Automatically advance the read cursor after a local runtime delivery succeeds |

Low-level delivery throttling via `min_interval_seconds` remains supported in daemon/API settings for compatibility, but it is no longer exposed in the default Web settings UI.

## Runtime-Only Actor Secrets

CCCC supports per-actor private environment variables for runtime customization (different model/API stacks per actor).

- Stored in group runtime state under `CCCC_HOME/groups/<group_id>/state/`
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

### Message Reliability

- Chat sends carry a stable `client_id`; the daemon deduplicates retries within the group and sender scope.
- The Web outbox reconciles immediately from the HTTP response and again from SSE/reconnect catch-up, so a missing SSE frame does not leave a permanent optimistic duplicate.
- Local runtime delivery submits message text and Enter atomically under a per-session input lock, preventing status probes from interleaving with user messages.
- Messages for stopped or temporarily unavailable actors remain unread and are replayed after the actor starts.
- Runtime delivery uses bounded retry for transient failures and suppresses concurrent duplicate delivery of the same event.
- Actor delivery includes the complete event id, reply/priority requirements, refs, relay origin, and attachment access instructions.

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
| web_model | ChatGPT Web conversation | ChatGPT Web conversation with an MCP-capable GPT-5.x session; GPT-5.x Pro is advisory-only and has no reliable CCCC local access |
| custom | Any command | Any command |

These entries show stable runtime entrypoints or surfaces, not every runtime-specific launch flag. CCCC applies launch defaults automatically and actor/profile commands can be reviewed or customized in settings.

CCCC first-class runtime support is the named runtimes above. `custom` remains the manual fallback for any other command.

### Setup Commands

```bash
cccc setup --runtime claude   # Configure MCP (auto)
cccc setup --runtime codex
cccc setup --runtime droid
cccc setup --runtime amp
cccc setup --runtime auggie
cccc setup --runtime grok
cccc setup --runtime kimi
cccc setup --runtime cursor       # Prompt-assisted setup inside Cursor CLI
cccc setup --runtime kilo         # Prompt-assisted setup inside Kilo Code CLI
cccc setup --runtime antigravity  # Prompt-assisted setup inside Antigravity
cccc setup --runtime custom
```

OpenCode is configured in the actor environment. Hermes and custom runtimes
return the manual stdio configuration in the current Rust build.

`web_model` does not use `cccc setup`; create the single `ChatGPT Web Model` actor from the CCCC Web group, then use Web Settings to sign in to ChatGPT, copy its remote MCP URL, and bind one specific ChatGPT conversation.

### Runtime Detection

```bash
cccc doctor        # Environment check + runtime detection
cccc runtime list  # List available runtimes (JSON)
```
