# FAQ

## How do I install CCCC?

Download the archive for your platform from GitHub Releases and put `cccc`, `ccccd`, `cccc-mcp`, and `cccc-web` on `PATH`. For source builds:

```bash
npm ci --prefix web
npm -C web run build
cargo build --workspace --release --locked
```

The release archive does not require Python.

## Where is data stored?

Rust uses `CCCC_RUST_HOME`, default `~/.cccc-rust`. It refuses the legacy `~/.cccc` directory and every child of it. Run `cccc home` to see the effective path.

## How do I switch back to the old implementation?

```bash
git switch python
```

Switch back with `git switch rust`. Each branch uses its own home. No automatic migration occurs.

## How do I check health?

```bash
cccc version
cccc doctor
cccc daemon status
cccc status
```

## Which runtimes are recognized?

Claude, Codex, Copilot, Cursor, Devin, Kiro, Kilo, Antigravity, Droid, Grok, Hermes, Kimi, OpenCode, Amp, Auggie, Web Model, and custom commands. Run `cccc runtime list` to inspect commands detected on this machine.

## What is the difference between foreman and peer?

The first enabled actor is the default foreman. A foreman can coordinate group-level work. Peers manage their own work and communicate through the same ledger and MCP tools.

## Why will an actor not start?

1. Run `cccc actor list` and `cccc doctor`.
2. Verify the runtime executable is on `PATH`.
3. Inspect terminal output in the Web UI.
4. Run `cccc actor restart <actor_id>`.

`runtime=web_model` does not start a local process; open its browser/connector setup instead.

## How do read receipts work?

An inbox cursor is cumulative per actor. Marking an event read advances the cursor through that event and appends a visible `chat.read` ledger event. Attention ACK is separate.

## How do I expose the Web UI safely?

Create an administrator token in Settings > Web Access before exposing the service. Keep the default loopback bind when possible. Use TLS and an authenticated tunnel or reverse proxy for remote access.

## Does Group Bridge expose my whole machine?

No. Pairing creates an explicit trust for one group. `messages` allows message delivery, `read` adds bounded read tools, and `full` permits the broader scoped MCP surface. Credentials are removed from status responses. Grant `full` only to a trusted peer.

## Why does Group Space say degraded?

The provider adapter is unavailable or not authenticated, so CCCC used its local source/ledger query path. The response intentionally does not pretend a remote NotebookLM call succeeded.

## Why does local voice transcription return `asr_unavailable`?

No local ASR backend is configured. Browser ASR remains the default usable path. An unavailable response must not be treated as an empty transcript.

## Why is IM configured but not running?

Configuration and authorization state do not prove an external platform worker exists. CCCC reports these states separately and does not fabricate a live adapter. Validate real inbound/outbound delivery for the selected platform.

## Port 8848 is unavailable

```bash
CCCC_WEB_PORT=9000 cccc
```

On Windows, inspect reserved TCP ranges when no process owns the port:

```powershell
netsh interface ipv4 show excludedportrange protocol=tcp
```

## MCP is not working

```bash
cccc setup
cccc daemon status
cccc mcp
```

Confirm the runtime configuration uses the current `cccc` executable and `CCCC_RUST_HOME`, plus the intended `CCCC_GROUP_ID` and `CCCC_ACTOR_ID`.

## The ledger is large

```bash
cccc ledger snapshot
cccc ledger compact
```

Back up Rust Home before compaction. Blobs are stored separately from the JSONL ledger.
