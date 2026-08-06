# Terminal history

Native PTY actors keep terminal output in three bounded layers:

- A 512 KiB in-memory hot buffer serves live WebSocket output, reconnects, and the initial screen snapshot.
- A small completed-session cache keeps recently stopped sessions queryable without reopening files.
- An append-oriented rolling transcript under `CCCC_HOME/groups/<group_id>/state/terminal/<actor_id>/` preserves raw PTY bytes across actor and daemon restarts.

The durable transcript is the source of truth. Screen snapshots are derived from the hot ANSI stream so opening a busy TUI does not require replaying the complete transcript. WebSocket reconnects continue from an absolute byte cursor, while `/terminal/history` reads older pages from the durable transcript when they are no longer in memory.

## Retention

`Settings > Global > Developer > Terminal transcript` controls `observability.terminal_transcript.per_actor_bytes`. The default is 10 MiB per actor and the accepted range is 1 MiB to 200 MiB.

When the limit is reached, CCCC keeps the newest bytes, removes older session files, and reports `cursor_expired` for cursors older than the retained boundary. This bounds both disk and memory use; “durable” means no loss inside the configured retention window, not unlimited retention.

Transcript files are created with owner-only permissions on Unix. They contain raw terminal output and can therefore include commands or secrets printed by a runtime. Protect `CCCC_HOME` accordingly.

## Shutdown behavior

Normal stop and natural process exit drain the PTY reader before the transcript is finalized. Writes are flushed and synchronized before the runtime session is removed. A machine crash can still lose bytes that have not reached the operating system; avoiding that window entirely would require synchronizing every PTY chunk and would materially reduce throughput.

The `terminal/clear` operation advances the absolute cursor and clears both the hot buffer and the active durable transcript. It does not reset the cursor to zero, which keeps reconnect semantics unambiguous.
