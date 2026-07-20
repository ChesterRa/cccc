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

Secrets should be supplied through environment variable names rather than literal values. Configuration is isolated per group under `CCCC_HOME`.

## Chat Subscription

Send `/subscribe` to the bot. The bot immediately replies with the real 12-character pairing key, which expires after 10 minutes. Repeating `/subscribe` from the same chat and platform during that window returns the same pending key instead of creating duplicates.

Approve the request from Web **Pending Requests**, or use:

```bash
cccc im pending --group GROUP_ID
cccc im bind --key KEY --group GROUP_ID
```

The bot also replies immediately to `/unsubscribe`, `/pause`, `/resume`, `/verbose [on|off]`, `/status`, `/help`, invalid `/send` usage, and unknown commands. `/verbose` is idempotent: no argument or `on` enables it, while `off` disables it.

## Security

- One configuration belongs to one group.
- Pending chats require explicit approval.
- Revocation removes the chat from the authorized list.
- The daemon remains the collaboration source of truth; adapters are transport ports.
- Test both inbound and outbound delivery before using an IM platform for operational control.

The platform-specific pages in this directory describe provider-side bot creation. Treat any adapter-specific command in an older page as legacy until the Rust status endpoint reports a real worker.
