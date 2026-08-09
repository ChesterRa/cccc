# Terminal history

Native PTY actors always keep terminal output in two bounded memory layers, with an optional durable third layer:

- A configurable in-memory hot buffer serves live WebSocket output, reconnects, and the initial raw ANSI replay. It defaults to 10 MiB per actor, matching Python.
- A completed-session cache keeps up to 256 KiB per stopped actor and 8 MiB total queryable without reopening files.
- When durable persistence is enabled, an append-oriented rolling transcript under `CCCC_HOME/groups/<group_id>/state/terminal/<actor_id>/` preserves raw PTY bytes across actor and daemon restarts.

Fresh WebSocket attaches atomically capture the current PTY session's UTF-8-complete replay boundary, then stream retained raw ANSI history up to that fixed boundary in bounded 512 KiB pages. New output cannot make the initial loop chase a moving tail and starve keyboard input; once the boundary is reached, live polling and input handling run together. Reconnects continue from an absolute byte cursor and request only new output. Rendered screen snapshots remain available for diagnostics, but are not used to initialize the interactive terminal because TUI repaint sequences intentionally erase older frames. When persistence is enabled, the durable transcript extends `/terminal/history` beyond the in-memory window and across daemon restarts; durable output from earlier sessions is never injected into an interactive attach.

## Retention

Durable capture is opt-in through `observability.terminal_transcript.enabled=true` and `observability.terminal_transcript.persist=true`. Both default to `false`, matching the Python implementation's memory-only default. `per_actor_bytes` controls both the memory ring and, when enabled, durable retention. It defaults to 10 MiB; zero selects that default, and larger values are capped at 50,000,000 bytes like Python.

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
