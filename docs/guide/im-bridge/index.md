# IM Bridge Overview

Bridge your CCCC working group to popular IM platforms for mobile access.

## What is IM Bridge?

The IM Bridge allows you to:

- Send messages to agents from your phone
- Receive updates and notifications
- Control the group with slash commands
- Share files and attachments

## Supported Platforms

| Platform | Status | Progressive output | Long final replies |
|----------|--------|--------------------|--------------------|
| [Telegram](./telegram) | ✅ | Edited message | Lossless 4,096-character chunks |
| [Slack](./slack) | ✅ | `chat.update` | Lossless 4,000-character chunks |
| [Discord](./discord) | ✅ | Edited message | Lossless 2,000-character chunks |
| [Feishu/Lark](./feishu) | ✅ | Edited message (with `im:message:update`) | Lossless 30,720-character chunks |
| [DingTalk](./dingtalk) | ✅ | AI Card Streaming | Lossless 4,096-character / 64-line chunks |
| [WeCom](./wecom) | ✅ | Native stream reply | Lossless 2,048-character / 64-line chunks |
| Weixin / WeChat | ✅ | Final messages only | Lossless 4,000-character chunks |

## Design Principles

- **1 Group = 1 Bot**: Each working group connects to one bot instance for simplicity and isolation
- **Explicit authorization**: Standard bot chats use `/subscribe`; the account that confirms a Weixin QR login is authorized immediately
- **Thin ports**: IM bridges forward messages and commands; the daemon remains the single source of truth

## Common Commands

Once authorized, these commands work across platforms:

| Command | Description |
|---------|-------------|
| `/send <message>` | Send to foreman (default) |
| `/send @<actor> <message>` | Send to specific actor |
| `/send @all <message>` | Send to all agents |
| `/send @peers <message>` | Send to non-foreman agents |
| `/subscribe` | Request authorization and receive a one-time binding key (not required for the Weixin QR-login account) |
| `/unsubscribe` | Stop receiving messages |
| `/status` | Show group status |
| `/pause` | Pause delivery for this chat or thread |
| `/resume` | Resume delivery for this chat or thread |
| `/verbose [on\|off]` | Enable verbose delivery, or disable it with `off` |
| `/help` | Show help |

::: tip Implicit Send
On platforms that support group chats, @mentioning the bot, or sending a direct message with plain text, is automatically treated as `/send` to the **foreman**. You only need the explicit `/send` command when targeting specific agents.

A recognized CCCC slash command is itself an explicit bot address and may be sent without an @mention on group providers that deliver it. Ordinary group text and attachments still require an explicit @mention. The native worker applies this rule consistently, including for Feishu, to avoid a provider-specific command policy.
:::

The legacy `/context`, `/launch`, and `/quit` commands are retired; use Web or
the CCCC CLI for lifecycle control. The low-level
`skip_pending_on_start=false` backlog-replay policy is also outside the product
contract. A legacy value is normalized away and the native worker starts at the
current ledger boundary, so an old setting cannot prevent IM startup after
upgrading.

Weixin currently supports direct bot chats only. Confirming the QR login immediately authorizes the scanning account; it can send plain text as soon as the bridge is running, with no `/subscribe`, binding key, or manual approval step. Worker startup and login-status recovery repair this authorization automatically. The Rust SDK callback does not expose a stable group-chat ID, so Weixin group messages are intentionally outside the supported bridge contract.

If Weixin asks for a pairing code after the QR scan, enter the code shown on the phone in **Settings → IM Bridge**. The page keeps polling through the scanned and verification states, including regional endpoint redirects, until login succeeds or the QR session ends.

On platforms using explicit chat authorization, plain text is not forwarded before approval. Direct chats reply with the readable target group name and ask you to run `/subscribe`; approve the generated key in **Settings → IM Bridge → Pending Requests**. Telegram groups with Group Privacy disabled stay silent for unrelated, unauthorized messages and only return this feedback for commands or an explicit @mention, preventing ambient group traffic from triggering bot replies. After approval, direct chats accept plain text without `/send`; use `/send` only when selecting an explicit recipient. The target name is included deliberately so a stale bot process or reused credential cannot look like a successful subscription to the intended group. Internal group IDs remain limited to the CLI binding command where they are operationally required.

Telegram topics, Slack threads, Feishu reply threads, and Discord thread channels retain their native target when a subscription is approved. `system.notify` is fail-closed: it never leaves the group unless its producer explicitly sets `im_visibility: "public"`, and actor-targeted notifications remain internal regardless of that flag.

Progressive output is an optimization, not the durability boundary. If a
provider cannot start or finalize an editable message, CCCC sends the completed
response normally. If the completed response exceeds the provider's
single-message limit, the final fallback is split on Unicode-safe boundaries
and every chunk is delivered; a truncated progressive preview never suppresses
that fallback. Intermediate snapshots are batched to stay within provider edit
limits while the final frame is always sent.

::: warning One active worker per bot credential
Do not leave a 0.4.35 worker running during an upgrade, or assign the same bot
credential to multiple CCCC groups. Providers may distribute callbacks between
those workers, so a command can be answered by one group while the next
plain-text message reaches another. If the reply names the wrong group, stop the
stale worker or correct the credential assignment before approving access.
:::

Reserve `/send @all <message>` for true broadcasts, announcements, or urgent shared constraints. Use plain text, `/send @foreman <message>`, or a specific actor target for routine coordination.

## CLI Commands

```bash
# Configure (platform-specific, see each guide)
cccc im set <platform> --token-env <ENV_VAR>

# Control
cccc im start        # Start IM bridge
cccc im stop         # Stop IM bridge
cccc im status       # Check bridge status
cccc im logs         # View logs
cccc im logs -f      # Follow logs
```

::: tip WeCom Note
WeCom currently uses the same start/stop/status CLI controls, but credentials are configured through the Web UI rather than `cccc im set`.
:::

## Quick Start

1. Choose a platform from the list above
2. Follow the setup guide to create a bot
3. Configure CCCC with the bot credentials
4. Start the bridge and authorize the chat: confirm the QR login for Weixin, or run `/subscribe` and approve the binding request on other platforms

## Next Steps

- [Telegram Setup](./telegram) - Quick personal setup
- [Slack Setup](./slack) - Team collaboration
- [Discord Setup](./discord) - Community access
- [Feishu/Lark Setup](./feishu) - Enterprise (China/Global)
- [DingTalk Setup](./dingtalk) - Enterprise (China)
- [WeCom Setup](./wecom) - Enterprise (China)
- Weixin / WeChat setup is configured from the Web IM Bridge settings surface.
