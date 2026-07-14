# Capability Governance

The Rust capability store combines built-in packs, imported capsules, per-group bindings, visibility, and explicit block state.

## Operations

```text
capability_search
capability_state
capability_overview
capability_enable
capability_visibility
capability_import
capability_install_target
capability_tool_call
capability_block
capability_uninstall
capability_source_delete
capability_allowlist_get
capability_allowlist_validate
capability_allowlist_update
capability_allowlist_reset
```

Global policy mutation requires `by=user`. Group block/unblock follows role permissions. Blocking a capability removes its active group binding.

Policy and imported capability state live under `CCCC_RUST_HOME`; there is no legacy `CCCC_HOME` overlay in the Rust implementation.

## Recommended Flow

1. Search or inspect capability state.
2. Validate the intended policy change.
3. Apply it with the expected revision when provided.
4. Enable only the capability needed for the current group.
5. Re-list MCP tools after changing visibility or bindings.

Use reset to return to the packaged Rust defaults.
