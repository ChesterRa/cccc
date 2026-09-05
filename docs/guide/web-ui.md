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
- **Sidebar**: Group list, navigation, and the persistent Codex Voice control
- **Tabs**: Chat tab + one tab per agent
- **Main Area**: Chat messages or terminal view
- **Input**: Message composer with @mention support

### Codex Voice (Experimental)

The **Codex Voice** control below the Group list is a global assistant entry, not a Group roster item.
It has two stable actions: the main control always opens the Voice console, and the adjacent
microphone/square control starts or stops live audio. The same meanings are preserved in the
collapsed sidebar and active-call mobile bar. Settings, mute, and the Analyst terminal are not
separate Dock actions.

The Voice console is one operational workspace. Call state, audio settings, mute, and Stop are in its
top audio-control area. The left pane shows the current spoken turn and accumulates provider fragments
until the authoritative final transcript arrives; the right pane embeds the selected Voice Analyst
runtime's genuine writable TUI. On narrow screens, **Conversation** and **Voice Analyst** are tabs in the same console.
Speaking voice, microphone, and (where supported) speaker choices expand inline rather than opening a
second settings modal. The host must already be signed in through `codex login`, and the browser must
allow microphone access. That host login belongs only to the Realtime provider call.

The two visible roles are one product flow:

- **Realtime Voice** owns the low-latency spoken conversation, interruption, and clarification.
- **Voice Analyst** is the backing managed agent session. It handles file inspection, tools, current CCCC facts,
  and substantial analysis, then returns useful progress and the final result to the same spoken
  conversation. Under local user authority it can list all Groups and query an explicitly named Group.
  Codex, Claude Code, Grok Build, OpenCode, and Kilo are the currently admitted Analyst runtimes.

CCCC starts the Analyst in one stable neutral directory at
`CCCC_HOME/state/codex_voice/analyst-workdir/`. It is not a repository, Working Group, implicit MCP
target, or authorization boundary. Voice therefore starts without a selected Group or attached
repository and never changes identity when the sidebar selection changes. The Analyst's CCCC MCP
session has global user authority and the full tool catalog, but it receives no default `group_id`;
every Group, Actor, task, ledger, or repository operation must resolve and pass an explicit target.
Repository modification remains work for the target Group's Foreman or peer rather than work rooted
in the neutral Voice directory.

CCCC reads the existing Codex credential only in the native process that creates the provider call;
the browser receives the WebRTC answer and bounded session events, not the credential. Use the
console header to mute the microphone, resume browser-blocked playback, or stop the call. The
Analyst uses the same trusted-local YOLO boundary as the corresponding CCCC Actor runtime. Its
authentication and model provider are independent from the Realtime login: the default launch
inherits the normal Codex configuration, while Custom or Runtime Profile settings can select Codex,
Claude Code, Grok, OpenCode, or Kilo and configure that runtime's provider home, model provider, or API key without
changing the Realtime credential path. The two sides exchange only
delegations, progress, and results through the CCCC controller.

