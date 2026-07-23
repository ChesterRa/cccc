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
- PTY actor delivery coalesces short bursts into one submitted batch while
  retaining one ledger event and read-cursor completion per message. The
  canonical `settings.min_interval_seconds` value controls longer throttling;
  Rust also honors the legacy Python `delivery.min_interval_seconds` field.
- `runner=headless` actors do not own terminal processes. Their runtime consumes
  work through `cccc_runtime_wait_next_turn` and commits a contiguous event
  prefix through `cccc_runtime_complete_turn`.

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

The Rust build connects directly to NotebookLM without a Python sidecar. It
supports notebook selection/creation, text ingest, source management, and
grounded query. Local fallback must still be selected explicitly with
`--provider local`.

The Rust authentication worker owns saved-session validation, projected browser
startup, timeout/cancellation, credential verification, and temporary profile
cleanup. It also captures provider `Set-Cookie` rotation after API calls. Stored
credentials are updated atomically; `CCCC_NOTEBOOKLM_AUTH_JSON` is always treated
as a read-only operational override.

### Validate control plane

```bash
cccc space credential status
cccc space health
```

Maintainers can run the opt-in live protocol smoke test without placing account
data in fixtures:

```bash
CCCC_NOTEBOOKLM_AUTH_JSON="$(cat /path/to/storage-state.json)" \
  cargo test -p cccc-notebooklm --test live -- --ignored
```

### Validate curated context export path

After a `context_sync` update (`vision.update` / `overview.manual.update` / `task.*` / `agent.*`), check queue:

```bash
cccc space jobs --lane work --action list
```

Expected: a `kind=context_sync` job appears for bound groups.

### Validate repo `space/` reconciliation

```bash
cccc space sync --lane work --force
```

Expected in the current Rust build: `provider_unavailable` because full repo
reconciliation is intentionally not enabled yet.

### Safe rollback (core workflows keep running)

Expected behavior:

- NotebookLM artifact and full-sync operations return `provider_unavailable`.
- Notebook/source/query operations fail loudly on auth expiry or protocol drift.
- Explicit `--provider local` operations return `degraded=true` and never claim remote delivery.
- Core CCCC chat/task/actor workflows continue normally.

## 11) Release Verification

Python distribution:

```bash
scripts/pre_commit_checks.sh --full
scripts/build_package.sh
docker build -f docker/Dockerfile .
```

Rust distribution:

```bash
scripts/build_package_rust.sh
docker build -f docker/Dockerfile.rust .
```

Python releases use `.github/workflows/release.yml`. Rust releases use
`.github/workflows/release-rust.yml` and `rust-v<version>` tags.
