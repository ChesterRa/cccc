# Voice Secretary

Voice Secretary runs in the native CCCC product and uses one durable workflow
authority. Local ASR is provided by the linked `sherpa-onnx` runtime.

Voice Secretary is a hidden internal actor backed by repository Markdown.
Enabling it copies the foreman's runtime settings into the dedicated
`voice-secretary` actor; disabling it removes only that actor and leaves
documents, transcript sidecars, and model caches intact.

## Local ASR

Open **Settings > Assistants**, enable Voice Secretary, select **Local ASR**, and
install the final and live models. The sherpa-onnx runtime is linked into the
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
If final ASR fails, the live transcript remains available. For segmented
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

Prompt refinement and document instructions are semantic inputs, not meeting
transcripts: they never create a session entry or a per-session transcript
sidecar. Semantic input is appended to the daemon-owned durable input log before
the daemon writes the corresponding ledger event and targeted `system.notify`.
Segment identity is independent from the prompt request ID: one prompt request
may accumulate several speech appends, while each append carries its own
`input_append_id`. Retrying the same append reuses that ID and does not duplicate
input or invalidate a pending draft; later speech keeps the request ID but uses a
new append ID. The internal actor reads unread batches through
`cccc_voice_secretary_document`, edits the
repository document, and the daemon reconciles the Markdown content into the
document index when assistants or documents are next read or selected. A changed
file advances the indexed revision once; repeated reads are idempotent.
The durable input log remains the idempotency source after the bounded session
preview is trimmed, and interrupted ledger notification is completed on retry.
Archived documents are retained in durable state but omitted from the working
document projection. Archiving the current document selects the most recently
updated remaining active document, or clears the active target when none remain.

Only the `voice-secretary` actor may advance the unread input cursor. Document
paths must be repository-relative Markdown paths; symbolic-link components are
rejected so the document API cannot write outside the selected workspace.

Prompt mode uses two distinct operations. Web input first calls
`assistant_voice_input_append(kind="prompt_refine")`, which records the current
composer text, speech, operation, request ID, and composer snapshot before
delivering one canonical input envelope. The actor then returns the optimized
text through
`cccc_voice_secretary_composer(action="submit_prompt_draft")`, which maps to
`assistant_voice_prompt_draft_submit`. Draft submission updates only the
existing request; it never appends another Voice Secretary input. Empty or
non-substantive refinements use `no_op=true`.

Instruction/Ask mode creates a durable pending `ask_requests` item in the same
accepted daemon operation as its semantic input, before actor delivery. The
delivered notification renders an explicit work
order with the target, request ID, and required MCP output instead of relying on
the actor to infer routing from raw JSON. User-visible answers must be submitted
through `cccc_voice_secretary_request(action="report")`; ordinary console text
is not treated as a delivered reply. A report updates the Ask item, emits an
`assistant.voice.request` ledger event, and lets the web client restore
`reply_text` after refresh or reconnect. Exact input and report retries are
idempotent.

When Voice Secretary is disabled, microphone input remains available as direct
dictation. Local ASR accepts the explicit `composer` dispatch target, but the
browser appends the transcript straight to the composer
without creating a secretary input, running prompt refinement, updating a
document, persisting a secretary session, or starting speaker diarization.
Composer acquisition and heartbeats remain valid while Voice Secretary is
disabled; a heartbeat that omits its dispatch target inherits `composer` from
the matching active lease.

An active local-ASR audio stream renews its recording lease. The browser's
HTTP heartbeat remains a cross-tab status signal, but transient heartbeat
failures do not stop or orphan an otherwise healthy recording WebSocket. The
explicit single-recorder lease remains authoritative.

Documents use the active workspace under `docs/voice-secretary/`. Groups without
an active workspace store the Markdown fallback under CCCC_HOME. Removing a
model, disabling the assistant, or restarting CCCC does not delete documents or
raw transcript sidecars.

Voice Secretary has one durable authority for lifecycle, durable health,
sessions, prompt drafts/requests, and ask requests:
`groups/<group_id>/state/assistants.json`. Assistant enablement and configuration
remain in `group.yaml`; process-local PID, port, service, and socket observations
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
