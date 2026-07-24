# IM Bridge Overview

Bridge your CCCC working group to popular IM platforms for mobile access.

## What is IM Bridge?

The IM Bridge allows you to:

- Send messages to agents from your phone
- Receive updates and notifications
- Control the group with slash commands
- Share files and attachments

## Supported Platforms

| Platform | Status | Best For |
|----------|--------|----------|
| [Telegram](./telegram) | ✅ | Personal use, quick setup |
| [Slack](./slack) | ✅ | Team collaboration |
| [Discord](./discord) | ✅ | Community/gaming |
| [Feishu/Lark](./feishu) | ✅ | Enterprise (China/Global) |
| [DingTalk](./dingtalk) | ✅ | Enterprise (China) — AI Card Streaming supported |
| [WeCom](./wecom) | ✅ | Enterprise (China) — Long connection Bot ID / Secret flow |
| Weixin / WeChat | ✅ | Personal and group chat access via the Python Weixin adapter |

## Design Principles

- **1 Group = 1 Bot**: Each working group connects to one bot instance for simplicity and isolation
- **Explicit subscription**: Users must `/subscribe` before receiving messages
- **Thin ports**: IM bridges forward messages and commands; the daemon remains the single source of truth

## Common Commands

Once subscribed to any platform, these commands work universally:

| Command | Description |
|---------|-------------|
| `/send <message>` | Send to foreman (default) |
| `/send @<actor> <message>` | Send to specific actor |
| `/send @all <message>` | Send to all agents |
| `/send @peers <message>` | Send to non-foreman agents |
| `/subscribe` | Start receiving messages |
| `/unsubscribe` | Stop receiving messages |
| `/status` | Show group status |
| `/pause` | Pause message delivery |
| `/resume` | Resume message delivery |
| `/verbose` | Toggle verbose mode |
| `/help` | Show help |

::: tip Implicit Send
On all platforms, @mentioning the bot (in groups) or sending a direct message with plain text is automatically treated as `/send` to the **foreman**. You only need the explicit `/send` command when targeting specific agents.
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
4. Start the bridge and subscribe in your chat

## Next Steps

- [Telegram Setup](./telegram) - Quick personal setup
- [Slack Setup](./slack) - Team collaboration
- [Discord Setup](./discord) - Community access
- [Feishu/Lark Setup](./feishu) - Enterprise (China/Global)
- [DingTalk Setup](./dingtalk) - Enterprise (China)
- [WeCom Setup](./wecom) - Enterprise (China)
- Weixin / WeChat setup is configured from the Web IM Bridge settings surface.
