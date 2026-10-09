# Voice Secretary

Voice Secretary runs in the native CCCC product and uses one durable workflow
authority. Local ASR is provided by the linked `sherpa-onnx` runtime.

Voice Secretary is one global, on-demand service with direct Runtime configuration
or a linked Runtime Profile. Groups keep their own documents, transcripts and requests;
no secretary appears in the normal Actor roster. Voice Analyst remains a
separate service for Realtime calls, with its own configuration and context.

In the expanded Secretary workspace, **View Secretary** opens the shared execution
view. Codex, Claude Code, Grok Build, OpenCode and Kilo offer an interactive native
terminal; ACP-only runtimes show actual output and tool activity. The header names
the Group/task currently being processed. Results return to the original document,
Questions or composer; a model answer alone is not proof of successful application.
Native terminal input uses the same resident session. The owner observes manual
turns and waits for them before dispatching queued work; hiding the terminal does
not stop that observation. Ordinary terminal conversation has no automatic Group
destination or composer writeback. Changing Groups does not retarget a running task.

The first task starts the Secretary. Its process and conversation remain available
between tasks, including after a normal clarification question. All Groups share
this conversation; it is not a boundary for confidential Group information. Tasks
run one at a time, so a long document task can delay other requests. Each task has
an immutable destination checked by CCCC, even when the visible Group changes.
**New session** closes an idle conversation without deleting saved work; the next
task starts a fresh one. Cancel, connection/termination failure, Runtime changes
and daemon shutdown can also close the session. Waiting for information does not
block other tasks. A daemon restart does not restore the shared conversation.

**Recent tasks** is an optional history for the selected Group, with clarification,
retry and diagnostic controls. Results and follow-up remain associated with their
original requests. Opening a result destination never reapplies or sends it.
The matching composer acknowledgment determines whether a draft was inserted.
Opening a terminal never creates a model task; closing the panel only hides it.
Shared execution/terminal access requires an instance administrator. Recording
and Dictation do not depend on the terminal or an available model.
If the global Secretary cannot start, the workspace and Secretary settings show
the failed startup stage and affected Group or task. Saved inputs are retained;
you do not need a terminal log to see this cause. Unrelated old Runtime settings
do not block the Secretary, and viewing status does not retry failed work.

Open **Settings → This instance → Voice → Voice Secretary** to select a Runtime
directly or link a Runtime Profile, and manage recognition and document-processing defaults. Secretary supports the same
structured runtimes as Voice Analyst: **Codex, Claude Code, Grok Build, OpenCode,
Kilo**, and **Antigravity, GitHub Copilot, Devin and Cursor in ACP mode**. Native
TUI Profiles are excluded. Tasks have distinct working copies and share one persistent execution session;
native tools follow the Runtime permission model. CCCC binds its task MCP to the
accepted target and checks document versions before committing working copies.
These checks do not sandbox arbitrary native file or network operations.
Profiles retain the Runtime's normal authentication and workspace-trust requirements.
For Claude Code, trust `$CCCC_HOME/voice-secretary/workspace` once using the configured
Claude installation and `CLAUDE_CONFIG_DIR` before starting Secretary work. Create
the directory first if it does not yet exist. This is the shared runtime workspace. This setup does not require
sending a model prompt. An untrusted directory
is reported as a startup failure, not silently bypassed or retried.
Secretary instructions restrict work to the fixed target and its working copy.
Codex and Claude use dedicated MCP configurations; Copilot disables other configured
servers and built-in MCPs for the task process. Other providers may still expose
user-configured services; this is not a filesystem or connected-service sandbox.
All voice settings use **Settings → This instance → Voice**, with two tabs:
**Voice Secretary** and **Codex Voice**. Secretary has Runtime/model, capture and
recognition, and document-processing sections. Codex Voice has Realtime call
preferences and Voice Analyst. Their runtime forms use the same direct/Profile
choices, but credentials, contexts and results stay separate.
The shared **Audio devices** section above the two tabs owns microphone and speaker
choices for both workflows and all Groups in the current browser. The Secretary's
**Adjust devices** shortcut and either workspace's settings open that same editor. Device
changes apply to the next recording or call, leaving active audio unchanged.
Local and external ASR capture audio from the browser and use the selected microphone;
Browser ASR uses the browser's default microphone. The Secretary does not play audio;
the speaker choice applies to audio-producing workflows such as Codex Voice. Browsers
without output selection use the system speaker. Missing devices remain selected
until the user chooses another device; refreshing never requests microphone access.
If browser storage cannot be written, the editor reports that choices are retained
only for the current page, without silently restoring a stale device at start.

