# Web UI Guide

The CCCC Web UI is a mobile-first control plane for managing your AI agents.

## Accessing the Web UI

After starting CCCC:

```bash
cccc
```

Open http://127.0.0.1:8848/ in your browser.

`cccc` is the single owner of the default local app session: it starts the daemon and Web together, and pressing `Ctrl+C` stops both together. If another `cccc` session is already running for the same `CCCC_HOME`, a second `cccc` command will refuse to start instead of silently sharing the old daemon.

## Interface Overview

The Web UI has these main areas:

- **Header**: Group selector, settings, theme toggle
- **Sidebar**: Group list and navigation
- **Tabs**: Chat tab + one tab per agent
- **Main Area**: Chat messages or terminal view
- **Input**: Message composer with @mention support

### Embedded browser views

ChatGPT Web Model, NotebookLM sign-in, and Presentation use the same embedded-browser viewer. The
website always runs in the daemon-owned browser session; changing the viewer does not replace that
browser, navigate it, or change its profile.

- **Page** shows the website content directly and uses the available panel space efficiently. It is
  the default for normal Web Model operation and Presentation.
- **Browser** shows the complete browser window when a safe VNC projection is available. It is the
  default for sign-in and setup surfaces where browser UI or native prompts may matter.

Switching views reconnects only the viewer transport and keeps the current browser session and URL.
On platforms or installations without the VNC capability, **Browser** is unavailable and **Page**
remains active. Neither view emulates the website: it still sees the same daemon-owned browser
process. Web Model and NotebookLM use a real system Chrome/Edge session for sites such as ChatGPT
and Google; Presentation may use its own Chromium runtime. Browser-native UI that is outside the web
page is only visible through **Browser** (or through the physical browser window on platforms that
expose it).

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
   - Built-in automation, rules, and snippets
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

With an empty message input, press `Up` to recall your most recent message in the current Group.
Continue with `Up` / `Down` to browse the already loaded message history. Editing or repositioning
the cursor leaves history mode. Recall restores message text only; recipients, reply context,
attachments, and delivery options remain those currently shown in the composer.

### Message diagrams

In Chat and Inbox messages, a fenced code block labeled `mermaid` is rendered as a diagram after
the message is complete. Use **View source** to inspect or copy the original definition. Invalid or
oversized diagrams fall back to the source automatically; other Markdown surfaces continue to show
Mermaid fences as ordinary code blocks. Flowchart image shapes (`@{ img: ... }`) also remain as
source because Mermaid waits on browser image decoding before completing the diagram and can
otherwise block later message diagrams. Click a rendered diagram, or use **Expand**, to open a
near-fullscreen viewer. The viewer reuses the completed SVG without rendering the diagram again;
small diagrams expand to the available canvas while large diagrams remain scrollable.

### @Mentions

Type `@` to trigger autocomplete:

- `@all` - Broadcast to all agents; use for announcements or urgent shared constraints, not default task dispatch
- `@foreman` - Ask the coordinator to plan, route, or summarize work
- `@peers` - Send to all peers
- `@<actor_id>` - Send to a specific agent for targeted work

For concrete delegated work that needs an owner, done criterion, evidence, handoff, or acceptance trail, use task-backed delegation. In chat it appears as a linked task chip; ordinary messages remain the right path for quick questions and discussion.

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

### Copy Groups

Use **Copy Groups** when you need to duplicate, migrate, or back up a working group.

- **Export group copy** downloads a zip containing durable CCCC group state: ledger history, actors, memory, attachments, automation, and group settings.
- The copy package does **not** include the workspace repository/project files. Copy or clone the workspace separately, then choose the workspace root during import.
- System credentials, browser sessions, provider auth, and live runtime state are excluded. The package still contains user content such as ledger history, memory, and attachments; treat it as sensitive. Imported actors are stopped and the imported group starts idle.
- If a group id already exists, import creates a new copy instead of replacing the existing group.

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

Configure Telegram, Slack, Discord, Feishu, DingTalk, or WeCom integration.

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

### LAN / Private Network

```bash
CCCC_WEB_HOST=0.0.0.0 cccc
```

This keeps localhost access working while also letting other devices on the same network open `http://YOUR_LAN_IP:8848/ui/`.
The Rust launcher also honors the binding saved in **Settings > Web Access**, including the legacy Python `settings.yaml` during migration. Explicit `--host` / `--port` flags still take precedence.

If CCCC is running inside WSL2's default NAT networking, this is the exception: `0.0.0.0` only opens the port inside the Linux VM. For true LAN access from other devices, enable WSL mirrored networking or add a Windows `netsh interface portproxy` rule plus matching firewall allow.

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

The Web Access panel keeps LAN/public `Save`, `Apply now`, and remote-endpoint copying disabled until an Admin Access Token exists. Python and Rust also enforce the same rule at remote start, apply, and listener boundaries, so direct API calls and stale saved settings cannot bypass the panel. Group-scoped tokens do not satisfy this administrator recovery requirement. Switching back to localhost-only remains available so an incomplete remote setup can be recovered safely.

In **Settings > Web Access**, `127.0.0.1` means local-only and `0.0.0.0` means localhost plus your LAN IP on a normal local host. On WSL2 NAT, it still stays inside the VM until Windows networking forwards it outward.

`Save` stores the target binding. If Web was started by `cccc` or `cccc web`, use `Apply now` in **Settings > Web Access** to perform the short supervised restart. If Web is managed by Docker, systemd, or another external supervisor, restart that service instead.

For the default local app flow, prefer restarting from the owning `cccc` session itself: `Ctrl+C` to stop the whole app, then run `cccc` again. That keeps daemon and Web on the same fresh code/runtime.

`Start` / `Stop` are only for Tailscale remote access and do not rebind the already-running Web socket.

CCCC keeps the token policy simple:

- localhost-only: no remote-exposure token prerequisite
- LAN/private network and public URL/tunnel/reverse proxy: an Admin Access Token is mandatory before exposure

`CCCC_WEB_ALLOW_UNAUTHENTICATED=1` is an explicit unsafe listener override for deployments that already enforce a trusted network boundary outside CCCC. It is intentionally not offered as a Web UI toggle.

Then authenticate once to bootstrap the session cookie:

- Open `http://YOUR_HOST:8848/?token=<access-token>` (or `.../ui/?token=...`) using an Access Token created in Web Access.

After that, you can use the Web UI normally without `?token=...`.

The query token is only a session-bootstrap transport; it does not widen the
token's permissions. A token scoped to selected Groups receives global stream
metadata only for those Groups, and the global stream never carries message
content. Full event content remains on the per-Group stream and is subject to
the same scope check. Administrative capability changes require an Admin token.
