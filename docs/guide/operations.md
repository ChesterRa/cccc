# Operations Runbook

## Runtime Layout

```text
CCCC_HOME=~/.cccc
~/.cccc/.cccc-rust-v1
~/.cccc/daemon/ccccd.addr.json
~/.cccc/groups/<group_id>/group.yaml
~/.cccc/groups/<group_id>/ledger.jsonl
~/.cccc/groups/<group_id>/state/
```

Python and Rust use this same home. Stop the active daemon before switching implementations, and never run both daemons concurrently.

`cccc daemon stop` terminates local actor runtime sessions before releasing the daemon lock. A Web process started by `cccc` observes daemon loss and exits instead of remaining bound with a broken API.

## Start And Health

```bash
cccc daemon start
cccc daemon status
cccc status
cccc doctor
cccc
```

Web health endpoints:

```bash
curl -fsS http://127.0.0.1:8848/api/v1/health
curl -fsS http://127.0.0.1:8848/api/v1/ready
```

## Triage Order

1. Run `cccc doctor` and `cccc daemon status`.
2. Inspect the group with `cccc active`, `cccc group show`, and `cccc actor list`.
3. Inspect the ledger with `cccc tail -n 100`.
4. Restart only the affected actor with `cccc actor restart <id>`.
5. Restart the group if multiple actors are stale.
6. Restart the daemon only after group-level recovery fails.

Runtime output is visible through the Web terminal and daemon terminal operations. Terminal output is not a delivered chat message.

## Backup

Stop writers and archive the complete Rust Home:

```bash
cccc daemon stop
tar -C "$HOME" -czf "cccc-rust-backup-$(date +%Y%m%d-%H%M%S).tar.gz" .cccc
```

Restore the complete archive while all CCCC daemons are stopped. Keep the `.cccc-rust-v1` marker with the rest of the home.

## Upgrade

1. Back up `CCCC_HOME`.
2. Download the new GitHub Release archive for the current platform.
3. Replace all four binaries together.
4. Run `cccc version`, `cccc doctor`, and `cccc status`.
5. Start the Web UI and test one message/read cycle.

There is no in-process self-updater. This keeps release replacement explicit and rollbackable.

## Access Tokens

The first administrator token can bootstrap Web login. Once tokens exist, anonymous API requests are rejected. Bind the Web service to loopback unless a reverse proxy or tunnel provides TLS and access control.

Group-scoped tokens cannot access other groups. Administrative endpoints require an administrator token.

## Group Bridge

Pairing flow:

1. Issuer creates an invite in Settings.
2. Requester submits connection info.
3. Issuer approves and creates a scoped registration.
4. Requester synchronizes the outbound and claims the credential using the pairing capability.
5. Both sides use idempotent delivery receipts.

Credentials are stored only in Rust Home and removed from status/list responses. Use `messages` for ordinary collaboration, `read` for bounded inspection, and `full` only for a trusted peer that may modify the workspace.

## Group Space

```bash
cccc space status
cccc space auth status
cccc space sync --lane work
cccc space query "current blockers" --lane work
```

Provider health reports its mode. A degraded response means the local source/ledger search path was used; it is not reported as a successful remote NotebookLM query.

## Voice

Browser ASR is the default usable transcription path. Service-local transcription returns `asr_unavailable` when no backend command/runtime is configured. Do not treat that response as an empty transcript.

## IM

`cccc im status` distinguishes configured, enabled, and running. Configuration alone is not evidence that an external platform connection exists. Validate inbound and outbound delivery on the selected platform before relying on it for operations.

## Release Verification

```bash
scripts/pre_commit_checks.sh
scripts/build_package.sh
docker build -f docker/Dockerfile .
```

For a release candidate, also verify Linux, macOS, and Windows CI jobs and test existing-home adoption without changing existing files.
