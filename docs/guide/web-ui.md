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

- **Header**: Group title and controls, work view switch, and settings menu
- **Sidebar**: Group list, navigation, and the persistent Codex Voice control
- **Main Area**: Group message history or a paginated terminal view
- **Input**: Message composer with @mention support

### Group message and terminal views

On desktop, Group editing, search, context and runtime controls stay directly in the header.
**Group connections** is available under the current Group’s settings and in each Group’s sidebar **⋮** menu, including remote Groups
in CCCC Connect. The settings entry manages the current Group; the sidebar menu manages the Group
whose menu you opened. Only incoming invitations ask which local Group should accept
the connection.
**Settings and more** collects theme, text size, language, account and the full settings entry.
Appearance choices use application menus for theme, text size and language. Selecting an
option keeps the containing menu open and returns focus to its trigger. Arrow keys navigate
options; Escape closes only the inner choice menu. Narrow headers use the existing overflow
sheet with the same choices and access rules; choice panels are portalled above its scroll area.
Message identities, recipients, references, timestamps and actions follow the text-size preference.
Reading controls share the application theme; embedded PDFs, web pages, images and terminal
ANSI content retain their own colors. Image and diagram viewers keep zoom buttons separate
from native wheel scrolling and support dragging the enlarged content.

In Group settings, refreshing data or saving a different section preserves edited fields and
updates fields you have not changed. Delivery, Automation, Messaging and Transcript display
save results in the current settings view; a failed save leaves your changes available for retry.
Notebook refresh also preserves a pending notebook choice. Changing Groups starts from that
Group's saved settings. Closing settings still discards unsaved form edits.
Web Access refresh preserves the selected access goal together with edited connection fields.
Saving or resetting one Guidance document updates that document without reloading the other
editor; **Discard changes** remains explicit.

The **Group status button** combines the status dot with run controls, without a dropdown arrow.
When space allows, the header also shows the state text; narrow headers keep a compact button.
The sidebar keeps a passive status dot and places run actions in each Group’s **⋮** menu
and context menu. These actions target that exact Group without switching views. **Start Group** starts enabled
Agents. **Pause message delivery** pauses CCCC delivery; work already in progress may continue.
**Resume running** resumes delivery, starting enabled Agents if none are running. **Stop Group**
stops running Agents while preserving their enabled settings. Pending operations disable run and
delete menu actions to prevent duplicate submissions. Read-only views retain passive status
indicators. On mobile, use the same header control; collapsed sidebar icons remain navigation.

Use **Messages / Terminals** in the Group header to switch the current Group's work view.
On narrow headers, the view icon switches between these views. Presentation also opens from
the header through the bookmark icon, beside the **Files** folder icon on desktop, with its own update indicator. Tooltips and accessible names identify these controls. View, search and context controls align with the main work area as the side panel resizes. Files and Presentation
share one side column: selecting the other swaps panels; selecting the active one closes it.
Switching views only replaces the history area; the message composer stays in place.
Each Group remembers its view and terminal page in this browser, including after a page reload. Message drafts and
the message reading position survive switching between views.

The terminal view shows up to four Actors per page, in the Group's existing order. Narrow areas
show one Actor per page. Page arrows appear only when there is more than one page, in the
Group header or the single Actor title bar on mobile. The arrows have 44-pixel targets, with a
visible page count and disabled endpoints. On touchscreens, swipe horizontally on the Actor
title area to page; buttons, vertical gestures and the terminal body keep their own behavior.
Resizing keeps the focused Actor visible,
or preserves the first visible Actor when no terminal has focus. Removing Actors displays a
valid page without discarding the saved preference during loading.
Indicators can point to waiting or stuck Actors on other pages without moving the current page.

Each visible terminal accepts input directly. Click its content to choose the keyboard target;
the focused pane is outlined, and new output does not take focus. The bottom composer still
sends Group messages, independently of terminal input. Maximize opens the existing Actor controls
using the same terminal instance; close the expanded view to return to the grid. Escape and Tab
remain terminal keys while the terminal has focus. Existing access and single-writer rules apply:
a read-only user cannot type. Opening a view leaves any existing writer in control; use
**Take control** to take over explicitly. An ordinary reconnect does not take control away
from another writer. With no existing writer, the terminal accepts input immediately.
A reconnect that resumes contiguous output keeps the existing screen visible while catching up.

Switching pages, Groups, or back to Messages keeps recently visited terminals and
their connections in memory, including scrollback and selection. Each workbench
retains up to **32 hidden terminals for five minutes** after leaving them; visible
terminals do not count toward that limit. The oldest hidden views are released
first, and background output does not extend their lifetime. Unvisited Actors are
not loaded in advance. Hidden terminals continue receiving output but cannot
accept input, resize the remote terminal, or take control automatically. On return, a
retained writer synchronizes the PTY with its visible dimensions, including when
control returned while hidden. Another window can still use **Take control**. Expiry, a browser reload, or a replaced
Actor may require a fresh terminal view; retention does not start or stop Actors.

