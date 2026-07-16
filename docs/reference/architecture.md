# Architecture

CCCC uses a Rust workspace behind the existing React/TypeScript application. The daemon owns mutable collaboration state; CLI, Web, and MCP are ports over the same versioned operations.

## Runtime Topology

```text
browser / CLI / MCP / remote connector
                 |
       cccc (Web / CLI / MCP modes)
                 |
        cccc-client + daemon IPC v1
                 |
       cccc daemon run (same executable)
                 |
 group.yaml / ledger.jsonl / state / blobs / memory
                 |
          CCCC_HOME only
```

The public distribution has one executable. `cccc` starts the daemon when needed, serves the Web application, exposes MCP through `cccc mcp`, and provides explicit `cccc daemon` and `cccc web` modes. The workspace keeps focused crates and internal development binaries without making them installation dependencies.

## Workspace Boundaries

| Crate | Responsibility |
|---|---|
| `cccc-contracts` | IPC requests/responses, actors, events, enums |
| `cccc-core` | Rust Home, registry, groups, ledger, scopes, inbox, memory, policy |
| `cccc-runtime` | PTY and headless child-process sessions |
| `cccc-client` | Unix-socket or loopback-TCP daemon client |
| `cccc-daemon` | Single-writer operations and actor lifecycle |
| `cccc-mcp` | MCP catalog, daemon mapping, scoped repo/shell tools |
| `cccc-web` | HTTP/WebSocket API, browser surfaces, embedded Web assets |
| `cccc-cli` | Command parsing and user-facing workflows |

Dependencies point inward toward contracts and core. Ports do not write group files directly except for focused integration stores that use `cccc-core` atomic state APIs.

## Rust Home

`HomeLayout` resolves only `CCCC_HOME`, defaulting to `~/.cccc`.

```text
~/.cccc/
  .cccc-rust-v1
  registry.json
  settings.json
  daemon/
  groups/<group_id>/
    group.yaml
    ledger.jsonl
    context/
    scopes/
    state/
    state/blobs/
  browser-profiles/
```

Initialization adopts an existing CCCC layout in `~/.cccc` and adds the Rust compatibility marker without replacing existing files. A non-empty custom directory that has neither a CCCC marker, registry, nor groups directory is rejected.

## Group And Ledger

A group contains metadata, scopes, actors, automation policy, and namespaced integration state. Durable events use an append-only JSONL ledger:

```json
{
  "v": 1,
  "id": "event-id",
  "ts": "2026-07-14T00:00:00Z",
  "kind": "chat.message",
  "group_id": "g_example",
  "scope_key": "scope_example",
  "by": "user",
  "data": {"text": "hello", "to": ["@foreman"]}
}
```

Inbox read state is a per-actor cursor. A `chat.read` event records the visible read transition. Attention acknowledgements are separate ledger events.

## Actors And Runtime

Actors declare a runtime, runner, command, environment, scope, submit behavior, and profile. `cccc-runtime` holds live sessions in process:

- `pty`: terminal-backed interactive process.
- `headless`: structured process output without an interactive terminal.
- `web_model`: browser/remote-MCP actor with no local child process.

Lifecycle changes update both the process session and the durable actor document. Private actor/profile environment values are stored separately from public actor metadata.

## Ports

### CLI

The CLI resolves an active group, validates local arguments, and delegates to daemon operations. It does not implement alternate persistence.

### MCP

MCP tool names map to daemon operations or bounded scope-local tools. Web Model connectors inject an immutable group/actor binding. Group Bridge MCP injects the trusted group and filters tools by `messages`, `read`, or `full` access.

### Web

Axum serves JSON APIs, terminal and browser WebSockets, SSE headless output, Group Bridge sessions, and embedded Vite assets. Access tokens support administrator and group-scoped principals.

## Integrations

- Browser profiles and integration state live under Rust Home.
- Group Bridge credentials never appear in list/status responses.
- Cross-group delivery is idempotent and records provenance on both sides.
- Group Space and Assistant state use named atomic namespaces.
- Browser ASR is the default available voice path. Missing local ASR returns an explicit unavailable state.
- IM control state never claims a network worker is live unless one is actually running.

## Concurrency And Atomicity

The daemon serializes IPC requests. Core JSON/YAML writers use temporary-file replacement. Ledger writes append complete JSON lines. Integration state updates mutate one namespace through the group/global store instead of using ad hoc files.

## Security Boundaries

- Python and Rust share the CCCC storage contracts; only one daemon may write `CCCC_HOME` at a time.
- Scope-local file operations reject absolute paths, `..`, and symlink escapes.
- Web Model connectors cannot change their bound group.
- Group Bridge disables HTTP redirects, uses bearer credentials, validates source group identity, and filters MCP tools by trust level.
- Remote Access defaults to loopback and requires explicit configuration.
- Web access tokens are required once token authentication has been configured.

## Build And Release

Vite builds `web/dist`; `rust-embed` includes it in the Rust distribution. GitHub Releases publish one single-executable archive per supported target, a four-archive `SHA256SUMS` manifest, and version-pinned Unix and Windows installers. The installed `cccc` process self-launches its daemon mode, so no sibling helper executable is required. The standalone release requires neither Node nor Python. The Docker image still includes Node-based agent CLIs for its managed runtime environment, but no Python backend.
