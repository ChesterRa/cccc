# Operations Runbook

This page is for operators who need reliable day-to-day CCCC execution.

## 1) Runtime Topology

Default runtime home:
- `CCCC_HOME=~/.cccc`

Key paths:
- `~/.cccc/registry.json`
- `~/.cccc/daemon/ccccd.sock`
- `~/.cccc/daemon/ccccd.log`
- `~/.cccc/groups/<group_id>/group.yaml`
- `~/.cccc/groups/<group_id>/ledger.jsonl`

## 2) Startup and Health Checks

### Start

```bash
cccc
```

### Health Baseline

```bash
cccc doctor
cccc daemon status
cccc groups
```

Expected:
- daemon reachable
- runtimes detected
- active group list loadable

## 3) Incident Triage Order

When a group appears stuck:

1. Check daemon health.
2. Check group state (`active/idle/paused/stopped`).
3. Check actor runtime status.
4. Check message obligations (reply-required/attention ack).
5. Check automation and delivery throttling.

Useful commands:

```bash
cccc daemon status
cccc actor list
cccc inbox --actor-id <actor_id>
cccc tail -n 100 -f
```

## 4) Fast Recovery Playbook

### Actor-level recovery (preferred)

```bash
cccc actor restart <actor_id>
```

Use this before group-level restart.

### Group-level recovery

```bash
cccc group stop
cccc group start
```

### Daemon-level recovery (last resort)

```bash
cccc daemon stop
cccc daemon start
```

## 5) Secure Remote Access

Required baseline:
- Set `CCCC_WEB_TOKEN`.
- Use Cloudflare Access or Tailscale for network boundary.

Do not:
- Expose Web UI directly without an access gateway.
- Store secrets in repo files.

## 6) Upgrade Playbook (RC-safe)

### Before upgrade

1. Stop active high-risk sessions.
2. Backup `CCCC_HOME`.
3. Record current version and smoke state.

### Upgrade

```bash
python -m pip install -U cccc-pair
```

### After upgrade

```bash
cccc doctor
cccc daemon status
cccc mcp
```

Run a small end-to-end smoke:
- create/attach group
- add/start actor
- send/reply
- verify ledger and inbox behavior

## 7) Backup and Restore

### Backup (minimal)

Backup `CCCC_HOME`:
- registry
- daemon logs (optional)
- all groups (`group.yaml`, ledger, state)

### Restore

1. Stop daemon.
2. Restore `CCCC_HOME` directory.
3. Start daemon and verify with `cccc doctor`.

## 8) Operational Guardrails

- Keep one source of truth: decisions should be in CCCC messages.
- Use `reply_required` for critical asks.
- Prefer explicit recipients over broad broadcast when scope is narrow.
- Keep automation focused on objective reminders, not chat noise.

## 9) Escalation Checklist

If an issue repeats:

1. Collect evidence:
   - group id
   - actor id
   - event ids
   - recent `cccc tail -n 100`
2. Capture reproducible sequence.
3. Classify severity (`P0/P1/P2`).
4. Register fix or risk in release findings.