Each Actor title bar keeps its status dot, common terminal actions and maximize control together.
Ordinary running/working states are available through the dot's label; stopped, waiting, stuck
and terminal connection notices remain visible as short text, including in narrow panes.
In narrow panes with pagination, the Actor name and notice occupy their own row above the
page and terminal controls, so touch targets do not crowd out the Actor's identity.
Use **More** for history, session, configuration, inbox and lifecycle actions. Unavailable
actions are disabled; stopping and restarting use the same operations as the expanded view.
Different Actors have independent pending-operation indicators.

Once a retained view expires or is evicted, returning attaches to the Actor's current output;
this never stops or restarts Actors. Actors without a TUI use their existing
runtime activity display, and stopped Actors are explicitly identified. Hidden Group messages do
not clear the message unread count or count as viewed for Voice suppression. Following a message
source link returns to Messages and locates the original event.

The Runtime Dock's progress bubbles show the latest short live update for each Actor, with up to
two Actors visible at once. Updates from the same Actor replace the previous excerpt. Open the
Actor to inspect the complete output. Restoring a Group or reconnecting its activity stream
restores history without replaying it as new bubbles; ordinary message unread indicators are
independent of these brief progress previews. The same rules apply to every managed Runtime.

### Right sidebar

Files and Presentation share one resizable sidebar with matching header controls. Each Group remembers its expanded width and Presentation density. Files always opens at the expanded width; its file draft survives switching to Presentation and back.

Presentation keeps four fixed slots. New Groups open a compact button rail by default; existing saved layout and window/split preferences are preserved. Open a filled slot to read its content, and use the four slot buttons to switch without returning to the list. Empty slots open the pin flow. The expanded overview uses image thumbnails and compact titled entries for other resources, without starting interactive browser/PDF sessions or clearing update indicators.

Collapse returns to the compact rail; closing the sidebar only hides it and keeps its contents. The expand button restores the remembered width and opens an available slot. Dragging to the minimum width also returns to compact slots. The divider supports arrow keys, and Enter toggles density. The viewer can still open in a larger window, and phones retain full-screen surfaces.

### Message search and settings

Message search shows the current Group name and Actor display names. Enter or **Search** submits the keywords; changing a filter reruns the submitted query. Typing new keywords does not change existing highlights or pagination until you submit again. Initial guidance, loading, no matches and failed requests are separate states. **Open** returns to the message context; reply and copying the event ID remain available.

Settings keeps **This group** and **This instance** separate. Section forms share a plain work surface, while independent configurations and authorization states retain their own boundaries. Under **Branding**, save the product name explicitly; icon uploads and resets apply immediately. The product name changes the Web sidebar, sign-in screen and browser tab, not the instance name shown in Connect.

Related fields use headings and spacing rather than nested cards. Section actions stay beside their heading when space allows and wrap on narrow screens. Settings help text follows the text-size preference. Switches support keyboard focus and Space, with a larger click/touch target around the compact track. These visual conventions do not change which settings save immediately or require a Save action.

### Workspace files

Open **Files** in the Group header to browse the active workspace. The file tree and
Presentation share the right-hand column. Opening a text file on desktop places its
editor in the message area and keeps the composer available. Closing the editor returns
to messages; covered messages do not count as viewed for unread tracking.

Save with the toolbar button or `Ctrl+S` / `Cmd+S`. Unsaved edits survive closing and
reopening files within the current Group, including internal symlinks to the same file.
Save before leaving the workspace. Switching Groups or instances with unsaved file edits asks whether to stay or discard; refreshing or closing the page uses the browser’s unsaved-changes warning. Closing the file viewer retains its drafts while you stay in the same workspace. A scope change or loss of access initiated elsewhere still retires the old editor; drafts never authorize saving into another scope.
Changing the active workspace clears the file view and its drafts; stale saves are rejected
instead of writing into the newly selected workspace. If the file changed on disk since it
was opened, saving reports a conflict. **Reload file** reads the latest contents from disk,
including updated media at the same path. With unsaved edits, it asks before discarding them;
canceling or a failed read keeps the draft. Edits made while the reload is waiting are also
retained. Reload is unavailable during a save. This check does not lock out external editors
or Actors.

The tree shows workspace files by default, including untracked and Git-ignored files.
Git's own root metadata directory remains hidden. Use **File browser options → Hide
Git-ignored files** to reduce clutter; this preference is remembered in this browser.
Folders load only when expanded. **Refresh directory** refreshes the tree, preserving
expansion and file drafts; **Reload file** refreshes the open viewer.

