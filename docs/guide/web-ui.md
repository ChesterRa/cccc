# Web UI Guide

The CCCC Web UI is a mobile-first control plane for managing your AI agents.

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

### Switching Groups

Click on a group in the sidebar to switch.

### Group Settings

1. Click the **Settings** icon in the header
2. Configure:
   - Group title
   - Guidance (preamble/help)
   - Automation rules and engine policies
   - Delivery and messaging defaults
   - IM Bridge settings

## Managing Agents

### Adding an Agent

1. Click **Add Actor** button
2. Choose a runtime (Claude, Codex, etc.)
3. Set actor ID and options
4. Click **Create**

### Starting/Stopping Agents

- Click the **Play** button to start an agent
- Click the **Stop** button to stop
- Use **Restart** to clear context and restart

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

The Context panel shows shared project state:

### Vision

One-sentence project goal. Agents should align with this.

### Sketch

Execution plan or architecture sketch. Static, no TODOs.

### Milestones

Coarse-grained project phases (2-6 total).

### Tasks

Detailed work items with steps and acceptance criteria.

### Notes

Lessons learned, discoveries, warnings.

### References

Useful files and URLs.

## Settings Panel

Access via the gear icon:

### Automation

- **Rules**: Create reminders with interval / recurring schedule / one-time schedule.
- **Actions**:
  - `Send Reminder` (normal reminder delivery)
  - `Set Group Status` (operational, one-time only)
  - `Control Actor Runtimes` (operational, one-time only)
- **One-time behavior**: One-time rules auto-complete after firing, then can be cleaned up from completed list.
- **Engine Policies**: Configure built-in nudges (reply-required, attention ACK, unread), actor idle, keepalive, silence, and help nudges.

### IM Bridge

Configure Telegram, Slack, Discord, Feishu, or DingTalk integration.

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

Always set `CCCC_WEB_TOKEN` when exposing the Web UI:

```bash
export CCCC_WEB_TOKEN="your-secret-token"
cccc
```

Then authenticate once to bootstrap the session cookie:

- Open `http://YOUR_HOST:8848/?token=your-secret-token` (or `.../ui/?token=...`)

After that, you can use the Web UI normally without `?token=...`.
