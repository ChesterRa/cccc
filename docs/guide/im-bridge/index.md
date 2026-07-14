# IM Bridge

The Rust control plane stores configuration and chat authorization state for Telegram, Slack, Discord, Feishu/Lark, DingTalk, WeCom, and Weixin.

## Status Semantics

| State | Meaning |
|---|---|
| configured | Required platform fields were accepted and stored in Rust Home |
| enabled | The group requested the bridge to run |
| running | A real network adapter is active |
| authorized | A chat passed the explicit pairing/subscription flow |

The Rust package does not report `running=true` merely because configuration exists. Platform network adapters must be available and validated independently.

## CLI

```bash
cccc im set <platform> [credential options] [--group ID]
cccc im config [--group ID]
cccc im start [--group ID]
cccc im stop [--group ID]
cccc im status [--group ID]
cccc im pending [--group ID]
cccc im bind --key KEY [--group ID]
cccc im authorized [--group ID]
cccc im reject --key KEY [--group ID]
cccc im revoke --chat-id ID [--thread-id N] [--group ID]
```

Secrets should be supplied through environment variable names rather than literal values. Configuration is isolated per group under `CCCC_RUST_HOME`.

## Security

- One configuration belongs to one group.
- Pending chats require explicit approval.
- Revocation removes the chat from the authorized list.
- The daemon remains the collaboration source of truth; adapters are transport ports.
- Test both inbound and outbound delivery before using an IM platform for operational control.

The platform-specific pages in this directory describe provider-side bot creation. Treat any adapter-specific command in an older page as legacy until the Rust status endpoint reports a real worker.