Paste a workspace-relative or absolute path into **Go to file or folder**, then press
Enter. Files open in the viewer; folders expand and receive focus in the tree without
closing your current file or losing its draft. Use `.` or the workspace root path to
return to the top of the tree. Missing paths and paths outside the workspace report
different errors. **Reveal current file** expands its parent folders
and moves focus to its row; explicit reveal also clears the Git ignore filter so the
target can be shown. **Collapse all folders** only folds the tree, keeping the file open.
If you move on while a location is loading, its result does not take focus away from
the editor, message composer, or another file selection.
Use arrow keys to move through the tree and expand/collapse folders, and Enter to open.

Each row's **…** button, right-click, or Shift+F10 opens the same actions: attach the path
as message context, copy a relative or absolute path, download a file, or pin a supported
file to Presentation when permitted. Attaching inserts a path into the composer; it does
not upload the file.

On desktop, **File browser options → New file / New folder / Upload files / Upload
folder** works at the workspace root. The same actions in a folder's menu target
that folder. File and folder menus also provide **Rename**, **Move to…**, and
**Delete**. Move destinations include the full workspace-relative name, and their
parent folder must already exist. Name collisions preserve the existing item.
Deleting a folder permanently deletes its contents; the confirmation names the path
and warns about affected unsaved edits. There is no recycle bin or undo.

Drag files or folders from your computer onto the Files tree to upload them; the
highlighted folder and drop label identify the destination. Drop on the panel
background for the workspace root. Native folder drops preserve subdirectories and
empty folders. The folder picker includes the browser-provided file tree; browsers
cannot include empty folders through that picker. If directory drops are unsupported,
use the upload menu instead. Each selection is limited to 1,000 files/folders and
100 MiB total. Uploads stop at the first conflict or failure. **Stop upload** keeps
completed entries; it does not roll back the batch. Git metadata cannot be uploaded.
Dropping here does not attach files to your message.

Drag one existing tree entry from the current Files panel onto a folder (or the panel background) to move it within
the workspace. **Move to…** offers the same operation without dragging. Moves and
renames carry unsaved drafts to the new paths; a pending save must finish first.
Changing a file's extension also updates its preview type while keeping unsaved text.
A destination with another unsaved draft is rejected so both drafts are retained.
Moving files does not rewrite imports or Presentation references. Refresh or repin
those references when needed.

Files uses UTF-8 paths. A directory containing names that cannot be represented
exactly shows an error; manage those names in the terminal. Names are never silently
replaced with another file's path.

Select **Changes** next to **Files** to inspect the active workspace's Git changes.
The list separates working-tree modifications, staged changes, conflicts and untracked
items. Select a tracked change to see its diff in the main area; use **Unified** or
**Side by side** when the viewer is wide enough. Untracked files open in Files,
untracked folders reveal their contents, and conflicts open the current file for
inspection. The view uses saved content; unsaved drafts remain intact when switching
views. **Refresh directory** refreshes the current Git view as well. Lists are read
on entry and after local writes while visible, without periodic background scanning.

Changes is read-only: CCCC does not stage, discard, commit or push on behalf of the
user. These remain terminal operations because Actors may share the repository's
index. Large lists/diffs show an explicit limit; Git failures are not shown as a
clean workspace. Phones retain their existing read-only Files behavior and can view
Git changes in the same full-screen surface.

Symbolic links have a link icon. If the target is missing, outside the workspace,
or inaccessible, its row explains why and disables content actions; you can still
copy its path, or rename/remove the link on desktop without changing its target. CCCC does not repair links created by other tools or download files
outside the workspace. After fixing a link on disk, use **Refresh directory**.

An empty file panel shows **No files to display** after loading completes. If ignored
files are hidden, use **Show git-ignored files** to reveal them. Otherwise, add files
to the workspace and use **Refresh directory**. Loading and failed requests have separate states.

A directory that cannot be loaded shows its error below the row. Use **Retry** there
to load it again; an error does not mean the directory is empty.

On phones, Files opens a read-only viewer. Binary files and files larger than 1 MiB are
not editable here. Files stay within the active workspace, and exhibit mode does not
expose this surface.

Images open in the shared graphics viewer, including files larger than the text
limit. Videos and audio use native browser controls, without autoplay. PDFs open
in the browser's PDF viewer, with **Open in a new tab** and **Download file** available
if inline viewing is disabled or unsupported. Native support depends on the browser
and the file's encoding.

Markdown renders with tables and Mermaid diagrams. CSV and TSV files open as tables,
including quoted cells and multiline values; the preview shows up to 200 rows and
50 columns. Text-sized SVG, Markdown, CSV/TSV and HTML offer **View source** and
**Preview**. Switching views preserves unsaved edits, and preview uses the current
draft. The text/source limit remains 1 MiB. Downloads always contain the saved file.

