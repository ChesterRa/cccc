# Discord Setup

Connect your CCCC working group to Discord for community access.

## Overview

Discord integration is great for:

- Developer communities
- Open source projects
- Gaming and hobby groups
- Public collaboration

## Prerequisites

- A Discord server where you have admin rights
- CCCC installed and running

## Step 1: Create a Discord Application

1. Go to [Discord Developer Portal](https://discord.com/developers/applications)
2. Click **New Application**
3. Enter a name (e.g., "CCCC Bot")
4. Accept the terms and click **Create**

## Step 2: Create a Bot

1. In your application, go to **Bot** in the sidebar
2. Click **Add Bot**
3. Confirm by clicking **Yes, do it!**

### Configure Bot Settings

Under the Bot section:

| Setting | Recommended |
|---------|-------------|
| Public Bot | OFF (unless you want others to add it) |
| Requires OAuth2 Code Grant | OFF |
| Presence Intent | OFF |
| Server Members Intent | OFF |
| Message Content Intent | **ON** (required!) |

::: warning Important
**Message Content Intent** must be enabled, or the bot cannot read messages.
:::

## Step 3: Get Bot Token

1. In the **Bot** section, click **Reset Token**
2. Confirm and copy the new token
3. **Save this token securely**

::: danger Security
Never share your bot token. If exposed, regenerate it immediately.
:::

## Step 4: Set Bot Permissions

1. Go to **OAuth2** → **URL Generator**
2. Under **Scopes**, select:
   - `bot`
   - `applications.commands` (optional, for slash commands)

3. Under **Bot Permissions**, select:

| Permission | Purpose |
|------------|---------|
| Read Messages/View Channels | See channels and messages |
| Send Messages | Reply to users |
| Send Messages in Threads | Reply in threads |
| Embed Links | Rich message formatting |
| Attach Files | Share files |
| Read Message History | Access conversation history |
| Add Reactions | React to messages |

4. Copy the generated URL at the bottom

## Step 5: Add Bot to Server

1. Open the URL from Step 4 in your browser
2. Select your server from the dropdown
3. Click **Continue**
4. Review permissions and click **Authorize**
5. Complete the CAPTCHA

## Step 6: Set Environment Variable

```bash
# Add to your shell profile
export DISCORD_BOT_TOKEN="your-bot-token-here"
```

## Step 7: Configure CCCC

### Option A: Via Web UI

1. Open the CCCC Web UI at `http://127.0.0.1:8848/`
2. Go to **Settings** (gear icon in header)
3. Navigate to the **IM Bridge** section
4. Select **Discord** as the platform
5. Enter your credentials:
   - **Token Environment Variable**: `DISCORD_BOT_TOKEN`
6. Click **Save**

### Option B: Via CLI

```bash
cccc im set discord --token-env DISCORD_BOT_TOKEN
```

Both methods save to `group.yaml`:

```yaml
im:
  platform: discord
  token_env: DISCORD_BOT_TOKEN
```

## Step 8: Start the Bridge

```bash
cccc im start
```

## Step 9: Subscribe in Discord

1. Go to a channel where the bot has access
2. Send `/subscribe`
3. You should receive a confirmation

## Usage

### Sending Messages to Agents

In channels, @mention the bot first, then use the `/send` command:

```
@YourBotName /send Please review the latest pull request
```

In direct messages with the bot, you can use `/send` directly:

```
/send Please review the latest pull request
```

::: warning Important
- In channels, you must @mention the bot before the message is routed
- After mentioning the bot, plain text is treated as implicit send to foreman
- Use `/send` when you need explicit recipients like `@all` or `@peers`
:::

### Targeting Specific Agents

Use `@mention` syntax with the `/send` command:

```
/send @foreman Coordinate the release
/send @tester Run the integration tests
/send @all Status update please
```

### Receiving Messages

After subscribing, you will automatically receive:
- Agent responses
- Status updates
- Error notifications

Use `/verbose` to toggle whether you see agent-to-agent messages.

### Thread Support

Create threads for focused discussions. CCCC tracks thread context.

### File Attachments

Attach files to your message. They're stored in CCCC's blob storage, then forwarded to agents.

### Embeds

CCCC formats responses with Discord embeds for better readability when appropriate.

## Commands Reference

| Command | Description |
|---------|-------------|
| `/subscribe` | Start receiving messages from CCCC |
| `/unsubscribe` | Stop receiving messages |
| `/send <message>` | Send to foreman (default) |
| `/send @<actor> <message>` | Send to a specific agent |
| `/send @all <message>` | Send to all agents |
| `/send @peers <message>` | Send to non-foreman agents |
| `/status` | Show group and agent status |
| `/pause` | Pause message delivery |
| `/resume` | Resume message delivery |
| `/verbose` | Toggle verbose mode (see all agent messages) |
| `/help` | Show available commands |

## Slash Commands (Optional)

Register application commands for better UX:

1. Go to your app in Developer Portal
2. Navigate to **Bot** → **Interactions Endpoint URL**
3. Or use the Discord API to register commands

Example slash command structure:
```
/cccc send <message>
/cccc status
/cccc agents
```

## Troubleshooting

### "Missing Access" Error

The bot lacks permissions:

1. Check the bot's role in Server Settings
2. Ensure the role has necessary permissions
3. Verify channel-specific permissions

### "Missing Intent" Error

Enable Message Content Intent:

1. Go to Developer Portal → Your App → Bot
2. Enable **Message Content Intent**
3. Save changes
4. Restart the bridge

### Bot is offline

1. Check the bridge is running:
   ```bash
   cccc im status
   ```

2. Verify the token:
   ```bash
   cccc im logs -f
   ```

3. Regenerate token if needed

### Rate limiting

Discord has strict rate limits:
- Reduce message frequency
- Use `/verbose off` to minimize traffic
- Consider batching updates

## Advanced Configuration

### Specific Channels

Limit the bot to certain channels:

```yaml
im:
  platform: discord
  token_env: DISCORD_BOT_TOKEN
  allowed_channels:
    - 123456789012345678  # Channel ID
```

Get channel ID: Enable Developer Mode in Discord settings, right-click channel → Copy ID.

### Activity Status

Set the bot's status message:

```yaml
im:
  platform: discord
  token_env: DISCORD_BOT_TOKEN
  activity:
    type: watching  # playing, streaming, listening, watching
    name: "for commands"
```

### Multiple Servers

One bot can serve multiple servers. Each server needs to:
1. Add the bot via OAuth URL
2. Subscribe in desired channels

## Security Notes

- Keep the bot token secret
- Limit bot permissions to what's needed
- Use channel permissions to restrict access
- Review server roles regularly
- Consider verification levels for your server
- Be cautious with public bots - they can be added by anyone
