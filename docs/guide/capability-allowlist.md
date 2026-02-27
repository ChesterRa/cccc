# Capability Allowlist Baseline

This guide documents the baseline capability allowlist policy draft:

- `src/cccc/resources/capability-allowlist.default.yaml`

The baseline is designed for CCCC's dynamic capability model and uses four levels:

- `indexed`: synced in local catalog only; hidden from normal discovery.
- `mounted`: discoverable/searchable; can be enabled on demand.
- `enabled`: available by default for runtime use.
- `pinned`: actor-role baseline (long-lived enable).

## Why this baseline

- Keep default surface large enough to be useful, but not noisy.
- Preserve safety for high-risk domains (`computer_use`, `trading`, `payment_write`, `terminal_exec`).
- Match current installer reality (`remote_only` + `package:npm` first).
- Keep official Anthropic skills broadly available, with minimal role pinning.

## Included sources

- `cccc_builtin`
- `anthropic_skills`
- `mcp_registry_official`
- `agentskills_validator` (validator role, not a market feed)

## Curation strategy

- Default source levels:
  - `cccc_builtin`: `enabled`
  - `anthropic_skills`: `mounted`
  - `mcp_registry_official`: `indexed` (curated overrides promoted to `mounted`/`enabled`)
  - `agentskills_validator`: `indexed`
- Curated MCP set promoted to `mounted`/`enabled` for high-utility paths.
- High-risk or unsupported-installer MCP entries remain `indexed` by default.
- Anthropic skills are `mounted` by default; a small foreman baseline is `pinned`.

## Maintenance workflow (recommended)

1. Keep new external entries at `indexed` first.
2. Promote to `mounted` only after smoke test + auth check.
3. Promote to `enabled` only after stability and context budget checks.
4. Use `pinned` minimally to avoid prompt bloat and behavior drift.