HTML opens as a static, isolated document: scripts, forms, nested pages and linked
stylesheets are disabled. Inline styles and workspace image/audio/video resources
are supported; it is not a running website or application preview. Markdown and HTML
resolve relative file links within the opened workspace and open them in Files.
Section fragments such as `report.md#details` or `report.html#details` position the
loaded preview at that section, including repeated same-file links. Positioning
happens once per navigation; ordinary updates do not pull the reader back. PDF
page fragments are passed to the browser viewer. HTML audio/video keep native
controls whether the URL is on the media element or a nested `source`.
HTML does not load external resources. Workspace paths, including percent-encoded
names, still undergo the same permission and scope checks. Root-relative document
links refer to the workspace root. Office documents and other unsupported formats
can be downloaded; CCCC does not upload them to an online document viewer.

Media and PDFs stream directly from the selected workspace and support byte ranges;
CCCC does not transcode or upload files to cloud storage. Each request checks the
Group permissions and opened workspace identity. Remote playback uses
the instance's upload bandwidth. Public Cloudflare Tunnel access remains subject
to Cloudflare's video/large-file distribution terms; preview support does not
establish an exemption or add a Cloudflare Stream subscription.

### Inspect images and diagrams

Workspace images, expanded message images, Mermaid diagrams, Presentation images and enlarged
quoted snapshots share a static graphics viewer. Use **Fit**, **100%**, **+** or
**−** to change scale; dragging pans enlarged content and the wheel keeps native
scrolling.
Touch screens support pinch zoom. Focus the viewport for `+`/`−`, `0` (fit),
`1` (actual size) and native arrow-key scrolling. Closing a modal restores focus
to its opener. Regular message scrolling, PDF controls and interactive browser
surfaces retain their own behavior. The workspace text editor is unchanged.
Refreshing the same workspace-linked Presentation image preserves its zoom and
position, including across a failed refresh and recovery. Image and Markdown updates replace
content only after loading succeeds (and images decode). During a transient failure, the last
loaded version stays visible with an update-failure notice. The next successful refresh clears
the notice. Slow requests finish before another automatic refresh starts. First-load errors
and confirmed missing or inaccessible resources show an error instead of stale content.
Switching to another slot or publication clears the previous resource and starts images with
**Fit** again.

Workspace-linked PDF and HTML readers retain their current page, zoom and
navigation state. Use **Refresh** to load edits from disk; publishing a new card
also updates the reader. They do not reload on the automatic image/Markdown
refresh timer.

### Codex Voice (Experimental)

The **Codex Voice** control below the Group list is a global assistant entry, not a Group roster item.
It has two stable actions: the main control always opens the Voice console, and the adjacent
microphone/square control starts or stops live audio. The same meanings are preserved in the
collapsed sidebar and active-call mobile bar. Settings, mute, and the Analyst terminal are not
separate Dock actions.

The Voice console is one operational workspace. Call state, audio settings, mute, and Stop are in its
top audio-control area. The conversation keeps up to 40 recent spoken entries from this call,
updating streaming fragments with the final transcript. These entries stay in this browser's memory
until the next call or page reload. The collapsible right pane embeds the selected Voice Analyst
runtime's writable TUI; its work status is independent of the live audio status. On desktop, drag
the divider to adjust the pane widths; focus it and use Left/Right or Home/End for keyboard control.
Double-click the divider to restore the default ratio. This browser remembers the chosen ratio,
while both panes retain a readable minimum width. Resizing does not reconnect the terminal. On narrow screens,
**Conversation** and **Voice Analyst** switch between the two panes.
Settings replace the console body inside the same panel; call controls stay available.
**Back to conversation** or Escape returns to the console without disconnecting its terminal.
Analyst drafts survive tab changes and panel minimization until explicitly saved or discarded.
The minimize control keeps the call running; **Stop voice** ends live audio.
Its tabs are **Voice & audio**, **Message notifications**, and **Voice Analyst**. Speaking voice,
microphone and supported speaker choices remain in Voice & audio, alongside response detail
(concise / standard / detailed) and speaking style (natural / direct / patient). Expression defaults
save automatically for the instance and apply to the next call, without restarting the Analyst.
Detail applies to incoming Actor notifications as well as answers. Voice is instructed to identify
each notification's Group and sender. Detailed asks it to retain substantive findings, numbers,
conditions, qualifications and next steps in natural speech; raw tool traces remain in the Analyst terminal.
Speaking voice and device choices are saved in this browser and also apply to the next call. Spoken
instructions can override them. The host must already be signed in through `codex login`, and the browser must
allow microphone access. That host login belongs only to the Realtime provider call.

#### Message notifications and source links

Notifications are off for every Group by default. In **Message notifications**, choose **To user**
or **All chat messages** for the Groups you want to hear from; enabling a scope starts at the
current ledger boundary, not old history. Explicit replies to work requested through Voice are
followed independently of subscriptions. Both acknowledgements and later replies are retained,
and only the expected Actor's reply to the exact source request counts as its response.

