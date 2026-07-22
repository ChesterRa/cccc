# CLI Reference

The executable is `cccc`. Commands emit formatted JSON unless the result is naturally a path, version, or short status.

## Global

```bash
cccc [--host HOST] [--port PORT] [COMMAND]
cccc --help
cccc version
cccc home
cccc status
cccc doctor
```

With no command, `cccc` starts the Rust daemon when needed and serves the Web UI. `--host` and `--port` override `CCCC_WEB_HOST` and `CCCC_WEB_PORT`.

## Daemon

```bash
cccc daemon start
cccc daemon stop
cccc daemon status
cccc daemon run
```

`run` stays in the foreground. Unix uses a socket under Rust Home; Windows and `CCCC_DAEMON_TRANSPORT=tcp` use loopback TCP.

On Linux, the Python runtime report includes projected-browser readiness for its headed browser:
system Chrome/Edge, required `Xvfb` isolation, and optional `x11vnc`. The Rust runtime uses a
headless Chromium surface instead; its report identifies the discovered browser and explicitly
reports `xvfb_required=false` because it never attaches that surface to the host desktop.

## Groups

```bash
cccc group create --title TITLE [--topic TOPIC]
cccc groups
cccc group show <group_id>
cccc group use <group_id> [path]
cccc use <group_id>
cccc active
cccc attach [path] [--group <group_id>]
cccc group update [--group ID] [--title TITLE] [--topic TOPIC]
cccc group start [--group ID]
cccc group stop [--group ID]
cccc group set-state active|idle|paused|stopped [--group ID]
cccc group detach-scope <scope_key> [--group ID]
cccc group reset [--group ID] --confirm <group_id>
cccc group delete [--group ID] --confirm <group_id>
```

Commands with optional `--group` use the active group.

## Actors

```bash
cccc actor list [--group ID]
cccc actor add <actor_id> [--runtime codex] [--runner pty|headless]
  [--title TITLE] [--command "COMMAND"] [--env KEY=VALUE] [--scope KEY]
cccc actor update <actor_id> [--runtime NAME] [--runner MODE]
  [--title TITLE] [--command "COMMAND"] [--env KEY=VALUE]
cccc actor start|stop|restart|remove <actor_id> [--group ID]
cccc actor secrets <actor_id> [--set KEY=VALUE] [--unset KEY] [--clear]
cccc prompt <actor_id> [--group ID]
```

`runtime=web_model` uses browser delivery and remote MCP, so it has no local child process.

## Messaging

```bash
cccc send TEXT [--group ID] [--to TARGET] [--priority normal|attention]
cccc tracked-send TEXT [--group ID] [--to TARGET]
cccc reply <event_id> TEXT [--group ID]
cccc inbox --actor-id ID [--group ID] [--limit N]
cccc read <event_id> --actor-id ID [--group ID]
cccc tail [--group ID] [-n N]
cccc ledger snapshot [--group ID]
cccc ledger compact [--group ID] [--dry-run]
```

Repeat `--to` for multiple recipients. Recipient tokens include actor IDs, `@all`, `@peers`, and `@foreman`.

## IM Control

```bash
cccc im set <platform> [credential options] [--group ID]
cccc im config|start|stop|status [--group ID]
cccc im bind --key KEY [--group ID]
cccc im pending|authorized [--group ID]
cccc im reject --key KEY [--group ID]
cccc im revoke --chat-id ID [--thread-id N] [--group ID]
cccc im logs [-n LINES] [-f] [--group ID]
```

Platforms: `telegram`, `slack`, `discord`, `feishu`, `dingtalk`, `wecom`, `weixin`. Credential options store environment-variable names or direct secrets in Rust Home. The Web control plane starts the corresponding Rust network adapter and verifies credentials where the provider supports an explicit validation call. Missing or rejected credentials fail without fabricating a running adapter.

`cccc im ...` controls the long-running Web process at `CCCC_WEB_HOST` / `CCCC_WEB_PORT` (default `127.0.0.1:8848`). Start `cccc` first; the short-lived CLI process does not own persistent IM connections. Global `--host` and `--port` options can target a non-default local Web listener.

## Group Space

```bash
cccc space status [--group ID] [--provider notebooklm]
cccc space bind [remote_space_id] --lane work|memory [--group ID]
cccc space unbind --lane work|memory [--group ID]
cccc space sync --lane work|memory [--group ID] [--force]
cccc space ingest --lane work|memory [--payload JSON] [--idempotency-key KEY] [--provider notebooklm|local]
cccc space query QUERY --lane work|memory [--options JSON] [--provider notebooklm|local]
cccc space sources --lane work|memory [--action list|rename|delete]
  [--source-id ID] [--new-title TITLE]
cccc space jobs --lane work|memory [--action list|retry|cancel] [--job-id ID]
cccc space auth [status|start|cancel|disconnect]
cccc space credential status
cccc space credential set (--auth-json JSON | --auth-json-file PATH)
cccc space credential clear
cccc space health
```

NotebookLM is the default provider. Configure a Playwright storage-state JSON
with `cccc space credential set --auth-json-file PATH`, run `cccc space health`,
then bind a notebook. Omitting `remote_space_id` on `space bind` creates a new
notebook. Local operations require explicit `--provider local` and return a
degraded marker; there is no silent fallback. NotebookLM artifact and full sync
operations remain explicitly unavailable.

## Runtime, MCP, And Web

```bash
cccc runtime list
cccc runtime hermes status
cccc runtime hermes prepare --yes
cccc runtime hermes mcp-test
cccc setup
cccc mcp
cccc web
```

`setup` prints an MCP configuration whose command is the current executable and whose environment points to `CCCC_HOME`. `mcp` runs stdio MCP. `web` is equivalent to the default launch.

## Environment

| Variable | Default | Purpose |
|---|---|---|
| `CCCC_HOME` | `~/.cccc` | Rust runtime home |
| `CCCC_WEB_HOST` | `127.0.0.1` | Web bind host |
| `CCCC_WEB_PORT` | `8848` | Web port |
| `CCCC_DAEMON_TRANSPORT` | platform default | `unix` or `tcp` |
| `CCCC_DAEMON_HOST` | `127.0.0.1` | TCP daemon host |
| `CCCC_DAEMON_PORT` | random | TCP daemon port |
| `CCCC_GROUP_ID` | active group | MCP group context |
| `CCCC_ACTOR_ID` | none | MCP actor context |
| `RUST_LOG` | normal | Rust tracing filter |

Python and Rust use the same `CCCC_HOME`. Stop the active daemon before switching implementations.