The composer offers **Prompt**, **Doc** and **Ask**, in that order. Prompt writes to
the composer and defaults to dictation only, without a Secretary model task.
Check **Polish after recording** beneath **Prompt** to refine recognized speech
automatically. This option remains visible whenever the mode menu is open, including
while Doc or Ask is selected. Changing it only saves the Prompt preference and keeps
the menu open; selecting any mode closes the menu. The manual polish button also works on typed text and dictated text
when automatic polishing is off.

The composer remembers its capture mode, automatic-polishing choice and recognition language separately
for each Group in the current browser, including across page reloads. A new Group
starts in Prompt with automatic polishing off and follows the instance's default recognition language until
you choose a language for that Group. Microphone and speaker choices remain shared.
Each new recording reads the latest shared recognition settings and the Group's
language choice. Active recordings keep their starting mode, polishing choice, language, devices and
target even when you switch Groups. For Browser ASR, **System default** follows the
browser's preferred language rather than detecting the spoken language. Missing
regions are resolved before recognition starts (for example, `ja` becomes `ja-JP`);
explicit regions such as `en-GB` or `zh-TW` remain unchanged. Browser recognition
support and service availability vary by browser. Recognition errors also appear
beside the composer controls; reconnection is indicated only while recording is active.

The gear in either workspace scrolls to the corresponding feature tab;
**Adjust devices** instead focuses the common audio section. Closing settings
returns without ending a call, recording or task. Recognition choices, document
update preferences and call preferences save automatically. Runtime/credential drafts
and Secretary work rules have separate explicit Save/Discard controls. A preference
save never submits the Runtime or credentials. Analyst drafts survive closing settings;
unsaved Secretary/ASR drafts prompt before leaving.

Choose a model through the custom launch command (for example `--model ...` where
supported) or the Runtime's native configuration. Linked Profiles use their command
and private environment; **Edit shared Profile** opens the existing editor in place.
Both runtime forms support **Add to Runtime Profiles**, **Create Profile** and
**Edit shared Profile** through the same editor. Creating a Profile from Custom
settings copies only that service's saved private environment and staged changes;
values remain write-only. Applying the new Profile is a separate Runtime save.
Editing a shared Profile also affects its other users. Custom Secretary credentials
are independent of Analyst and Actor credentials, and secret values are never read back.
No extra model configuration layer or per-Group secretary Actor is created.

There is no Group Voice settings page or enable switch. Shared recognition backend,
language, update interval and work rules belong to the global Secretary. Specific
requirements are entered with a task in that Group's workspace. The recording
language selector remembers the choice for that Group in the current browser; it does not change the instance default.
Runtime changes affect the next task; recognition and checkpoint defaults affect
the next recording. Already running work keeps its settings and target.
**Prompt with automatic polishing off** writes recognized text into the composer without a model task; no
Secretary Runtime is required for it. Recognition credentials and local model
maintenance retain their own explicit save/install controls.

Without a configured Runtime, Secretary input remains saved and does not occupy
execution capacity. Recognition defaults can be saved on their own. When configuring
or changing a Runtime with pending inputs, choose whether to process them (consuming
provider allowance) or retain them and process only new input. Held input remains
visible after restart; an explicit process choice can release it later with the same
Runtime. Preferences-only saves never release it implicitly. Invalid task sources
remain saved and are reported separately without blocking subsequent valid input.
A full execution queue delays processing, not transcription storage.

Saving a Prompt request does not mean model execution has started. If processing
is deferred, the composer and Secretary workspace show that the input is saved
and display the blocking reason. Unavailable execution remains visible even when
no task could be created. Later admission or a matching returned draft clears the
waiting problem without resubmitting the input. Startup ignores registry entries
for missing Groups; actual state or cleanup errors still prevent Secretary startup.