**Do not repeat messages already viewed in Web** is on by default. It uses exact source IDs after
at least 1.5 seconds of stable, fully visible, expanded display in a focused foreground page.
Hidden, covered, folded, partially visible and virtual-list overscan rows do not count. This does
not change Actor Mail unread state or reply obligations. A viewed explicit reply still updates
the Analyst's context, but does not trigger a duplicate reading. A user's explicit request to
read or explain a message still takes priority.

The console's compact **Message sources** section opens the original chat without stopping voice.
Pending and unconfirmed counts describe delivery observations, not task completion or proof that
you heard a result. During your call, queued output is shown as **waiting for Voice delivery**;
expand the section to see whether it is waiting for the connection or a notification check.
Updates continue reaching Voice during conversation; Voice handles speaking and interruptions.
An unfinished earlier turn cannot block later delivery. If the connection stops accepting updates,
the call stops with an explanation; restarting voice can deliver known unsent notifications while
the Analyst retains its context. Each source also shows the associated result's delivery state. Fully skipped output identifies
whether it was already viewed, excluded by policy, or unavailable at the submission check. The
recorded skip reason survives later preference changes. “Submitted” describes delivery to Voice,
not completed playback or proof of hearing. The experimental provider can omit details or fail to
resume a report after an interruption even after receiving its context; the Analyst and source links
remain available to inspect it. Delivery uncertainty can remain for earlier updates
while new ones proceed. If notification consumption pauses, a call-wide notice explains that source
messages are retained and starting a new call retries consumption; voice and Analyst state remain
separate.
If a result combines viewed and unviewed background messages, Voice can point
to the remaining sources rather than inventing a separable summary. Attachments are identified
without opening or uploading their contents. Notification text is source data, not authorization
for an Actor action.

Delivery references and cursors survive restarts in protected instance state. Unsent browser
output can return on a later call only when it is positively known not to have been submitted;
an uncertain handoff is retained and is not automatically replayed. Available results are returned
in their recorded completion order, including catch-up on a later call. All new notification routes
require trusted-local/admin access and are unavailable to scoped users and Exhibit viewers.
Remote administrator tokens are rechecked during calls, notification handoff and Analyst terminal
input/output. Revoking or downgrading one closes even an idle terminal connection within the next
authorization poll (one second) and stops new Voice work on that connection. An unfinished update from a replaced
Analyst remains unconfirmed rather than being silently executed again by its replacement.

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

For Claude Code, CCCC requires both the launcher and background worker to report version 2.1.259
or newer. Their versions can differ after a Claude update; an upgraded supervisor may retain older
workers or migrate idle sessions without changing the managed session's identity. CCCC owns the Agent View
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

User requests and source-message updates enter the same Runtime admission path immediately; the
Runtime owns steer/queue behavior. Analyst results are delivered to Realtime in order, including
while someone is speaking. Realtime owns speech timing and yielding to interruptions; missing
speech-turn events cannot block later result delivery. Adjacent fragments of the same result
are combined without losing text and split to the provider limit before sending. Delivery waits
only for connection readiness, send-buffer capacity, and pre-submission policy checks. If the
connection or send buffer does not recover within 15 seconds, the call stops with an explanation
and positively unsent results remain available for a later call. Previously submitted output is
not automatically replayed.
Pre-submission checks have a 10-second deadline and retry transient connection failures up to
three times with increasing delays. A failed check never permits unchecked content to be spoken.
If checking still fails, the call stops with an explanation and positively unsent results remain
available for a later call. A successfully suppressed result stays suppressed if its check is retried.
If Realtime does not confirm receipt within 30 seconds, the console shows a notice without
disconnecting or replaying the update. Results exceeding the 32 KiB Voice limit are reported
explicitly: use the Analyst terminal for the full output or ask for a shorter summary. Neither
notice stops the Analyst. Provider receipts confirm receipt, not that every detail was spoken.

Realtime Call and Voice Analyst have separate lifecycles. Stopping voice disconnects browser audio
and releases the microphone lease, but keeps the Analyst warm. Ongoing Analyst or linked
Actor work continues. Completed Analyst results are retained without waking audio. Actor replies
arriving while voice is off stay in the ledger and are consumed on the next explicit call; they do
not start notification model work in the background. A later call reuses the warm
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

In Web Model settings, **Open ChatGPT** reuses the current page without reloading it. Complete sign-in
and any website security verification manually in that browser. **Browser connected** describes the
viewer connection; it does not mean ChatGPT is signed in. Delivery waits for the signed-in conversation
composer, leaving the sign-in or verification page in place. Use **Reload ChatGPT page** only when you
intend to restart the dedicated browser; its saved profile and delivery target are retained.

## Performance behavior

- Hidden tabs release their group event streams immediately. Returning to the tab reconnects and
  catches up from the last event cursor, so several open tabs do not exhaust the browser's
  per-origin connection pool.
- Hover-prefetched group data is reused when that group is selected; group bootstrap does not issue
  a second actors read for the same transition.
- Production Web responses negotiate Brotli or gzip compression. Voice Secretary code and locale
  resources are loaded on demand instead of being part of every initial page load.

