# Voice Secretary

Voice Secretary is a hidden internal actor backed by repository Markdown and a
native Rust speech pipeline. Enabling it copies the foreman's runtime settings
into the dedicated `voice-secretary` actor; disabling it removes only that actor
and leaves documents, transcript sidecars, and model caches intact.

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
Only one native diarization job runs at a time. The sherpa-onnx diarization API
requires one complete `f32` waveform, so this stage has a bounded, temporary
full-recording memory peak; it reads directly from the recording file without
also retaining a duplicate PCM byte buffer. If the model is unavailable or the
worker is busy, capture closes normally and reports that speaker analysis was
skipped. Every recording has an independent session ID, so a late result cannot
overwrite a newer recording.

## Durable Input

Stable segments are appended to:

```text
~/.cccc/voice-secretary/<group_id>/<session_id>/transcripts/segments.jsonl
```

Semantic input is appended to `inputs.jsonl` before the daemon writes an
`assistant.voice.input` event and a targeted `system.notify`. Segment IDs are
idempotent, so browser retries do not duplicate document input. The internal
actor reads unread batches through `cccc_voice_secretary_document`, edits the
repository document, and saves it through the daemon/MCP contract.
The durable input log remains the idempotency source after the bounded session
preview is trimmed, and interrupted ledger notification is completed on retry.

Only the `voice-secretary` actor may advance the unread input cursor. Document
paths must be repository-relative Markdown paths; symbolic-link components are
rejected so the document API cannot write outside the selected workspace.

Documents use the active workspace under `docs/voice-secretary/`. Groups without
an active workspace store the Markdown fallback under CCCC_HOME. Removing a
model, disabling the assistant, or restarting CCCC does not delete documents or
raw transcript sidecars.

The Rust daemon is the source of truth for Voice Secretary sessions, input
cursors, document indexes, recording leases, and model installation state.
