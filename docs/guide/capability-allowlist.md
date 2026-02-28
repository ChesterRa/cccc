# Capability Allowlist (A1/A2)

This guide documents the release behavior of capability governance.

## 1. Files and Merge Model

Allowlist policy is always composed from two layers:

1. packaged default: `src/cccc/resources/capability-allowlist.default.yaml`
2. user overlay: `CCCC_HOME/config/capability-allowlist.user.yaml`

Effective policy is deterministic merge:

- `effective = merge(default, overlay)`
- mapping keys are merged recursively
- non-mapping values (including lists) are replaced by overlay value

No env/path override compatibility remains. Use overlay APIs only.

## 2. Policy Levels

Supported level values:

1. `indexed`: kept in local catalog, hidden from normal discovery/exposure
2. `mounted`: searchable/discoverable, can be enabled on demand
3. `enabled`: mounted + preferred baseline intent (does not auto-mutate runtime state by itself)
4. `pinned`: enabled + long-lived baseline intent (typically applied via actor-profile/role defaults)

Runtime mutation still happens only through explicit enable flows (`capability_enable` / `capability_use`) or actor profile default application on actor start.

Release baseline policy:

1. third-party sources are discoverable by default (`mounted`)
2. runtime flow has no approval gate; enable path is one-step when `enable_supported=true`
3. `blocked` is explicit deny state (policy/source/runtime blocklist), reserved for clear deny signals

Runtime blocklist model:

1. `scope=group`: foreman or user can block/unblock capabilities for this group
2. `scope=global`: only user can block/unblock capabilities globally
3. block action immediately revokes bindings and runtime tool visibility (relist/reconnect required)

## 3. Runtime Discovery Model

Runtime path does not run daemon-side periodic source sync loops.

Discovery flow:

1. search local curated catalog first
2. optionally augment from remote fallback when local results are insufficient:
   - MCP: official MCP Registry
   - skill: GitHub / AgentSkills-aligned / SkillsMP / ClawHub remote search
3. cache accepted remote hits into local catalog

This keeps runtime stable while preserving long-tail discoverability.

## 4. Daemon Ops (Global Overlay)

Use these IPC ops for allowlist governance:

1. `capability_allowlist_get`
2. `capability_allowlist_validate`
3. `capability_allowlist_update`
4. `capability_allowlist_reset`

Update/reset require `by="user"`.

`capability_allowlist_update` supports optimistic concurrency via `expected_revision`.

## 5. Web API Endpoints

Web routes map to daemon ops:

1. `GET /api/v1/capabilities/allowlist`
2. `POST /api/v1/capabilities/allowlist/validate`
3. `PUT /api/v1/capabilities/allowlist`
4. `DELETE /api/v1/capabilities/allowlist`

## 6. Recommended Workflow

1. call `capability_allowlist_get`
2. prepare patch (or full replacement)
3. call `capability_allowlist_validate`
4. apply with `capability_allowlist_update(expected_revision=...)`
5. if mismatch, refetch and retry

Use `capability_allowlist_reset` to drop overlay and return to packaged default.