## Managing Groups

### Creating and editing a Group

Click **+ New** in the sidebar, select the workspace directory and create the
Group. You can also run `cccc attach /path/to/project` on the CCCC host.
Select a Group in the sidebar to open it; use the pencil beside its header name
to edit its title and description.

### Group settings

Open **Settings and more**, then settings, and select **This group**. Guidance,
automation, delivery, messaging, assistants, connections and other Group options
remain separate from **This instance** settings.

## Managing Agents

Use the **+** in the Agent bar near the composer to add an Agent. Choose an
installed Runtime, set its Actor ID and review the configuration before adding it.
Open an existing Agent to inspect its Runtime and use its available lifecycle
actions. The Group status button controls the whole Group's enabled Agents and
message delivery; pausing delivery does not stop work already in progress.

Switch to **Terminals** for the paged multi-Agent view, or open one Agent's
inspector from its entry. Runtime restart and session-reset actions have different
semantics; use the action's explanation before discarding a session.

## Messaging

### Sending Messages

1. Type in the message input at the bottom
2. Press `Ctrl+Enter` / `Cmd+Enter`, or click Send

On desktop, the input grows with its contents up to a compact height by default.
Drag the separator above the composer to set its actual height, including for an
empty draft. That manual height stays in place while typing or clearing the input.
Double-click or press Enter on the focused separator to restore automatic sizing.
Up/Down adjust by 16 base pixels; Home selects the minimum and End the maximum.
The manual preference is stored locally in base-font pixels and scales with text
size. Its effective height is recalculated as the panel shrinks, capped at 60% of
the panel while reserving message space and accounting for other composer rows.
The separator is absent below 768 px; mobile message scrolling and the existing
input expansion control remain unchanged.

On phones narrower than 640 px, the Voice Secretary sheet keeps language,
microphone and device refresh on one row. Prompt-mode help starts collapsed;
the Activity area uses the remaining height and scrolls independently, so the
last reply can be read without scrolling the control header. Instruction mode
places the compact text input and Send button side by side. Document mode stacks
the heading, view switch, metadata and actions at full width; paths wrap, markdown
headings use smaller phone sizes, and document content scrolls within the available
space. Wider layouts use
the existing workspace arrangement.

Recipient chips are one-shot: a successful send clears the selection, and switching Groups does not
restore a previous manual recipient. Unsent message text and attachments still remain as per-Group
drafts.

