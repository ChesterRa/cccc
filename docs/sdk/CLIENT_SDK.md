# CCCC Client SDK

The official SDK for integrating external apps/services with a running CCCC daemon.

## Repository and Packages

- Repository: [ChesterRa/cccc-sdk](https://github.com/ChesterRa/cccc-sdk)
- Python package: `cccc-sdk` (import as `cccc_sdk`)
- TypeScript package: `cccc-sdk`

## How It Fits with CCCC Core

CCCC core (`cccc-pair`) is the runtime system:

- daemon
- ledger/state
- Web/CLI/MCP/IM ports

The SDK is a client layer:

- it does not start/own daemon state
- it connects to an existing daemon
- it uses the same control-plane semantics as Web/CLI/MCP

## When to Use SDK vs MCP

Use SDK when you are building:

- backend services
- bots
- IDE integrations
- automation services outside the agent runtime

Use MCP when the caller is an in-session agent/tool runtime.

## Install

```bash
# Python
pip install -U cccc-sdk

# TypeScript
npm install cccc-sdk
```

## Runtime Requirement

A CCCC daemon must already be running.

```bash
cccc daemon status
```

The SDK client then connects to the daemon transport configured by your CCCC runtime (`CCCC_HOME`, daemon socket/TCP settings).

## Integration Model

Typical production setup:

1. Run CCCC core (`cccc-pair`) as the local control plane.
2. Connect your app/service through the SDK.
3. Use SDK calls for group/actor/messaging/context/automation operations.
4. Keep operational truth in the CCCC ledger and group state.

## Compatibility Notes

- SDK and core are released independently, but should stay on the same major/minor line for best compatibility.
- For protocol-level details, see:
  - `docs/standards/CCCS_V1.md`
  - `docs/standards/CCCC_DAEMON_IPC_V1.md`

## Next

For concrete API examples and language-specific usage, follow the SDK repo documentation:

- [cccc-sdk README](https://github.com/ChesterRa/cccc-sdk)