Tasks show saved inputs waiting for processing and uncommitted document sources.
Status reminders highlight unresolved work. **Recent tasks** is ordered by creation
time, newest first, matching each row's timestamp regardless of status. Blocking
Runtime permission requests and Cursor questions/plans become **Needs information** tasks instead of
holding up other Groups. The Secretary does not approve them; it stops and cleans
up that task process. Provide the missing information or adjust Runtime permissions,
then use the ordinary task continuation. User cancellation retains
these sources for explicit review; it does not mark them committed. After process
cleanup and commit-journal reconciliation, interrupted/failed ASR document batches
with previously confirmed task-only native writes can get one fresh attempt
against the current document. Other Runtime executions require explicit retry;
changing the selected Profile does not make an earlier uncertain task safe to
replay. Version conflicts get the same bounded recovery; candidates are never
automatically merged. Uncertain
prepared commits and other requests still require review. A recovered ASR batch
keeps its place before newer queued document sources.

For a typed document request, choose **Update document** or **Ask about the saved
document**. Ask returns an answer without editing the document. The Secretary
can read bounded recent Group messages and files/search within the scope fixed
when the task was accepted. Private/generated paths and runtime state are excluded;
changing the visible scope cannot redirect an accepted task.
General Ask also receives the currently referenced document when it belongs to
that same scope. On narrow screens, use the existing Ask mode for a question;
recording and document selection remain separate actions.

Completed, projected tasks keep compact source IDs and receipts for deduplication,
and retire their model guidance and full working copies. Status polling clones
unresolved work and a bounded recent history rather than the entire retained task
history. A completed retry also retires
obsolete predecessor copies, retaining their diagnostic receipts. Deleted Groups release their owned
source/task data after provider cleanup; original repository files are not removed.
Claude Agent View jobs keep a recovery record for the Secretary workspace. After
a CCCC crash, the host stops only those owned jobs before releasing capacity and
deleting private launch settings. Retained records from the earlier task-based
execution are cleaned at their original directories before moving working copies;
an unavailable control service leaves cleanup pending.

On screens narrower than 640 px, the composer keeps the microphone and a
**Voice options** button in the action bar. Voice options contains capture mode,
language, prompt polishing, and the workspace entry in a scrollable panel with
44 px touch targets. Recording locks disable mode, automatic-polishing and language changes. Desktop and mobile
language controls share the same recording lock and work for dictation as well as model-assisted modes.
Menus close when switching Groups, when their controls become unavailable, or
when responsive layout hides their trigger. Opening the workspace transfers
keyboard focus into it; closing it returns focus to the Voice options button.
Transcription and prompt-processing status appear above the input instead of
competing with action buttons. The wider-screen controls remain inline.

## Workspace modes

**Prompt** works in the composer. By default, recording adds recognized text
without starting a model. Automatic polishing after recording, or the manual polish
button, returns a draft, no-change result, failure or a concise clarification. Prompt does not need a
separate large-panel view. Applying a draft still checks the original composer
snapshot in the original Group so a late result cannot overwrite newer typing,
including after navigation or clearing the composer. If the text changed, **Review
draft** keeps the candidate available for copying, explicit application or dismissal.
Only a successful local application is acknowledged as applied. Queue age never
marks Ask complete or ends Prompt observation; long-running work retains its request
identity until a result, cancellation or an explicit replacement.
A confirmed, cleaned-up failure or cancellation permits a new explicit Prompt
request in another Group; it does not automatically retry the old task.

The workspace has **Documents** and **Ask** views. Switching views does not change
the composer's capture mode. Documents provides the document library and a
Document/Transcript view, with document updates and questions submitted from its
input bar. Ask keeps questions, Markdown answers and sources together. Recording
fixes its destination at start; viewing another document does not retarget it.

**View Secretary** opens the shared runtime's terminal or structured output.
Recent tasks remain available for explicit recovery, clarification and reviewing
retained candidates. Returning to the workspace or closing the panel does not
stop recording or the resident Secretary.

## Live transcript and document actions

The Transcript view shows live original ASR text below the recording indicator. This preview is scoped to the recording group and document;
it is separate from saved entries and disappears on stop, when final transcript
processing takes over. The saved-entry count continues to count final entries.

