# Actor And Group Lifecycle Surfaces

This note is a focused map for starting, stopping, and restarting `CCCC` groups
and actors across the three operator surfaces that matter in practice:

- `CLI`
- `HTTP`
- `MCP`

It is intentionally narrower than a full API reference. The goal is to answer:

- which command or route starts an actor?
- which HTTP path backs the Web UI button?
- which MCP tool can do the same thing?
- which daemon operation is ultimately invoked?

## Terminology Alignment

This focused note follows the local glossary:

- `group` is the lifecycle owner for attached collaboration state
- `actor` is the live scheduled participant being started, stopped, or restarted
- `profile` is reusable actor runtime configuration, not the lifecycle action itself
- `attach` still defines the group's `authoritative_workspace`
- actor lifecycle may affect runtime execution, but it does not redefine
  workspace authority or turn registry bookkeeping into runtime truth

## Source Files

The concrete sources for this mapping are:

- `docs/reference/actor-group-lifecycle-openapi.yaml`
- `src/cccc/cli/actor_cmds.py`
- `src/cccc/cli/group_cmds.py`
- `src/cccc/ports/web/routes/actors.py`
- `src/cccc/ports/web/routes/im.py`
- `src/cccc/ports/mcp/handlers/cccc_group_actor.py`
- `src/cccc/ports/mcp/server.py`
- `web/src/services/api.ts`

The sibling `actor-group-lifecycle-openapi.yaml` file is a focused HTTP excerpt
that complements this note. Use this Markdown page for cross-surface mapping,
and the YAML file when you want a machine-readable snapshot of the current HTTP
lifecycle routes.

## Quick Map

| Intent | CLI | HTTP | MCP | Daemon op |
|---|---|---|---|---|
| Start one actor | `cccc actor start <actor_id> --group <group_id>` | `POST /api/v1/groups/{group_id}/actors/{actor_id}/start` | `cccc_actor(action="start", actor_id=..., group_id=...)` | `actor_start` |
| Stop one actor | `cccc actor stop <actor_id> --group <group_id>` | `POST /api/v1/groups/{group_id}/actors/{actor_id}/stop` | `cccc_actor(action="stop", actor_id=..., group_id=...)` | `actor_stop` |
| Restart one actor | `cccc actor restart <actor_id> --group <group_id>` | `POST /api/v1/groups/{group_id}/actors/{actor_id}/restart` | `cccc_actor(action="restart", actor_id=..., group_id=...)` | `actor_restart` |
| Start group actors | `cccc group start --group <group_id>` | `POST /api/v1/groups/{group_id}/start` | no dedicated `group start` tool in current MCP surface | `group_start` |
| Stop group actors | `cccc group stop --group <group_id>` | `POST /api/v1/groups/{group_id}/stop` | `cccc_group(action="set_state", state="stopped", group_id=...)` | `group_stop` |
| Set group state active/idle/paused | `cccc group set-state <state> --group <group_id>` | `POST /api/v1/groups/{group_id}/state?state=<state>` | `cccc_group(action="set_state", state=..., group_id=...)` | `group_set_state` |

## CLI Surface

### Actor lifecycle

The CLI exposes direct actor lifecycle commands:

```bash
cccc actor start <actor_id> --group <group_id>
cccc actor stop <actor_id> --group <group_id>
cccc actor restart <actor_id> --group <group_id>
```

These commands eventually call the daemon with:

- `actor_start`
- `actor_stop`
- `actor_restart`

### Group lifecycle

The CLI exposes direct group lifecycle commands:

```bash
cccc group start --group <group_id>
cccc group stop --group <group_id>
cccc group set-state active --group <group_id>
cccc group set-state idle --group <group_id>
cccc group set-state paused --group <group_id>
cccc group set-state stopped --group <group_id>
```

Important detail:

- `set-state stopped` does not call `group_set_state`
- it is normalized to the daemon `group_stop` operation

## HTTP Surface

## Actor routes

The actor lifecycle routes are defined under the group-scoped Web router:

- `POST /api/v1/groups/{group_id}/actors/{actor_id}/start`
- `POST /api/v1/groups/{group_id}/actors/{actor_id}/stop`
- `POST /api/v1/groups/{group_id}/actors/{actor_id}/restart`

The Web UI helper methods in `web/src/services/api.ts` call exactly these paths:

- `startActor(...)`
- `stopActor(...)`
- `restartActor(...)`

### Group routes

The group lifecycle HTTP paths are:

- `POST /api/v1/groups/{group_id}/start`
- `POST /api/v1/groups/{group_id}/stop`
- `POST /api/v1/groups/{group_id}/state?state=active|idle|paused`

Important implementation detail:

- these paths are group lifecycle routes
- but today they are implemented in `src/cccc/ports/web/routes/im.py`
- this is easy to miss if you only search in `groups.py`

The Web UI helper methods are:

- `startGroup(...)`
- `stopGroup(...)`
- `setGroupState(...)`

## MCP Surface

### Actor lifecycle via `cccc_actor`

The current MCP tool surface supports actor lifecycle through one consolidated
tool:

- `cccc_actor`

Supported actions relevant here:

- `list`
- `add`
- `remove`
- `start`
- `stop`
- `restart`

Example:

```json
{
  "group_id": "g_xxx",
  "action": "start",
  "actor_id": "peer-impl"
}
```

### Group lifecycle via `cccc_group`

The current MCP tool surface does not expose a dedicated `group start` action.
Instead:

- `cccc_group(action="set_state", state="active" | "idle" | "paused")`
  maps to `group_set_state`
- `cccc_group(action="set_state", state="stopped")`
  maps to `group_stop`

That means:

- MCP can stop a group
- MCP can change non-stopped group state
- but current MCP does not have a first-class `group_start` action

If an automation or external controller needs to start a whole group today, the
available stable surfaces are:

- `CLI`
- `HTTP`
- direct daemon op from internal code

## Web UI Notes

If you are debugging button behavior in the browser:

- actor buttons call actor lifecycle HTTP routes
- group buttons call group lifecycle HTTP routes
- both eventually route into daemon ops

The Web UI client functions are the fastest file to inspect first:

- `web/src/services/api.ts`

## Practical Examples

### Start one actor with CLI

```bash
cccc actor start peer-arch --group g_example123
```

### Start one actor with HTTP

```bash
curl -X POST \
  "http://127.0.0.1:8848/api/v1/groups/g_example123/actors/peer-arch/start?by=user"
```

### Restart one actor with HTTP

```bash
curl -X POST \
  "http://127.0.0.1:8848/api/v1/groups/g_example123/actors/peer-arch/restart?by=user"
```

### Start a group with HTTP

```bash
curl -X POST \
  "http://127.0.0.1:8848/api/v1/groups/g_example123/start?by=user"
```

### Stop a group with MCP semantics

```json
{
  "group_id": "g_example123",
  "action": "set_state",
  "state": "stopped"
}
```

## Scope Boundary

This document only covers lifecycle entry points for:

- `group start`
- `group stop`
- `group state`
- `actor start`
- `actor stop`
- `actor restart`

It does not try to document:

- the full HTTP API
- WebSocket terminal streaming
- IM bridge routes
- actor-profile CRUD surfaces
- attach / authoritative-workspace semantics beyond the lifecycle boundary
- execution-workspace resolution policy
- registry lookup behavior
- `resume`-specific launch metadata
- PTY keystroke injection or raw terminal input delivery

## Related Glossary

- [group](/reference/glossary/group)
- [actor](/reference/glossary/actor)
- [profile](/reference/glossary/profile)
- [attach](/reference/glossary/attach)
- [authoritative_workspace](/reference/glossary/authoritative_workspace)
- [execution_workspace](/reference/glossary/execution_workspace)
- [registry](/reference/glossary/registry)
- [status](/reference/glossary/status)

## Change Log

- `2026-03-23`: Added local glossary alignment so lifecycle entry-point docs stop being misread as the source of truth for profiles, workspace authority, or registry semantics.
