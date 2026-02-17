# SDK Overview

Use the official SDK when you need to integrate CCCC with external applications and services.

## Official SDK

- Repository: [ChesterRa/cccc-sdk](https://github.com/ChesterRa/cccc-sdk)
- Python package: `cccc-sdk` (import as `cccc_sdk`)
- TypeScript package: `cccc-sdk`

## Install

```bash
pip install -U cccc-sdk
npm install cccc-sdk
```

## Relationship to CCCC Core

- CCCC core (`cccc-pair`) is the runtime control plane (daemon + ledger + ports).
- SDK is a client interface to that running control plane.
- SDK does not replace core and does not persist state on its own.

## Next

- [Client SDK](./CLIENT_SDK)