The workspace outline reacts to microphone volume during recording and indicates
processing during final audio analysis. Reading the current audio level does not
re-render the composer for each incoming audio frame.

Right-click a working-document row (or press Shift+F10 with the row focused) to
select, archive, or delete that exact document. Archive keeps the file and marks
the document archived. Delete removes it from the active and archived lists,
clears its quoted references and capture target, and chooses another active
document. A confirmation names the document before either mutation. Rename changes
only the display title; the file keeps its path. During
recording these menu actions are disabled; the server also rejects deletion while
the group holds a recording lease.

The row's menu button also opens with Enter or Space without selecting the
document; Enter or Space on the row itself still selects it. Deleted index
entries cannot be archived, so a stale client's archive request cannot make a
deleted document available for restoration.
Saving or appending transcript to a deleted path is also rejected before any
Markdown, session, transcript-log, or ledger write. Clients receive a deletion
error instead of a successful save to an invisible document.

Deletion uses `POST /api/v1/groups/{group_id}/assistants/voice_secretary/documents/delete`
with `document_path`. The document must be registered; traversal and symlink paths
are rejected. Its Markdown file is removed from disk; the index entry is kept with
status `deleted` so the document never reappears through workspace discovery.
Transcripts and historical ledger events are retained. There is no recovery folder:
the confirmation names the document because the file cannot be restored afterwards.
An API failure keeps the document visible, its file in place, and its references intact.

Internally the file is first moved to `CCCC_HOME/voice-secretary/<group_id>/trash/`
so a failed index update or deletion-event append can be rolled back. The temporary
copy remains until both the index and ledger writes succeed. On failure, rollback
copies the file back without consuming its backup, restores the previous index
(including the active document), then removes the backup. If rollback itself fails,
the error reports the retained backup path rather than discarding the last copy.
Only after the index and ledger commit is the file permanently removed; if that
final cleanup fails, the copy is left behind and a warning is logged. Copying and
rollback also work across filesystems through a synced destination-side temporary
file.

The working-document sidebar displays titles only, with full titles available on
hover. Its footer opens the archive directory: click an archived title for a
read-only preview, Restore to return it to its previous folder, or Delete to
remove it from both working and archived lists. Deletion is visually separated
from Archive in the context menu.

The normal workspace poll reconciles local archive guards with server-visible
documents. A restore by another client therefore reappears without reloading or
switching groups. Stale or failed refresh responses cannot clear these guards.

Folders are single-level, group-wide persistent organization, stored in the voice
document index. The sidebar shows them as a tree: click a folder to expand its
documents in place. By default, unfiled documents follow the folders. Create a
folder from the header's folder button. Drag a document onto a folder to file it,
or onto the unfiled area to take it out; the Move to folder menu action does the
same without dragging. Drag a folder anywhere among the folders and unfiled
documents; the mixed order is saved. Items absent from that saved order appear
before ordered items, with folders first and then unfiled documents. On touch
screens, long-press a row to start dragging. New documents are created at
the root. Removing a folder moves its documents to the root without deleting
them. Folder assignment survives editing, archiving and restoring; it does not
change Markdown paths or quoted references.

`GET /api/v1/groups/{group_id}/assistants/voice_secretary/documents/library`
returns folders and non-deleted documents, including archived content. POST to
the same endpoint accepts `create_folder`, `rename_folder`, `remove_folder`,
`rename`, `reorder_root`, `move`, and `restore`, with `name`, `folder_id`, `document_path`,
or `root_order` as appropriate. `root_order` is the mixed order of root items as
`folder:<id>` / `document:<path>` keys; keys for missing items are dropped.
Empty/duplicate names, missing folders, unauthorized writers, and attempts to
restore a deleted document are rejected. Read responses from a previous group
cannot overwrite the current group's library.

## External realtime ASR: Bailian and Volcengine

Select **Settings → This instance → Voice → Voice Secretary → Capture and recognition → External provider ASR**.
Choose the provider and explicitly save its credentials in that section. Recognition defaults are global;
credentials/model settings are shared by this CCCC service instance and can only
be managed by administrators. Existing recordings retain the provider/model
selected at start; changes apply to subsequent recordings.

