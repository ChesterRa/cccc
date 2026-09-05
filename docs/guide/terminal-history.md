# Terminal history

Native PTY actors always keep terminal output in two bounded memory layers, with an optional durable third layer:

- A configurable in-memory hot buffer serves live WebSocket output, cursor reconnects, history queries, and raw-replay fallback. It defaults to 10 MiB per actor.
- A completed-session cache keeps up to 256 KiB per stopped actor and 8 MiB total queryable without reopening files.
- When durable persistence is enabled, an append-oriented rolling transcript under `CCCC_HOME/groups/<group_id>/state/terminal/<actor_id>/` preserves raw PTY bytes across actor and daemon restarts.

Fresh WebSocket clients negotiate `snapshot_v1`. Rust maintains a bounded headless terminal mirror beside the raw byte ring and serializes its current screen plus up to 512 recent scrollback lines into one ANSI snapshot. The snapshot carries the exact raw PTY cursor it represents; after xterm parses and acknowledges it, the stream continues with untouched PTY bytes after that cursor. Snapshot encoding bytes never advance the raw cursor. This makes a newly opened actor show the latest terminal state immediately instead of parsing the oldest retained output first.

Reconnects with a valid consumed cursor remain tail-only and do not reset the terminal. If the cursor has expired, the server sends a new snapshot. Unsupported stateful graphics/control strings, unsafe mirror dimensions, an oversized snapshot, or a client that does not negotiate `snapshot_v1` automatically uses the retained raw ANSI replay path. The raw ring and optional durable transcript remain the source for `/terminal/history`; older durable sessions are not injected into an interactive xterm attach.

For a control connection that explicitly takes ownership, the browser includes its fitted rows and columns in the attach negotiation. The runtime serializes writer registration, that initial resize, and snapshot capture under one session lock, so the returned owner and snapshot dimensions cannot come from different concurrent takeovers. The browser always parses a snapshot at the snapshot's advertised rows and columns, then refits its local xterm if the visible viewport differs; a viewer never resizes the shared PTY. `term_resize` remains the canonical later resize operation. WebSocket bridges include their attachment ID so the runtime atomically rejects resize requests from a controller that has already been replaced. Native Rust also accepts its former `terminal_resize` spelling as a compatibility input, but new clients should not emit that alias.

## Cursor and restart boundaries

`terminal_replay` is an active-session operation: it never stitches an older
durable transcript into a new interactive terminal. Memory-only absolute
cursors remain continuous across replacement sessions while the same daemon
process owns the actor. A daemon restart does not transfer that live PTY, its
input mode, or its hot output ring. The actor is
started again; provider-level session resume, where supported, is a separate
runtime feature.

With durable persistence enabled, `/terminal/history` and `terminal_since` can
query the retained transcript and its stored cursor after a daemon restart.
Without it, completed output is available only while it remains in the bounded
process-local caches. A cursor from a previous memory-only daemon lifetime must
therefore not be treated as a durable resume token.

## Retention

Durable capture is opt-in through `observability.terminal_transcript.enabled=true` and `observability.terminal_transcript.persist=true`. Both default to `false`. `per_actor_bytes` controls both the memory ring and, when enabled, durable retention. It defaults to 10 MiB; zero selects that default, and larger values are capped at 50,000,000 bytes.

```yaml
# CCCC_HOME/settings.yaml
observability:
  terminal_transcript:
    enabled: true
    persist: true
    per_actor_bytes: 10485760
```

Restart the affected PTY actors after changing this setting; capture mode is selected when each session starts.

When the durable limit is reached, CCCC keeps the newest bytes, removes older session files, and reports `cursor_expired` for cursors older than the retained boundary. Disabling persistence stops new durable writes; it does not silently delete existing transcript files.

If the archive cannot be created or written, actor startup and PTY draining continue with bounded in-memory history. CCCC reports the archive failure locally instead of turning an observability failure into a runtime outage.

Transcript files are created with owner-only permissions on Unix. They contain raw terminal output and can therefore include commands or secrets printed by a runtime. Protect `CCCC_HOME` accordingly.

## Shutdown behavior

Normal stop and natural process exit drain the PTY reader before the transcript is finalized. Writes are flushed and synchronized before the runtime session is removed. If a descendant keeps the PTY open past the bounded drain window, the completed session is sealed before a replacement starts; late output from the old session cannot overlap or hide the new session's cursor range. A machine crash can still lose bytes that have not reached the operating system; avoiding that window entirely would require synchronizing every PTY chunk and would materially reduce throughput.

The `terminal/clear` operation advances the absolute cursor and clears the hot buffer, active durable transcript, and the in-memory screen mirror used for future snapshots. It does not reset the cursor to zero, which keeps reconnect semantics unambiguous and prevents a fresh attach from restoring cleared scrollback.

## Browsing repainted output

The history panel preserves inferred screens before backward cursor moves, line
or screen erasure, carriage-return redraws, and alternate-screen switches.
Frames appear in chronological order separated by a blank line. They are inferred
from terminal controls, not timestamped screenshots; partial redraws can produce
intermediate frames. Identical consecutive frames are collapsed. Live terminal
snapshots and `terminal_tail` still show the current screen only.

Backward requests extend one contiguous byte range to a pinned end cursor, so
split ANSI sequences are interpreted together and new live output does not
change the page. Rendered frames have a 50 MB display budget, separate from raw
retention; if repaint expansion exceeds it, an explicit marker reports omitted
older rendered frames. The raw transcript is unchanged.

History rendering keeps scrolled-off lines independently of the bounded screen.
Loading older pages therefore retains both earlier lines and already-visible
newer lines even when the cumulative transcript exceeds 4,096 rows. This
scrollback shares the 50 MB display budget and explicit omission marker with
inferred frames; newline-heavy output is stored in chunks rather than one
allocation per blank line.

Short or control-only pages do not automatically scan the retained archive. Use
**Load older history** to continue; scrolling to the top of scrollable content
also loads an older page. Errors pause automatic loading until an explicit retry.
If retention or a terminal clear overtakes the pinned snapshot, paging stops
with an expired-history notice and keeps the text already displayed. Reopen
history to capture the current retained output.

When a second Ctrl-C or the launcher's normal-shutdown deadline forces exit,
the launcher terminates registered OS process trees directly. This path does not
call normal runtime stop, wait for session/protocol locks, or add another cleanup
timeout. PTY actors, managed provider processes and their helpers, DeepSeek
supervisors, and Web-owned detached daemons register ownership during spawn.
Registration and forced admission closure are serialized, so a new child cannot
slip between creation and registration during forced exit.

Unix polling observes exit without reaping (`waitid` with `WNOWAIT`), terminates
remaining process-group members, then revokes registration before reaping the
leader. This prevents an old registration from targeting a reused PID/PGID.
Windows uses owned Job handles with kill-on-close semantics. Ordinary shutdown
still performs graceful protocol closure, output draining, and process reaping;
forced termination issues OS termination without waiting for those operations,
so final transcript writes may be incomplete.

Windows standard-process launches (managed providers and helpers, DeepSeek, and
supervised daemon startup) begin suspended. CCCC assigns the Job before resuming
the primary thread, so descendants inherit Job membership from their first
instruction. Caller-supplied detached-process flags are preserved explicitly.
Failed assignment or resume terminates and reaps the suspended child.

A managed child's normal stop retains its handle until termination and reaping
succeed. Signal, polling or wait errors leave that same child available for a
subsequent stop attempt. This serialization does not affect the independent
emergency termination registry.