Broadcasts (`@all` and `@peers`, including the Group's default broadcast target) leave
disabled Actors stopped and deliver to enabled recipients. To wake a disabled Actor,
address it explicitly; `@foreman` also wakes the coordinator. An explicit target still
wakes when combined with a broadcast. Mail leaves disabled Actors stopped.

Messages larger than 64 KiB after UTF-8 encoding are sent as UTF-8 text attachments for
same-group and CCCC Connect reply targets. Local cross-group text remains inline because its two
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

## Project Context

Open **Project Context** from the clipboard button in the Group header. The
header identifies the Group being viewed.

- **Coordination** combines the working summary, `PROJECT.md`, coordination log
  and task board. The summary is a short steering brief; `PROJECT.md` remains
  the fuller repository reference. Empty summaries keep an Edit action without
  filling the page with empty fields.
- **Tasks** retain Planned, Active and Done columns, including empty drop targets.
  Search and attention filters narrow the board. Open a task for requirements,
  steps, handoffs and completion details. Missing requirements or closeout records
  describe record completeness; they do not change an existing task's status.
- **Agent State** shows the latest saved reports, including their timestamps and
  staleness. Focus, next action and blockers remain visible. Expand working context
  or recovery cues when needed. A task named in a report does not prove that its
  Runtime is currently working on it.
- **Self-Evolving Skills** shows candidates originating in this Group. Instance
  capability governance has a wider scope; the two views do not replace each other.

## Settings Panel

Open settings from **Settings and more** in the header:

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

Configure Telegram, Slack, Discord, Feishu, DingTalk, WeCom, or [Mattermost](./im-bridge/mattermost) integration.

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

Switch between Light, Dark, or Automatic theme. Automatic uses light mode from 07:00 to 19:00 in the browser's local time zone and dark mode otherwise.

Dark mode uses charcoal surfaces with lighter panels and visible borders. Input outlines and keyboard focus are stronger than ordinary section dividers. Text size also scales side-panel titles, file paths and settings labels; terminal text keeps its own sizing.

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

Before exposing the Web UI beyond localhost, first create an **Admin Access Token** in **Settings > Web Access**. With no administrator token, non-local clients receive only the UI shell and health/session guidance; protected APIs and business WebSockets remain locked, while direct loopback access keeps the passwordless local principal described above. Direct local setup no longer requires copying a bootstrap code. Linking your
account initializes an administrator Token if needed; completing that flow from
the verified localhost Web page also signs that browser in. Enabling hosted Remote
Access from that local page covers installations linked before this automatic
setup existed. Existing Tokens are preserved. When first configuring through a
remote address instead, read `web_bootstrap_token` inside the host's effective
`CCCC_HOME` (default `~/.cccc`) and enter that one-time code. The file is mode
`0600` on Unix and is removed once administrator setup completes.

The Web Access panel keeps LAN/public `Save`, `Apply now`, and remote-endpoint copying disabled until an Admin Access Token exists. The native daemon and Web boundary enforce the same rule at remote start, apply, and listener boundaries, so direct API calls and stale saved settings cannot bypass the panel. Group-scoped tokens do not satisfy this administrator recovery requirement. Switching back to localhost-only remains available so an incomplete remote setup can be recovered safely.

In **Settings > Web Access**, `127.0.0.1` means local-only and `0.0.0.0` means localhost plus your LAN IP on a normal local host. On WSL2 NAT, it still stays inside the VM until Windows networking forwards it outward.

`Save` stores the target binding. If Web was started by `cccc` or `cccc web`, use `Apply now` in **Settings > Web Access** to perform the short supervised restart. If Web is managed by Docker, systemd, or another external supervisor, restart that service instead.

For the default local app flow, prefer restarting from the owning `cccc` session itself: `Ctrl+C` to stop the whole app, then run `cccc` again. That keeps daemon and Web on the same fresh code/runtime.

`Start` / `Stop` are only for Tailscale remote access and do not rebind the already-running Web socket.

CCCC keeps the token policy simple:

- localhost-only: direct loopback browser requests are passwordless and use a non-persistent local administrator principal
- LAN/private network and public URL/tunnel/reverse proxy: an Admin Access Token is mandatory before exposure

`CCCC_WEB_ALLOW_UNAUTHENTICATED=1` is only an unsafe listener override; it never grants API authorization or bypasses first-admin bootstrap. Plain HTTP manual LAN exposure is allowed by default for private-network use, but still requires an Admin Access Token before remote APIs can be used. Prefer an HTTPS reverse proxy, tunnel, or encrypted overlay for untrusted networks.

CCCC adds `frame-ancestors 'self'`, `SAMEORIGIN`, `nosniff`, `no-referrer`, a restrictive permissions policy, and HSTS on HTTPS responses. Supervised CCCC Web processes trust reverse-proxy forwarding headers automatically only while the effective listener is loopback. A supervised LAN/wildcard listener or externally managed reverse proxy must explicitly set `CCCC_WEB_TRUST_PROXY_HEADERS=1` and must overwrite—not append—client-supplied `Forwarded` and `X-Forwarded-*` headers. Direct public listeners should leave this flag unset.

Enter the Access Token in the Web sign-in form. CCCC validates it through the
`Authorization` header and establishes an HttpOnly, `SameSite=Lax` session
cookie. The cookie has a rolling 30-day lifetime and is refreshed when the Web
session is checked, avoiding repeated token entry after mobile browsers reclaim
a tab. The temporary header token is removed from browser session storage after
the cookie is established. Access tokens are not accepted in ordinary API, SSE,
or WebSocket query strings and should never be placed in shared URLs.

For managed remote access, open **Settings → Web Access** and link this installation
to your CCCC account in the Reach section. Approve the request on the website,
then return to the same local panel. If an administrator Access Token already
exists, reuse it; otherwise complete the displayed token prerequisite. Choose
**Turn on remote access** explicitly. Account registration only reserves the
device address; enabling Reach creates its DNS record and tunnel.

Once enabled, Reach restores automatically when CCCC restarts and its Web
listener is ready. Keep CCCC running on this computer; opening the settings
panel is not required. Temporary network/account failures retry in the
background, with a delay of up to one minute between failed attempts. An
already-running tunnel helper handles its own network reconnection.
Turning Reach off, unlinking the account, or cutting the device prevents
automatic restoration. If unlinking cannot complete because the account service
is unavailable, Reach stays off and the device credential is retained so you can
retry unlinking. If the installed helper is missing or no longer matches
this CCCC version, install it explicitly with `cccc reach install`; automatic
restoration does not download or upgrade executables.

The panel confirms the connection with at most six status checks over 45 seconds.
It pauses checks while hidden and never restarts or provisions a tunnel as a
retry. If confirmation ends without a connection, check this computer's network
and CCCC process, then use **Check connection**. A stopped helper offers an
explicit startup retry in addition to daemon recovery. **Turn off** remains available, and other provider/binding
settings remain locked while Reach is enabled even if the tunnel disconnects.

**Tunnel connected** means the account service observed a connected tunnel. Open
Web from another network to confirm application access; website account login
does not sign the browser into this device. The displayed check time is the last
status observation, not a continuous availability guarantee.

Reach follows the same credential rule. Its status payload exposes only a tokenless public
address. Clicking **Open Web** or **Copy Admin Link** asks the local authenticated
Rust Web session for a 120-second, one-time exchange code bound to that Reach
origin. The public endpoint consumes the code once, establishes the HttpOnly
cookie, and redirects to a clean `/ui/` URL.

A passwordless localhost administrator reuses an existing administrator Access
Token for this exchange; no new long-lived token is created. A remotely signed-in
administrator remains bound to their own active token. Revoking that token also
invalidates outstanding links backed by it.

Cookie-authenticated Rust Web writes and WebSocket connections require an exact allowed `Origin`, with a
same-origin `Referer` accepted only as a fallback. This check is independent of
CORS and blocks same-site sibling domains from submitting state-changing forms.

Cross-origin browser clients can opt in through `CCCC_WEB_CORS_ORIGINS`, a
comma-separated list of exact origins (scheme, host and port). Named origins
support credentials and are also accepted by Cookie write and Cookie-authenticated
WebSocket origin checks. Include only trusted client sites.

`CCCC_WEB_ALLOW_ANY_ORIGIN=1` instead enables wildcard HTTP CORS without
credentials, for clients that explicitly supply an `Authorization: Bearer ...`
access token and omit browser credentials. It does not bypass Cookie write or
Cookie-authenticated WebSocket origin checks, create an authenticated principal, or expose the local
passwordless principal to other sites. Local passwordless reads with an Origin
or Referer must identify the same loopback origin, just as writes do. Browser
WebSocket clients using cookies still need a same-origin connection or a named
trusted origin. Explicit Bearer-authenticated WebSocket clients do not require an
Origin match; token and Group permissions still apply.
If both settings are present, wildcard HTTP CORS takes precedence; use only
`CCCC_WEB_CORS_ORIGINS` for cross-origin Cookie sessions. Both settings are off
by default and require restarting the Web process to change CORS responses.

#### Reverse proxy headers

When a reverse proxy terminates HTTPS or exposes CCCC under another host, it
must overwrite the browser-facing host and protocol headers. These values are
used by every browser WebSocket (realtime events, terminal, Voice Secretary, projected browser)
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
`csrf_origin_invalid` for Cookie-authenticated writes and WebSockets. If a proxy
cannot preserve the public host, configure its exact browser origin in
`CCCC_WEB_CORS_ORIGINS` instead of disabling source protection.

A token scoped to selected Groups receives global stream
metadata only for those Groups, and the global stream never carries message
content. Full event content remains on the per-Group stream and is subject to
the same scope check. Administrative capability changes require an Admin token.

Same-origin browser requests carrying `Sec-Fetch-Site: same-origin` satisfy the
Cookie CSRF check even when a reverse proxy rewrites Host or terminates HTTPS.
This does not require `CCCC_WEB_CORS_ORIGINS` or global proxy-header trust.
Proxies should preserve this browser-generated header, never manufacture it for
cross-origin requests. `same-site`, `cross-site`, `none`, and missing metadata
continue through the existing Origin/Referer and configured-origin checks.
Authentication and group permissions remain required. See the
[Fetch Metadata specification](https://www.w3.org/TR/fetch-metadata/).

Browsers send no Fetch Metadata on a WebSocket handshake, so Terminal and
stream sockets fall back to comparing the request `Origin` against the served
host. That comparison ignores the scheme, because TLS terminating at a reverse
proxy makes the origin server read every `https://` page back as `http://`.
Host and port must still match exactly — a different host, port, or subdomain
is rejected — and the scheme alone adds nothing, since an attacker able to
serve a page from this host would already control the site. Proxies that
rewrite `Host` still need `CCCC_WEB_TRUST_PROXY_HEADERS=1` or an explicit
`CCCC_WEB_CORS_ORIGINS` entry.

### Shared realtime connection

The Web UI uses one `/api/v1/events/ws` WebSocket per page for global metadata,
current-Group ledger events, and headless output. Switching Groups replaces the
subscriptions on the existing socket. Background pages release their subscriptions
and close the socket when none remain. This avoids occupying the HTTP/1.1 pool
with three persistent SSE requests per visible page; ordinary API requests remain
available with several workbench windows open.

Each subscription has an ID. Both sides ignore packets from retired IDs. Reconnects
resume the ledger from its last delivered event ID and request a headless snapshot
from the same tail that supplies subsequent deltas. Existing UI catch-up and
coalescing continue to apply. Global events contain only permitted Group metadata.
Token and Connect-frame authority are rechecked while connected; each Group
subscription also validates Group access before opening its producer.

The server uses a bounded eight-packet output queue and a five-second socket-write
deadline. Heartbeats detect broken connections, and closing the socket cancels all
its producers. A failed channel retries independently; transport failure reconnects
with backoff. The UI does not fall back to HTTP SSE, which would recreate the
connection-pool blockage. The existing SSE endpoints remain available for external
clients and use the same typed event producers. Deploy the updated UI and backend
together to enable the new endpoint.
