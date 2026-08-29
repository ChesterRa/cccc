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
4. Check runtime delivery, Inbox read state, pending replies, and tracked tasks.
5. Check automation and delivery policy.

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
- Create an **Admin Access Token** in **Settings > Web Access** before any non-local exposure.
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
# Website-installer ownership
cccc update

# Pip ownership
python -m pip install -U "cccc-pair>=0.4.36"
```

Do not remove the v0.4.36 lower bound. Before a pip upgrade, run
`cccc daemon stop` and close any
foreground CCCC process so the executable is replaceable on every platform.
Do not layer the website installer over a pip-owned command. To switch
installation channels in the same directory, uninstall `cccc-pair` with pip
first; the installer refuses `pip-v1` ownership even when
`CCCC_ALLOW_REPLACE_EXISTING=1` is set.

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
- Use `message_mode=request_reply` only for a concrete recipient whose reply is
  required; use Mail for useful but non-urgent context.
- Mail is agent-only. Address either `user` alone or one/more agents in each
  message; split messages instead of mixing those audiences.
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

## 10) Group Space (NotebookLM) Runbook

### Activate the provider

Connect Google from the Notebook settings in CCCC Web. That flow stores the
credential and provider state used by the daemon. No feature toggle or
environment variable is required.

### Validate control plane

```bash
cccc space credential status
cccc space health
```

### Validate explicit local-file ingestion

Create a supported file under the attached scope and ingest it explicitly.
The native daemon rejects paths outside that scope before any provider write.

```bash
cccc space ingest --kind resource_ingest \
  --payload '{"source_type":"file","file_path":"space/spec.md","title":"Spec"}'
```

Expected: the result contains the created NotebookLM `source_id`.

### Disconnect safely (core workflows keep running)

Use **Disconnect Google** in the Notebook settings to remove this machine's
stored credential and disable provider access from this installation.

Expected after disconnect:

- Group Space operations may return degraded/disabled provider results.
- Core CCCC chat/task/actor workflows continue normally.

Optional throughput tuning:

```bash
export CCCC_SPACE_PROVIDER_MAX_INFLIGHT=1   # safer
export CCCC_SPACE_PROVIDER_MAX_INFLIGHT=4   # faster
```
