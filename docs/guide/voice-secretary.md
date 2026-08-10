# Voice Secretary (Experimental Rust Implementation)

Python remains the stable default CCCC implementation. This page documents Voice
Secretary in the experimental Rust implementation and its native speech
pipeline. See the standard Voice Secretary controls in the Web UI when running
Python.

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
instead of being accumulated in Rust byte buffers. Final ASR feeds PCM16 and WAV
samples to sherpa-onnx in bounded chunks on the blocking worker pool. A single
final-ASR permit prevents native inference from stalling normal Web/API requests
or multiplying large memory peaks. The 100 MiB value is a per-recording abuse and
resource limit (about 55 minutes of PCM16), not a preallocated memory requirement.
Each WebSocket recording must also hold the daemon recording lease.
Disconnects finalize the last hypothesis. Stopping capture releases the
microphone immediately, runs the installed SenseVoice model on the blocking
worker pool, and sends `final_asr_text` before closing the recording connection.
If final ASR fails, the live transcript remains available. An installed
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
Only one native diarization job runs at a time. The sherpa-onnx diarization API
requires one complete `f32` waveform, so this stage has a bounded, temporary
full-recording memory peak; it reads directly from the recording file without
also retaining a duplicate PCM byte buffer. If the model is unavailable or the
worker is busy, capture closes normally and reports that speaker analysis was
skipped. Every recording has an independent session ID, so a late result cannot
overwrite a newer recording.

## Durable Input

Stable document-capture ASR segments are appended to:

```text
~/.cccc/voice-secretary/<group_id>/<session_id>/transcripts/segments.jsonl
```

Prompt refinement and document instructions are semantic inputs, not meeting
transcripts: they never create a session entry or a per-session transcript
sidecar. Semantic input is appended only to `inputs.jsonl` before the daemon
writes an `assistant.voice.input` event and a targeted `system.notify`. Segment
identity is independent from the prompt request ID: one prompt request may
accumulate several speech appends, while each append carries its own
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

Instruction/Ask mode creates a durable pending `ask_requests` item atomically
with its semantic input. The delivered notification renders an explicit work
order with the target, request ID, and required MCP output instead of relying on
the actor to infer routing from raw JSON. User-visible answers must be submitted
through `cccc_voice_secretary_request(action="report")`; ordinary console text
is not treated as a delivered reply. A report updates the Ask item, emits an
`assistant.voice.request` ledger event, and lets the web client restore
`reply_text` after refresh or reconnect. Exact input and report retries are
idempotent.

When Voice Secretary is disabled, microphone input remains available as direct
dictation. Both Rust and Python local ASR paths accept the explicit `composer`
dispatch target, but the browser appends the transcript straight to the composer
without creating a secretary input, running prompt refinement, updating a
document, persisting a secretary session, or starting speaker diarization.

An active Rust local-ASR audio stream renews its recording lease. The browser's
HTTP heartbeat remains a cross-tab status signal, but transient heartbeat
failures do not stop or orphan an otherwise healthy recording WebSocket. This
matches the Python runtime's recording-liveness behavior while retaining Rust's
explicit single-recorder lease validation.

Documents use the active workspace under `docs/voice-secretary/`. Groups without
an active workspace store the Markdown fallback under CCCC_HOME. Removing a
model, disabling the assistant, or restarting CCCC does not delete documents or
raw transcript sidecars.

The Rust daemon is the source of truth for Voice Secretary sessions, input
cursors, document indexes, recording leases, and model installation state.
