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
cccc_message_reply(event_id="evt_xxx", text="Reply", insight="The original framing may be hiding a better route.")
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
[cccc] user → peer-a [event_id=<full event id> sent=2026-10-28T01:45:03Z]: Please implement the login feature
[cccc] user → peer-a (reply:evt_abc) [event_id=<full event id> sent=2026-10-28T01:46:10Z]: OK, please continue
```

The full current `event_id` is the value passed to
`cccc_message_reply(event_id=...)`. The short `reply:` marker only correlates the
message with its parent; ordinary delivery omits non-actionable storage mode and
full parent metadata. `sent` is the message's original timestamp as recorded in
the ledger (UTC). A runtime may show queued input long after it was accepted, so
an old `sent` time marks an instruction that later messages may already have
superseded.

### Questions to the User

When an agent needs the user's decision, it sends a `request_reply` message to
`user` with a `question`: a prompt, two to four options with their
consequences, and optionally a recommendation with its reason. The user picks
an option or writes their own answer in place, under the message.

- The daemon generates the message text from the question, so IM, the terminal
  and older clients still show the whole question, and an agent receives the
  answer as an ordinary reply such as `Selected B: <label>`.
- A question without options asks for a value only the user knows; the answer
  is delivered in the user's own words.
- Requests without a question can also be answered in place, with text.
- The CCCC Protocol lines of every Actor prompt and delivery, and the MCP
  tool descriptions, tell agents to ask this way whenever they need the
  user's decision or information, unless the user asked for another channel,
  and not to ask what they can responsibly decide themselves.
- The daemon accepts an answer only while the request is still open and only
  when its sender is provably the same Actor that asked. A requester that was
  removed, added again under the same ID, or cannot be confirmed is reported,
  and the answer is not sent to whichever Actor now has that ID.
- The open requests to the user are listed per Group and counted across Groups
  from the daemon's ledger, not from what a client happens to have loaded.
  Requests relayed from another Group or a Connect instance are listed and
  counted, and answered with an ordinary reply.
- Recording an answer closes the request; whether the agent has received it is
  reported like any delivery. A paused or stopped Group records answers
  without delivering them, and answering neither resumes the Group nor
  re-enables a disabled Actor.
- In the Web UI, while requests to the user are open in a Group, a Waiting
  for you filter appears first on its messages page with their number and
  lists all of them; it stays while it is being viewed. The Requests filter
  lists every request, answered or not, between any members. The sidebar
  shows each Group's count, including stopped and archived Groups. In the
  terminal interface, a Group's page has a Waiting for you row. A count that
  may be out of date is marked as such, one that cannot be confirmed is shown
  as unknown rather than zero, and neither appears in read-only (exhibit)
  mode.

### System Notifications

A browser can subscribe to Web Push notifications in the Web UI's settings.
While CCCC's Web service runs, it then tries to notify that browser of each
message an agent sends to the user and of each request waiting for the user's
answer. A notification can appear with no CCCC page open, and opening one
tries to show its message. Whether and when a notification appears is up to
the push service, the browser and the device. Setup steps and what each
platform needs are in the [Web UI guide](../guide/web-ui.md#system-notifications).

- What is announced follows the Inbox rules: a `chat.message` that names
  `user` (or `@user`) explicitly, from someone other than the user, and not
  the source copy of a send to another Group. `@all` does not count.
- Only what is written after a browser subscribed is announced to it, in the
  best-effort way described here. Subscribing records the newest event of each
  Group the signed-in token can see, and only later events are announced;
  requests already waiting are under Waiting for you.
- Notifications follow the Web service: nothing is announced while it is
  stopped, and what was written meanwhile is not announced when it starts.
- A message is sent once, best effort. A request is sent once too, unless a
  push service does not take it (busy, unreachable or too slow to answer):
  then it is offered again with backoff, while the Web service runs and the
  request is open, up to 10 per browser (older ones are dropped and logged).
  One that a push service took without confirming in time can arrive twice. A
  request already answered or withdrawn when it is read is not announced, and
  one closed, or whose Group was deleted, before its next try is not offered
  again; one closed just as it is sent can still be announced.
- Browsers are sent to in parallel, each one's notifications in order, and
  the next notifications wait until every browser's are sent. A push service
  that does not answer within 20 seconds, says it is busy, or fails is left
  out of that browser's notifications until its next try, a minute later and
  twice as long after each failure in a row, up to 30 minutes: messages
  meanwhile are not sent to it, and requests wait for that try. Each browser
  waits on its own push service only. One that answers slowly delays the next
  notifications for every browser.
- A push service can replace or expire a browser's subscription. That
  browser then gets no more notifications, and its settings no longer show
  them as enabled; enabling notifications again in that browser restores
  them.
- Disabling removes the browser's subscription even when the server cannot be
  told; the server forgets it once the push service refuses the next
  notification to it. Signing out of a browser in Web Access first asks the
  server, for up to 3 seconds, to stop notifying that browser for the account
  signing out. Only that account's subscription is removed: the browser keeps
  its own, and another account in the same browser profile keeps its
  notifications. If the server cannot be told, the account's notifications to
  that browser continue until its subscription is removed there.
- History is not announced. A Group first seen while the Web service runs is
  announced from after its import when that is among its newest 128 events;
  otherwise from its start if it has at most 64 events by then, and from its
  next event if it has more.
- A push service is asked to keep a notification for up to a day while the
  device cannot be reached. It can deliver it late or drop it, and the
  device's notification settings decide whether it is shown; it is not sent
  again. Messages stay in the chat, and open requests under Waiting for you.
- A subscription belongs to the access token the page was signed in with, or
  to the local administrator on this computer. Before every send, the token is
  checked again, along with the Groups it may see; a deleted token's
  subscriptions get nothing more and are removed when the next notification
  would go to them. Subscribing the same browser again keeps its place; under
  another token, it moves to that token and starts again.
- Every notification is a declarative push message (`"web_push": 8030`, W3C
  Push API) linking to its message. Safari 18.5 or later on macOS, and Home
  Screen web apps on iOS and iPadOS 18.4 or later, show it themselves and
  try to open that link when it is opened, loading CCCC at the message. A
  suspended Home Screen app can come forward without following it; the
  message is then in its Group's chat (a request also under Waiting for
  you). Where the link replaces an open CCCC page, as in a Home Screen web
  app, an unsent draft there is lost.
- Other browsers hand the same message to CCCC's service worker. Opening the
  notification then brings an open CCCC page forward and shows the message
  there, or opens a page at it when none is open; other CCCC pages, such as
  the Capability Center, are passed over. A page the browser reports as open
  is not reloaded, so a draft there is kept. A page that wakes from the
  background still goes to the message if it hears of the click within about
  ten seconds; an older click arriving after a newer one is ignored. When the
  page does not answer in time, or the browser does not report its pages in
  time, the page stays as it is and the message is in its Group's chat (a
  request also under Waiting for you). A browser that leaves an open page
  unreported, as older iOS versions can, may open or reload a page.
- The notification carries the Group's title, the sender and up to 140
  characters of the question's prompt or the message text, which a locked
  screen may show. Payloads are encrypted for the browser (RFC 8291) and signed
  with a VAPID key (RFC 8292) that the Web service makes the first time a
  browser subscribes.
- The key and the subscriptions are kept in `CCCC_HOME/web_push.json`, created
  with owner-only permissions on Unix; on Windows it is as private as the
  permissions of `CCCC_HOME`. An endpoint is the capability to notify one
  browser, so the API reports it only as a SHA-256 hash and logs never show
  it. An owner keeps at most 16 subscriptions; binding a 17th removes the one
  bound longest ago.
- Only Google, Mozilla, Apple and Microsoft push service hosts are accepted
  (for Google, also the endpoint that Chrome Beta, Dev and Canary use),
  over HTTPS on port 443 with the certificate checked and redirects refused.
  Requests follow the system proxy settings; the address a host name resolves
  to is not checked.
- If two Web processes run for one `CCCC_HOME`, only one sends; the other
  takes over within a minute after it stops, and as after a restart, what was
  written in between is not announced. Exhibit (read-only) mode neither shows
  these settings nor sends.
- While `web_push.json` cannot be read nothing is sent, and the log says so;
  what is written meanwhile, or just after it can be read again, may not be
  announced. A Group whose ledger cannot be read when notifications first find
  it may miss notifications for what is written to it before it can be
  followed. A subscription whose stored page origin is invalid or not in
  canonical form is neither sent to nor shown as enabled, and the log says
  so; enabling notifications again in that browser repairs it.
- Web API: `GET /api/v1/push/config` (the public key, or `null` before one is
  made; the hashes of the caller's subscriptions; and `sender`: `here`,
  `elsewhere` for another Web process, `starting`, or `unavailable` when
  nothing can send from this process), `POST /api/v1/push/key` (makes the key
  once), `PUT /api/v1/push/subscription` (the browser's `PushSubscription`
  JSON plus `application_server_key`; refused with `push_sender_unavailable`
  when nothing can send) and `DELETE /api/v1/push/subscription`
  (`{"endpoint": …}`, removes only the caller's own subscription). `PUT` needs
  an `Origin` or `Referer` that matches the request's `Host` (or trusted
  proxy headers) or an origin in `CCCC_WEB_CORS_ORIGINS`; notification links
  lead back to it. A client that sets these headers itself chooses the
  origin its own notifications link to.

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

## Shared File Memory

`cccc_memory` reads and writes the Group's local Markdown memory and searches it
by lexical relevance. Use `get` to inspect the source behind a search result.
The local backend does not perform embedding search; the accepted `vector_weight`
argument does not affect its ranking.

The optional `cccc_memory_admin` helpers retain their existing names and response
formats. Their current limits matter when choosing an operation:

- `index_sync`, with either `scan` or `rebuild`, reports file and nonempty-line
  counts. It does not build a persistent index or start a file watcher. Searches
  read the current Markdown files directly.
- `context_check` uses a character-based token estimate, not the selected
  Runtime's tokenizer or context limit.
- `compact` returns a bounded excerpt by default; it can omit facts later in the
  supplied history. Use `return_prompt=true` to obtain a summarization prompt for
  the calling model. It does not call a model or compact the Runtime's session.

Group coordination state, repository documents and external NotebookLM bindings
retain their separate purposes. Using local memory does not require NotebookLM.

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
- Imported groups start idle with actors stopped. If the packaged group id already exists, import creates a new copy and does not steal the existing workspace default mapping. An id still registered, as while a Group is being deleted, counts as existing.
- An import is assembled out of sight and appears complete, with its history and import marker, before anything can be written to it; nothing sent to the new Group is lost to the import. An interrupted import may leave a hidden `.importing-*` directory in `CCCC_HOME/groups`, which can be removed once no import is running. An import that fails after claiming its Group id but before publishing it keeps that id's directory, and its error names it. The import did not publish it, but a process still writing to an earlier Group with that id may have written into it, so check it before deleting it. That id then counts as taken, and importing again makes a new copy. If a complete import cannot be registered, the error names where its data is kept; that Group is whole and may already hold new messages, so do not delete it blindly.
- Copy Groups replaces the former group-template Web path; durable group features should be carried by Copy Groups unless explicitly blacklisted as unsafe or runtime-only.

### MCP Management Surface

```text
cccc_automation(action=state)
cccc_automation(action=manage, actions=[create_rule|update_rule|set_rule_enabled|delete_rule|replace_all_rules, ...])
```

`cccc_automation` is how agents manage reminders. A `manage` request saves all of its actions or none of them:
- A peer can manage its own reminders, which reach only itself. A rule it creates without `scope`, `owner_actor_id` or `to` is such a reminder.
- The foreman can manage every notify reminder, including peers' reminders, and can replace the whole list.
- Scheduled operations (`group_state`, `actor_control`) stay with the user in the Web UI. An agent's request that would change, disable, remove or reorder one is refused.

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
| cline | `cline` | Cline CLI native TUI |
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
| kimi | `kimi` | Kimi Code |
| opencode | `opencode` | OpenCode |
| grok_web_model | Grok Bot conversation | Shared Grok login and connector, per-Actor Bot URL and per-call routing credential; no pairing handshake |
| web_model | ChatGPT Web conversation | ChatGPT Web conversation with CCCC MCP access; optional experimental GPT Pro delivery attaches a tiny blank PNG but does not select the model or guarantee connector availability |
| chatgpt_dots | ChatGPT Dots | One Dot bound to one Actor/Group; asynchronous MCP Events delivery and task-scoped remote MCP, using the shared ChatGPT connector |
| custom | Any command | Any command |

These entries show stable runtime entrypoints or surfaces, not every runtime-specific launch flag. CCCC applies launch defaults automatically and actor/profile commands can be reviewed or customized in settings.

CCCC first-class runtime support is the named runtimes above. `custom` remains the manual fallback for any other command.

### Setup Commands

```bash
cccc setup --runtime claude   # Report CCCC-owned per-session MCP
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

`web_model` does not use `cccc setup`; create a `ChatGPT Web Model` Actor from the CCCC Web group, then use Web Settings to sign in to ChatGPT, copy its remote MCP URL, and bind one specific ChatGPT conversation for that Actor. For Grok, select `Grok Bot Web Model`, configure the shared Grok connector and save each Actor’s Bot URL; see [Grok Bot setup](/guide/grok-web-model-runtime).

### Runtime Detection

```bash
cccc doctor        # Environment check + runtime detection
cccc runtime list  # List available runtimes (JSON)
```
