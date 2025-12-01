# CCCC Advanced Features

## Auto-Compact (Context Compression)

CCCC includes intelligent auto-compact to prevent peer context degradation during long-running sessions.

### How it works

- **Idle Detection**: Automatically detects when peers are idle (no inflight messages, no queued work, sufficient silence)
- **Work Threshold**: Only triggers after meaningful work (≥6 messages exchanged since last compact)
- **Interval Gating**: Respects minimum time interval (default: 15 minutes) to avoid wasteful compaction
- **Per-Actor Support**: Each CLI actor declares compact capability in `agents.yaml` (e.g., `/compact` for Claude Code, `/compress` for Gemini CLI)

### Configuration

In `.cccc/settings/cli_profiles.yaml`:

```yaml
delivery:
  auto_compact:
    enabled: true                    # Global toggle
    min_interval_seconds: 900        # Wait 15 min between compacts
    min_messages_since_last: 6       # Require ≥6 messages of work
    idle_threshold_seconds: 120      # Peer must be idle for 2 min
    check_interval_seconds: 120       # Check every 2 minutes
```

### Benefits

- Maintains peer mental clarity across multi-hour sessions
- Prevents context window bloat and token waste
- Zero manual intervention — works automatically in the background

### Diagnostics

- Check `.cccc/state/ledger.jsonl` for `auto-compact` and `auto-compact-skip` events
- Logs include reason codes (e.g., `insufficient-messages`, `not-idle`, `time-interval`)

### Troubleshooting

**Check if it's working:**
```bash
grep "auto-compact" .cccc/state/ledger.jsonl | tail -20
```

**Common skip reasons:**
- `insufficient-messages` — Peer hasn't done enough work yet (< 6 messages)
- `not-idle` — Peer has inflight/queued messages or hasn't been silent for 2 minutes
- `time-interval` — Not enough time since last compact (< 15 minutes)
- `actor-compact-disabled` — CLI actor doesn't support compact (check `agents.yaml`)

---

## Aux (Optional On-Demand Helper)

Aux is a third peer for burst work (e.g., broad reviews, heavy tests, bulk transforms).

### Enable at startup

- In Setup Panel, select an actor for `aux` (e.g., `gemini`)
- Or set `aux→none` to disable

### Invoke Aux

- In TUI: `/aux <prompt>`
- In chat bridges: `/aux "<prompt>"` or `aux: <prompt>`

### Use cases

- Strategic reviews (check POR against all SUBPORs)
- Heavy lifting (run full test suites, generate bulk fixtures)
- External checks (dependency audits, security scans)

**Note**: Aux runs once per invocation. No persistent state between runs.

---

## Foreman (User Proxy)

Foreman is a lightweight "user proxy" that runs on a timer (default: 15 minutes) and performs one non-interactive task or writes one short user-voice request to the right peer.

### Enable at startup

- In Setup Panel, choose an actor for Foreman (or `none` to disable)
- You can reuse Aux actor or pick a different one

### Configure

- Edit `./FOREMAN_TASK.md` (free-form: describe what matters now and list standing tasks)
- System rules live at `.cccc/rules/FOREMAN.md` (auto-generated, no manual edits)

### Visibility

- Status panel shows: `Foreman: RUNNING | last @ HH:MM rc=N | next @ HH:MM`
- IM shows messages as `[FOREMAN→PeerA]` or `[FOREMAN→PeerB]`

### Controls

- `/foreman on|off|status|now` (only if Foreman was enabled at startup)
- `/verbose on|off` toggles Foreman CC to chat

### Use cases

- Periodic health checks (run tests every 30 minutes)
- Reminder to update POR/SUBPORs
- Enforce quality gates (lint, type-check before commit)

---

## RFD (Request For Decision)

For irreversible or high-impact changes, CCCC supports RFD cards:

- Peers can raise an RFD when they need human approval
- RFD appears as an inline card in IM bridges with approve/reject buttons
- Execution pauses until user responds
- Decision is logged to ledger for audit trail

---

## Multi-Role System

| Role | Responsibility | Required? |
|------|----------------|-----------|
| **PeerA** | Primary executor, collaborates equally with PeerB | Yes |
| **PeerB** | Primary executor, collaborates equally with PeerA | Yes |
| **Aux** | On-demand helper for batch tasks, heavy tests, etc. | No |
| **Foreman** | Scheduled "user proxy" for periodic checks and reminders | No |

> **Mix and match freely** — any role can use any supported CLI based on your needs.

---

## Session Cadence

- **Self-Check**: Every N handoffs (configurable, default 20), orchestrator triggers a short alignment check
- **POR Update**: PeerB receives periodic reminders to review `POR.md` and all active `SUBPOR.md` files
- **Auto-Compact**: When peers are idle after sufficient work, orchestrator automatically compacts context
- **Foreman Runs**: Every 15 minutes (if enabled), Foreman performs one standing task or writes one request