- **Alibaba Cloud Bailian**: API Key, Beijing or Singapore region, optional
  Workspace ID, and either `fun-asr-realtime` (default) or
  `paraformer-realtime-v2`. A Workspace ID selects the region's dedicated
  `maas.aliyuncs.com` endpoint; leaving it empty uses the supported DashScope
  endpoint for that region. API keys must match the selected region/workspace.
- **Volcengine Doubao**: the optimized bidirectional `bigmodel_async` endpoint.
  New-console accounts use **API Key**; legacy accounts use **App ID + Access
  Token**. Select the purchased 1.0/2.0 duration/concurrent Resource ID; the
  default is `volc.seedasr.sauc.duration`. Streaming language detection is owned
  by the Volcengine model; the local language selection is not sent as an
  unsupported forced-language parameter.

**Test connection** checks the saved credentials/connection (and Bailian task
admission) without sending microphone audio. It is not a recognition-accuracy
or available-quota guarantee. Real recognition requires an enabled, funded
provider account and network access from the CCCC server to the provider WSS
endpoint. Audio leaves the CCCC server for the chosen provider and may incur
provider charges.

Credentials are stored separately from Group/assistant configuration, in
`CCCC_HOME/config/voice-asr-providers.json`, using atomic owner-only writes on
Unix. Read APIs return presence/configured flags rather than credentials.
Blank credential inputs preserve the stored values; **Clear provider
credentials** explicitly removes them. Browser code never receives stored keys,
and vendor response bodies/credential headers are not relayed in errors.

External recording reuses the browser's 16 kHz mono PCM16 WebSocket transport,
recording lease and bounded segmented storage. Audio is packaged in 200 ms
chunks. Volcengine uses incremental (`single`) utterance results, which are
accumulated by timestamp instead of retransmitting the complete meeting on
every update. Bailian waits for `task-started` before audio; Volcengine's optimized
stream starts sending audio without waiting for a nonexistent task-started event.
Stopping flushes the remaining PCM, requests the provider's final result, and
waits for explicit completion before emitting the existing `final_asr_text` and
`closed` events. Empty captures close without submitting an empty recognition.

For document capture, the server buffers stable provider sentences and appends
them with idempotent IDs at the configured document-update interval. Disabling
automatic updates (`auto_document_max_window_seconds: null`) defers submission
until recording ends; live subtitles still arrive immediately. Stop and recovery
flush pending text regardless of the interval. The browser does not append
duplicate cloud checkpoints. Complete final results supersede live revisions
through the existing transcript API. Provider completion is separate from saving:
a failed last checkpoint is retried with its original segment ID, and the final
text is still returned with persistence status so the browser can retry saving.
If several segments remain unconfirmed, the browser retries those segments with
their original IDs before saving the final revision. This also recovers available
segments from an incomplete recording without treating them as a complete final
transcript. If a browser retry still fails, recording stops with an error and
the remaining unconfirmed text returns to the original Group's composer for
review; it is not automatically sent to an Actor.
Connection failures retain known document segments and recover available text to
the composer for non-document capture. A disconnected document recording gets a
bounded attempt to finalize its provider stream. Lease release is fenced to its
owner and also runs when the handler is cancelled.

This initial external integration is realtime WebSocket ASR. The existing HTTP
file-transcription endpoint remains local-ASR-only. Cloud capture does not invoke
local SenseVoice final ASR or local speaker separation. Provider session/quota
limits can be stricter than local recording limits; failures stop capture
explicitly rather than silently reconnecting/replaying billable audio.

