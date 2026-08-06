# Terminal history

Native PTY actors always keep terminal output in two bounded memory layers, with an optional durable third layer:

- A 512 KiB in-memory hot buffer serves live WebSocket output, reconnects, and the initial screen snapshot.
- A small completed-session cache keeps recently stopped sessions queryable without reopening files.
- When durable persistence is enabled, an append-oriented rolling transcript under `CCCC_HOME/groups/<group_id>/state/terminal/<actor_id>/` preserves raw PTY bytes across actor and daemon restarts.

Screen snapshots are derived from the hot ANSI stream so opening a busy TUI does not require replaying a complete transcript. WebSocket reconnects continue from an absolute byte cursor. When persistence is enabled, the durable transcript extends `/terminal/history` beyond the in-memory window and across daemon restarts; otherwise history is memory-only and bounded to the current daemon process.

## Retention

Durable capture is opt-in through `observability.terminal_transcript.enabled=true` and `observability.terminal_transcript.persist=true`. Both default to `false`, matching the Python implementation's memory-only default. `per_actor_bytes` defaults to 10 MiB and is clamped to 1 MiB through 200 MiB when persistence is active.

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

The `terminal/clear` operation advances the absolute cursor and clears the hot buffer plus the active durable transcript, when present. It does not reset the cursor to zero, which keeps reconnect semantics unambiguous.