The Analyst **Runtime Setup** uses the same Custom / Runtime Profile model as Actor editing. Custom
mode accepts a direct Codex, Claude Code, Grok, OpenCode, or Kilo command (including Kilo's official Windows npm entrypoint), supported provider/model options, and
write-only private environment values, and it can save that complete configuration as a reusable
Runtime Profile without revealing secrets to Web. Runtime Profile mode resolves a compatible Codex,
Claude Code, Grok, OpenCode, or Kilo Profile's command and private environment from the shared profile store;
the Actor-only submit field does not alter the Voice host. Historical Profiles with a string command are accepted
and normalized to the canonical argument array when next saved.

For Codex, CCCC removes conflicting host flags and pins app-server,
`shell_environment_policy.inherit=all`, `approval_policy=never`,
`sandbox_mode=danger-full-access`, the neutral Voice workspace, and the global CCCC MCP binding. For
Grok, CCCC accepts a direct root command with supported global provider/model options, then owns the
private leader socket, ACP controller, native TUI attachment, YOLO policy, workspace, and per-session
MCP binding. Grok subcommands, wrappers, prompt tails, and user-owned topology/session flags are
rejected rather than routed through a second PTY path. For OpenCode and Kilo, CCCC owns the ACP process,
generation-scoped authenticated loopback backend, session/load boundary, one-time permission policy,
per-session MCP injection, and native `opencode attach` or `kilo attach` command. It preserves supported model, agent,
pure-mode, and logging choices. Kilo's official Windows npm entrypoint uses Node and the installed
launcher for both ACP and TUI. Custom wrappers, subcommands, prompt tails, and user-owned topology
or session flags likewise fail explicitly. CCCC validates the live ACP and authenticated backend
behavior during startup instead of maintaining a separate legacy-version compatibility branch.
These CCCC-owned values cannot be overridden.

For Claude Code, CCCC requires version 2.1.259 or newer and owns the Agent View
background session, authenticated control channel, transcript lifecycle,
native `claude attach`, session identity, MCP binding, autonomy, and resume
arguments. Supported model, effort, agent, tool, plugin, and settings options
remain configurable. Private Profile environment is merged into a protected
CCCC settings file so the background worker receives it without exposing raw
values in the job record or Web API. Claude wrappers, renamed binaries, prompt
tails, print mode, remote-control mode, and user-owned background/session
arguments fail explicitly; there is no Hook, PTY-paste, or `claude -p` fallback.

Each runtime's controller and native TUI are separate processes but one configured Analyst. Codex's
app-server and remote TUI receive the same executable, model, Profile, supported `-c` overrides, YOLO
policy, and private environment. Claude's Agent View controller, transcript follower, and native TUI
share one background session and one effective launch identity. Grok's leader, ACP client, and TUI share one resolved provider
configuration, private environment, and exact session; CCCC adds topology arguments only to the
processes that accept them. OpenCode and Kilo each use a stdio ACP controller and authenticated native TUI attach sharing
one provider backend and exact session. CCCC injects the scoped MCP binding when it creates the
managed session,
so the model reached from either client has the same CCCC tools. Opening the embedded terminal
therefore observes and controls the existing Analyst session instead of starting another model
conversation.

If Realtime Voice reports a provider error, its error code is shown separately
from Analyst failures. The browser console retains a bounded error explanation;
CCCC's server log records only bounded code/type/event/parameter identifiers and
the call generation, not the explanation or conversation content. A stopped
audio call does not discard the warm Analyst session. Provider diagnostics do
not retry requests or change the existing disconnect policy.

Ordinary Codex, Claude Code, Grok, OpenCode, and Kilo Actors use the same runtime-specific managed adapter
as Voice Analyst and always attach the Runtime's native writable TUI. Actor controllers
are bound to a concrete Group and Actor MCP identity, while Voice Analyst uses the global user
identity and its neutral workspace. These are separate roles over the same adapter, not divergent
session paths. Actor messages enter the native TUI immediately, leaving queue-versus-steer
behavior to the receiving Runtime. Realtime Voice decides whether a spoken request needs an Analyst,
but it does not schedule the Analyst process. CCCC attempts each correlated delegation immediately:
Codex can append an explicit in-flight correction through exact-turn steering, while the other
admitted Runtimes receive the exact input through their verified native terminal and own whether it
steers or queues. Busy state alone never drops or delays a delegation.
All admitted Runtime commands fail explicitly when they cannot join the managed
session instead of falling back to a second execution path.

Custom non-secret settings live in `settings.yaml`; custom private values live in
`CCCC_HOME/state/secrets/codex_voice_analyst.json`. Profile secrets remain owned by the Runtime
Profile store, and Web receives key names only. Applying a valid setting while idle restarts the
managed host and resumes the same materialized Analyst session. Runtime settings cannot change while
Realtime Voice is connected. If the Analyst still has active or queued work after voice stops, Web
shows that distinct state and asks once before stopping the old managed host, discarding the unfinished
result, and applying the new settings. An effective Runtime Profile
command/environment change replaces the warm host at the next voice start while retaining the same
session when its runtime identity is unchanged; name, submit, and capability-only edits do not
restart this host. Changing the selected runtime, any effective Claude launch setting or private environment,
`CODEX_HOME`, `GROK_HOME`, OpenCode's storage/config
roots (including `OPENCODE_DB`), Kilo's storage/config roots (including `KILO_DB`), `HOME`, or `USERPROFILE` changes that identity and starts a new session after explicit
confirmation in Custom mode. A failed candidate launch restores the prior settings and runtime, but
work explicitly discarded before the switch cannot be recovered. Browser-local audio choices apply
to the next call.

Analyst results are returned immediately, including updates arriving while Voice is speaking.
If Realtime does not confirm receipt within 30 seconds, the console shows a notice without
disconnecting or replaying the update. Results exceeding the 32 KiB Voice limit are reported
explicitly: use the Analyst terminal for the full output or ask for a shorter summary. Neither
notice stops the Analyst. Provider receipts confirm receipt, not that every detail was spoken.

Realtime Call and Voice Analyst have separate lifecycles. Stopping voice disconnects browser audio
and releases the microphone lease, but keeps the Analyst warm. Ongoing Analyst or linked
Actor work continues, and its result returns to the Analyst session without waking audio; only a
result from the exact still-active call generation may become speech. A later call reuses the warm
Analyst. The console embeds the selected runtime's genuine TUI for that same Analyst session using the existing
terminal transport; it stays available after the call stops and does not create an Actor or a second
Analyst. Codex creates its resumable rollout lazily, so its terminal appears when the first real
investigation starts; Claude, Grok, OpenCode, and Kilo managed TUIs can attach as soon as their sessions are ready. CCCC does not
create a token-consuming placeholder task. **New Analyst session** is the explicit reset action and is
available only after the call stops. Raw local paths, provider session IDs, loopback endpoints, and
external terminal commands are deliberately not part of the product surface.

The embedded native TUI remains writable while managed work is active. The receiving Runtime owns
whether new terminal input steers the current turn or queues for the next one; CCCC registers exact
Voice input before writing it and uses the Runtime's authoritative echo to associate the result with
the correct delegation. OpenCode Analyst results remain available to Realtime Voice when the provider omits a
live ACP prompt echo: CCCC acknowledges the request from the matching user message on OpenCode's
authenticated backend stream, then uses the ACP response as the exact completion fence. A completed
Voice turn with no result is reported as failed instead of silently returning an empty answer.

The session manager keeps at most one Codex Voice call and one warm Voice Analyst per interactive
Web host. The normal launcher permits one such Web process per `CCCC_HOME`, while the call also uses
the existing daemon-owned global recording lease shared with Voice Secretary. Hiding the details
panel, switching Groups, or leaving the tab in the background does not stop or retarget the call.
Application-level heartbeats keep the otherwise idle event WebSocket alive through ordinary proxies.
Explicit cross-Group CCCC queries do not retarget the neutral Analyst workspace. Use **Stop voice**
to disconnect explicitly.
Closing or reloading the owning page or losing its browser transport ends only the live call and
releases the recording lease. Stopping Web also stops the warm Analyst process and embedded TUI while
retaining only the exact materialized neutral-workspace thread receipt in global runtime state. The
next call attempts to resume that thread; a legacy repository-bound receipt is deliberately replaced
with a fresh neutral-workspace thread and a visible migration warning. A resume failure likewise
starts fresh with a visible warning rather than silently changing scope.
This Experimental surface does not automatically reconnect a browser audio call. Current live
evidence covers Linux x86_64/WSL2 and isolated Chrome desktop plus 390px responsive journeys; macOS,
native Windows, mobile browsers, and physical-device audio quality remain unclaimed.

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

## Performance behavior

- Hidden tabs release their group event streams immediately. Returning to the tab reconnects and
  catches up from the last event cursor, so several open tabs do not exhaust the browser's
  per-origin connection pool.
- Hover-prefetched group data is reused when that group is selected; group bootstrap does not issue
  a second actors read for the same transition.
- Production Web responses negotiate Brotli or gzip compression. Voice Secretary code and locale
  resources are loaded on demand instead of being part of every initial page load.

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

Recipient chips are one-shot: a successful send clears the selection, and switching Groups does not
restore a previous manual recipient. Unsent message text and attachments still remain as per-Group
drafts.

Messages larger than 64 KiB after UTF-8 encoding are sent as UTF-8 text attachments for
same-group and remote Group Bridge targets. Local cross-group text remains inline because its two
local ledgers cannot share one attachment path; the bounded daemon IPC limit covers that JSON route.
This applies to typed, pasted, dictated, suggested, and restored drafts. Slash commands still require
inline text and therefore reject an automatically attached oversized body.

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

- **Built-in Automation**: Configure bounded Mail/reply notices and collaboration health loops such as actor idle alerts, keepalive, silence checks, and help nudges.
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
The native launcher also honors the binding saved in **Settings > Web Access**, including the 0.4.35 `settings.yaml` during migration. Explicit `--host` / `--port` flags still take precedence.

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

Direct browser access through `localhost`, `127.0.0.1`, or `::1` is passwordless and does not
create an Access Token. CCCC grants that request an in-memory local administrator principal only
when the reconstructed browser-facing origin is loopback. Unsafe writes and WebSockets must also
carry the exact same loopback Origin; non-local proxy client addresses are rejected. This local
principal is never persisted and is not valid through LAN, Reach, a public URL, or a reverse proxy.

Before exposing the Web UI beyond localhost, first create an **Admin Access Token** in **Settings > Web Access**. With no administrator token, non-local clients receive only the UI shell and health/session guidance; protected APIs and business WebSockets remain locked, while direct loopback access keeps the passwordless local principal described above. Read the one-time bootstrap code from `~/.cccc/web_bootstrap_token` on the CCCC host and enter it only when creating the first administrator token; the file is mode `0600` on Unix and is deleted after successful use.

The Web Access panel keeps LAN/public `Save`, `Apply now`, and remote-endpoint copying disabled until an Admin Access Token exists. The native daemon and Web boundary enforce the same rule at remote start, apply, and listener boundaries, so direct API calls and stale saved settings cannot bypass the panel. Group-scoped tokens do not satisfy this administrator recovery requirement. Switching back to localhost-only remains available so an incomplete remote setup can be recovered safely.

In **Settings > Web Access**, `127.0.0.1` means local-only and `0.0.0.0` means localhost plus your LAN IP on a normal local host. On WSL2 NAT, it still stays inside the VM until Windows networking forwards it outward.

`Save` stores the target binding. If Web was started by `cccc` or `cccc web`, use `Apply now` in **Settings > Web Access** to perform the short supervised restart. If Web is managed by Docker, systemd, or another external supervisor, restart that service instead.

For the default local app flow, prefer restarting from the owning `cccc` session itself: `Ctrl+C` to stop the whole app, then run `cccc` again. That keeps daemon and Web on the same fresh code/runtime.

`Start` / `Stop` are only for Tailscale remote access and do not rebind the already-running Web socket.

CCCC keeps the token policy simple:

- localhost-only: direct loopback browser requests are passwordless and use a non-persistent local administrator principal
- LAN/private network and public URL/tunnel/reverse proxy: an Admin Access Token is mandatory before exposure

`CCCC_WEB_ALLOW_UNAUTHENTICATED=1` is only an unsafe listener override; it never grants API authorization or bypasses first-admin bootstrap. Plain HTTP manual LAN exposure also requires `CCCC_REMOTE_ALLOW_INSECURE=1`; prefer an HTTPS reverse proxy, tunnel, or encrypted overlay. Neither override is offered as a Web UI toggle.

CCCC adds `frame-ancestors 'self'`, `SAMEORIGIN`, `nosniff`, `no-referrer`, a restrictive permissions policy, and HSTS on HTTPS responses. Supervised CCCC Web processes trust reverse-proxy forwarding headers automatically only while the effective listener is loopback. A supervised LAN/wildcard listener or externally managed reverse proxy must explicitly set `CCCC_WEB_TRUST_PROXY_HEADERS=1` and must overwrite—not append—client-supplied `Forwarded` and `X-Forwarded-*` headers. Direct public listeners should leave this flag unset.

Enter the Access Token in the Web sign-in form. CCCC validates it through the
`Authorization` header and establishes an HttpOnly, `SameSite=Lax` session
cookie. The cookie has a rolling 30-day lifetime and is refreshed when the Web
session is checked, avoiding repeated token entry after mobile browsers reclaim
a tab. The temporary header token is removed from browser session storage after
the cookie is established. Access tokens are not accepted in ordinary API, SSE,
or WebSocket query strings and should never be placed in shared URLs.

Reach follows the same rule. Its status payload exposes only a tokenless public
address. Clicking **Open Web** or **Copy Admin Link** asks the local authenticated
Rust Web session for a 120-second, one-time exchange code bound to that Reach
origin. The public endpoint consumes the code once, establishes the HttpOnly
cookie, and redirects to a clean `/ui/` URL.

Cookie-authenticated Rust Web writes require an exact allowed `Origin`, with a
same-origin `Referer` accepted only as a fallback. This check is independent of
CORS and blocks same-site sibling domains from submitting state-changing forms.

#### Reverse proxy headers

When a reverse proxy terminates HTTPS or exposes CCCC under another host, it
must overwrite the browser-facing host and protocol headers. These values are
used by every browser WebSocket (terminal, Voice Secretary, projected browser)
and by Cookie-authenticated write protection:

```nginx
proxy_http_version 1.1;
proxy_set_header Host $host;
proxy_set_header X-Forwarded-Host $host;
proxy_set_header X-Forwarded-Proto $scheme;
proxy_set_header Upgrade $http_upgrade;
proxy_set_header Connection "upgrade";
```

Do not pass through client-supplied `X-Forwarded-*` values. The trusted proxy
must overwrite them. CCCC also accepts RFC 7239 `Forwarded` with `host` and
`proto`, and handles comma-separated multi-proxy `X-Forwarded-*` chains by
using the first browser-facing value. A mismatch is rejected with
`origin_not_allowed` for WebSockets or `csrf_origin_invalid` for Cookie writes;
the server log records both the received and reconstructed origins.

A token scoped to selected Groups receives global stream
metadata only for those Groups, and the global stream never carries message
content. Full event content remains on the per-Group stream and is subject to
the same scope check. Administrative capability changes require an Admin token.