Protocol references:
- [Bailian realtime WebSocket](https://help.aliyun.com/zh/model-studio/fun-asr-realtime-websocket-api)
- [Bailian client events](https://help.aliyun.com/zh/model-studio/fun-asr-client-events)
- [Bailian server events](https://help.aliyun.com/zh/model-studio/fun-asr-server-events)
- [Volcengine streaming ASR](https://www.volcengine.com/docs/6561/1354869)

## Local ASR

Open **Settings → This instance → Voice → Voice Secretary → Capture and recognition** and select **Local ASR**.
Install the final and live models under **Entire instance → Voice**. The sherpa-onnx runtime is linked into the
Rust binary, so runtime install/remove actions are compatibility no-ops. Models
are downloaded into `~/.cccc/cache/voice-models`, verified against the bundled
manifest, unpacked in staging, and atomically activated. Existing model caches
and `install-state.json` files are read in place. Operating-system file locks
make interrupted installs recoverable after a process crash.

Live browser capture sends 16 kHz mono PCM16 as binary WebSocket frames; JSON is
used only for start/stop control messages. Both WebSocket recordings and HTTP
binary request bodies are streamed into auto-deleted files under `~/.cccc/cache`
instead of being accumulated in Rust byte buffers. Short WebSocket recordings
receive immediate final ASR. When speaker analysis is available, persistent
recordings over 30 seconds, or recordings stopped while the single native
inference worker is occupied, complete stop promptly, retain the durable live
transcript, and defer final speaker-labeled transcription to the queued
speaker-analysis stage. Final ASR paths that cannot
defer reuse one offline recognizer across bounded 30-second inference ranges.
HTTP uploads keep their fail-fast busy response. The 100 MiB value is a per-recording abuse and
resource limit (about 55 minutes of PCM16), not a preallocated memory requirement.
Each WebSocket recording must also hold the daemon recording lease.

### Switching Groups During Recording

An active recording is a navigation-independent session. Its Group, target
document, capture mode, dispatch target, composer snapshot, and session ID are
fixed when recording starts. Switching the visible Group does not move or stop
the recording: checkpoints, final transcripts, Ask/Prompt requests, and speaker
analysis continue to target the original Group. The UI identifies that Group
and keeps the single global recording lease until the user stops and saves.

Direct-composer results follow the same ownership rule. If another Group is
visible when text becomes ready, CCCC appends it to the original Group's
preserved composer draft instead of changing the visible Group's draft.
The live recognizer resets its native stream at every detected speech endpoint,
including silence or unchanged hypotheses, so decoded features do not accumulate
for the lifetime of an open microphone connection.

WebSocket PCM is rolled into a new file every 25 minutes (48,000,000 bytes). A
completed segment is flushed and data-synced before the server emits
`recording_segment_saved`; capture and live recognition continue without
reopening the microphone. Clean completion removes the temporary files after
the final pipeline releases them. The complete WebSocket session is capped at
800 MiB (about 7 hours 17 minutes) as an abuse and disk guard. HTTP uploads keep
their independent 100 MiB limit. Browser-side backpressure keeps a bounded PCM
tail and sends it before the stop frame; if audio must be dropped, capture stops
with an explicit error instead of silently shifting transcript timestamps.
Final result metadata reports each 30-second inference range in timeline order
and retains its owning 25-minute `recording_segment_index`.

The linked Rust speech runtime and an installed live streaming model are
separate readiness conditions: the microphone control is service-ready only
when a compatible streaming model is installed (or an explicit test mock is
configured). Runtime linkage alone must not defer a predictable missing-model
failure until after recording starts.
Disconnects finalize the last hypothesis. Stopping capture releases the
microphone immediately, runs the installed SenseVoice model on the blocking
worker pool, and sends `final_asr_text` before closing the recording connection.
For document capture, successful complete final ASR is also stored as a `final`
revision that supersedes the session's `live` checkpoints. The meeting view therefore
replaces the low-latency **Live Paraformer** cards with the higher-quality
**Final SenseVoice** result after stop or reconnect, while the raw live rows
remain in transcript sidecars for recovery and audit. The daemon atomically
deduplicates this revision against existing live semantic input; a short
recording with no live input uses the final revision as its one document input
when automatic document input is enabled.
Partial final ASR never supersedes the complete live transcript. If no live text
exists, its successful partial text remains the fallback. If final persistence
fails, the WebSocket reports that state and the browser retries the idempotent
revision before falling back to its retained transcript. If final ASR fails,
the live transcript remains available. For segmented
recordings, successful segment text is retained, while any failed segment is
reported explicitly as a partial final transcript in the Web UI. An installed
diarization model then adds speaker ranges in the background and emits an
`assistant.voice.session` event when the result is ready.
Speaker ranges are normalized to first-seen `Speaker 1..N` labels, tiny
spurious clusters are absorbed into the nearest stable speaker, and adjacent
same-speaker windows are merged within bounded durations. One offline
recognizer is then reused to transcribe each speaker window independently. The
complete sorted speaker timeline is retained; processing does not discard turns
after a fixed segment count. The
meeting view therefore restores per-speaker text instead of assigning one
whole-recording transcript to whichever range contains its midpoint.
Speaker identities are never synthesized by this post-processing: ranges and
labels are published only after the native pyannote + 3D-Speaker clustering
pipeline succeeds. A clustering or per-window ASR failure is reported as
`diarization_failed`, and the saved raw transcript remains unlabeled.
An unexpected WebSocket disconnect also flushes the owned temporary recording,
runs final ASR, and durably appends the best available final transcript for
document capture before starting speaker separation. Prompt, instruction, and
direct-composer capture never create meeting artifacts or speaker-analysis jobs.
The connection releases its recording lease only when the stored owner and lease
ID still match, so stale connection cleanup cannot unlock a newer recorder.
Reacquiring from the same browser owner also creates a fresh lease ID and fences
the superseded connection.
Only one native inference job runs at a time. The sherpa-onnx diarization API
requires one complete `f32` waveform, so this stage has a bounded, temporary
full-recording memory peak; it reads directly from the recording file without
also retaining a duplicate PCM byte buffer. Speaker analysis waits fairly behind
active final ASR or speaker work and persists its result when the worker becomes
available; only a missing model skips analysis. Every recording has an
independent session ID, so a late result cannot overwrite a newer recording.

## Durable Input

Stable document-capture ASR segments are appended to:

```text
~/.cccc/voice-secretary/<group_id>/<session_id>/transcripts/segments.jsonl
```

The bounded per-session meeting projection is shared in
`groups/<group_id>/state/assistants.json`. The durable document-level transcript
that survives session pruning and aggregates several recordings is shared at
`~/.cccc/voice-secretary/<group_id>/documents/<document_id>/transcript.jsonl`.
Both Web implementations read these records through the daemon instead of
owning a separate browser-side transcript authority. Transcript clearing also
uses the daemon operation so the session projection and both durable logs are
removed under the same transcript lock.

Each stored segment declares a transcript stage when the producer knows it:
`live` for incremental Paraformer checkpoints and `final` for a complete stopped
SenseVoice pass. A final revision records the live segment IDs it supersedes.
Consumers project the non-superseded records, but storage keeps both stages;
this is raw/final revision history, not destructive replacement. General LLM
punctuation, filler removal, and prose polishing are not part of this path.

Prompt refinement and document instructions are semantic inputs, not meeting
transcripts: they never create a session entry or a per-session transcript
sidecar. Semantic input is appended to the daemon-owned durable input log before
the daemon records the corresponding ledger event and global task. A task fixes
its Group, scope and document/request identity at acceptance. Retrying the same
append returns the same task, including an unconfirmed outcome. The task-bound
MCP tool supplies context and accepts the terminal business result. The model
incrementally edits a document working copy; the daemon alone checks its original
registration/base digest and atomically commits the original. A conflict retains
the candidate without replacing newer user work. A → B → A uses independent
processes and threads, never a shared cross-Group model context.

The **Secretary tasks** section in the existing workspace shows this Group's
queued, processing, needs-information, conflict, failed and unconfirmed tasks.
Cancel retains source input and does not stop recording. Needs-information work
releases its process before asking for a follow-up. Explicit continuation starts
a new task; uncertain outcomes require review and confirmation. Document
candidates can be reviewed/copied through the ordinary editor, not auto-merged.
A document lane and a separate Ask/Prompt lane keep long document work from
blocking interactive requests; no idle processes are retained.
Archived documents are retained in durable state but omitted from the working
document projection. Archiving the current document selects the most recently
updated remaining active document, or clears the active target when none remain.

There is no per-Group secretary Actor or legacy execution path. Global tasks use their own business receipts. Provider turn completion and console
text do not count as a delivered answer or committed document.
Document paths remain repository-relative Markdown paths with symlink rejection.

Prompt polishing records the composer snapshot and returns a draft through the
fixed task. A replaced request/snapshot cannot be overwritten by a late result.
Empty refinements use `no_op=true`; drafts are never sent as messages.
Ask records a durable request before execution. A terminal report updates that
request and its ledger feedback, allowing results to survive refresh/reconnect.
For explicit peer coordination, the Secretary may propose a message to one
current Group Actor. The user reviews and confirms forwarding; the task cannot
send messages itself. Secretary answers/drafts are not injected into Realtime Voice.

In **Prompt** with **Polish after recording** off, microphone input routes directly to the composer. Local ASR accepts the explicit `composer` dispatch target, but the
browser appends the transcript straight to the composer
without creating a secretary input, running prompt refinement, updating a
document, persisting a secretary session, or starting speaker diarization.
Composer acquisition and heartbeats remain valid without a configured Secretary Runtime; a heartbeat that omits its dispatch target inherits `composer` from
the matching active lease.

An active local-ASR audio stream renews its recording lease. The browser's
HTTP heartbeat remains a cross-tab status signal, but transient heartbeat
failures do not stop or orphan an otherwise healthy recording WebSocket. The
explicit single-recorder lease remains authoritative.

After a successful local-ASR stop, the server sends the final transcript events,
the application `closed` event, and a WebSocket Close frame with code 1000.
The browser processes transport errors after earlier transcript events so that
expected shutdown cannot interrupt asynchronous transcript finalization or
display a spurious connection-failed message. Unexpected connection failures
still report an error.

Documents use the active workspace under `docs/voice-secretary/`. Groups without
an active workspace store the Markdown fallback under CCCC_HOME. Removing a
model, clearing the Secretary Runtime, or restarting CCCC does not delete documents or
raw transcript sidecars.

Group sessions, prompt drafts/requests, and Ask feedback have one durable authority:
`groups/<group_id>/state/assistants.json`. Runtime, recognition defaults and shared
work rules belong to instance settings; obsolete Group configuration is ignored,
and task/commit receipts belong to `voice-secretary/jobs`. Process-local PID, port, service, and socket observations
are rebuilt after startup. Former preview input/document projection fields are
preserved under `rust_state` rather than being mistaken for common workflow
records. Every recording-lease mutation and expiry-capable read is serialized
through `~/.cccc/state/voice_secretary_recording_lease.json.lock`, so concurrent
clients cannot silently create a second recorder. Repository Markdown,
transcript/input sidecars, the shared document index, and native model caches
retain their specialized stores.

Native model installation is a Web-owned boundary: the Web UI manages the
bundled sherpa-onnx model cache, while the daemon reports
`assistant_voice_model_install=false` in daemon capabilities. Callers must
inspect that capability instead of assuming a daemon operation is available.
The shared model list and install/remove controls are in **Settings → This instance → Voice**;
model maintenance requires administrator access. Recognition defaults are shared,
and recordings can choose a temporary language without downloading another copy.

### Saving recognition settings

Recognition/backend/language/provider choices and the periodic-document toggle
save automatically. A valid update interval saves on blur or Enter. Failed preference
writes retain the selection and offer retry. Work rules and Runtime/private credentials
have independent explicit save/discard controls. Runtime saves preserve the latest
preferences; preferences do not alter the Runtime, private credentials or held-input
policy. Backlog processing requires a separate explicit choice, applied with a changed
Runtime or on its own when the Runtime is unchanged. Provider credentials use their
own save action; model installation/removal is an explicit operation.
These settings do not start a provider process. Accepted tasks alone trigger model
work once a Runtime is configured. Active recordings retain their original defaults.

New Volcengine configurations default to API Key authentication and ASR 2.0 hourly.
Existing credentials, authentication modes, and resource versions are preserved.
Choose credentials by their field names in the speech console, not by the age of
an account: API Key and App ID + Access Token are separate authentication methods.
Secret Key is not used by this streaming API. The selected model version and
billing plan must be enabled for that account. Connection tests distinguish a
known resource-not-granted rejection from authentication failure, unspecified
access denial, and quota limits without exposing upstream response bodies.

## Existing documents and obsolete Actors

Groups retain documents, transcripts and fixed-target tasks; they do not own another
Secretary configuration. Old Actor HELP files are preserved but their protocol is
not imported into the task executor. Daemon startup stops and removes obsolete
secretary Actors through the normal resource lifecycle before restoring Group
runtimes. No legacy executor or automatic replay of old Actor history is retained.
Configure the global Secretary explicitly, and choose what to do with unclaimed
saved input when selecting its Runtime.
