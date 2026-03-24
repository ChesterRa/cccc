# Web UI Guide

The CCCC Web UI is a mobile-first control plane for managing your AI agents.

## Terminology Alignment

This document follows the local glossary:

- `actor` is the live participant shown in the UI
- `profile` is reusable runtime configuration and launch intent
- `attach` sets the group's `authoritative_workspace`
- `status` is an evidence-bound surface
- `resume` is layered and is not automatically proved by one visible status line

## Accessing the Web UI

After starting CCCC:

```bash
cccc
```

Open http://127.0.0.1:8848/ in your browser.

## Interface Overview

The Web UI has these main areas:

- **Header**: Group selector, settings, theme toggle
- **Sidebar**: Group list and navigation
- **Tabs**: Chat tab + one tab per agent
- **Main Area**: Chat messages or terminal view
- **Input**: Message composer with @mention support

## Managing Groups

### Creating a Group

1. Click the **+** button in the sidebar
2. Or use CLI: `cccc attach /path/to/project`

Planned next step for template-driven setup:

This section describes intended behavior, not a shipped Web UI screen yet.

- after choosing a project path and template file, Web UI should expose a
  `Resume Recovery` section
- the user can add actor-specific bindings of:
  - actor
  - `session_id`
  - enabled toggle
- those bindings should stay outside the portable template file itself
- when enabled for a `codex` PTY actor, launch should prefer native
  `codex resume <session_id>`
- disabling resume later in actor settings should not erase the stored
  `session_id`

### Switching Groups

Click on a group in the sidebar to switch.

### Group Settings

1. Click the **Settings** icon in the header
2. Configure:
   - Group title
   - Guidance (preamble/help)
   - Built-in automation, rules, and snippets
   - Delivery and messaging defaults
   - IM Bridge settings

## Managing Agents

### Adding an Agent

1. Click **Add Actor** button
2. Choose a runtime (Claude, Codex, etc.)
3. Set actor ID and options
4. Click **Create**

If the UI later exposes reusable profile selection, that should be read as
"link this live actor to a reusable runtime profile", not "replace actor
identity with profile identity".

### Starting/Stopping Agents

- Click the **Play** button to start an agent
- Click the **Stop** button to stop
- Use **Restart** to clear context and restart

For native runtime resume, the intended behavior is:

- launch may resume the runtime's own session when the actor is configured for
  native resume
- CCCC's own preamble/help/system prompt injection flow should remain unchanged

See also: `Actor Native Resume Bindings` and `Codex Session Resume`.

### Viewing Agent Terminal

Click on an agent's tab to see its terminal output.

## Messaging

### Sending Messages

1. Type in the message input at the bottom
2. Press `Ctrl+Enter` / `Cmd+Enter`, or click Send

### @Mentions

Type `@` to trigger autocomplete:

- `@all` - Send to all agents
- `@foreman` - Send to the foreman
- `@peers` - Send to all peers
- `@<actor_id>` - Send to specific agent

### Replying

Click the reply icon on a message to quote and reply.

## Context Panel

The Context panel shows shared project state (v2):

### Presence

Agent runtime status and capsule (short-term memory: focus, blockers, next action).

### Vision

One-sentence project goal. Agents should align with this.

### Overview

Structured project view with manual section (roles, collaboration mode, current focus) and live daemon-computed snapshot.

### Tasks

Multi-level task tree. Root tasks = phases/stages. Child tasks = execution units. Each task has steps and acceptance criteria.

## Settings Panel

Access via the gear icon:

### Automation

- **Built-in Automation**: Configure system-managed follow-ups and collaboration health loops such as unread / reply-required / ACK follow-ups, actor idle alerts, keepalive, silence checks, and help nudges.
- **Rules**: Create scheduled reminders with interval / recurring schedule / one-time schedule.
- **Actions**:
  - `Send Reminder` (normal reminder delivery)
  - `Set Group Status` (operational, one-time only)
  - `Control Actor Runtimes` (operational, one-time only)
- **Snippets**: Reusable message templates managed alongside rules.
- **One-time behavior**: One-time rules auto-complete after firing, then can be cleaned up from completed list.

### IM Bridge

Configure Telegram, Slack, Discord, Feishu, or DingTalk integration.

### Group Space

Configure provider-backed shared memory per group:

- Provider credential (masked metadata only)
- Health check
- Binding (`remote_space_id`, optional auto-create)
- `Sync Now` two-way reconcile button:
  - local `repo/space/` resources -> provider,
  - provider source/artifact projection -> local `repo/space/`
- Ingest/query/jobs controls

For end-to-end setup details, see: `Group Space + NotebookLM`.

### Theme

Switch between Light, Dark, or System theme.

## Mobile Usage

The Web UI is responsive and works well on mobile:

- Swipe between tabs
- Pull down to refresh
- Tap and hold for context menus
- Works in mobile browsers (Chrome, Safari)

## Remote Access

To access from outside your local network:

### Cloudflare Tunnel (Recommended)

```bash
cloudflared tunnel --url http://127.0.0.1:8848
```

### Tailscale

```bash
CCCC_WEB_HOST=$(tailscale ip -4) cccc
```

### Security

Before exposing the Web UI beyond localhost, first create an **Admin Access Token** in **Settings > Web Access**.

Then authenticate once to bootstrap the session cookie:

- Open `http://YOUR_HOST:8848/?token=<access-token>` (or `.../ui/?token=...`) using an Access Token created in Web Access.

After that, you can use the Web UI normally without `?token=...`.

## Related Glossary

- [actor](/reference/glossary/actor)
- [profile](/reference/glossary/profile)
- [attach](/reference/glossary/attach)
- [authoritative_workspace](/reference/glossary/authoritative_workspace)
- [resume](/reference/glossary/resume)
- [status](/reference/glossary/status)

## Change Log

- `2026-03-23`: Added local glossary alignment so Web UI guidance keeps actor identity, reusable profile config, attach authority, and evidence-bound resume/status semantics separate.
