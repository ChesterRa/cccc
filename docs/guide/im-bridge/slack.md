# Slack Setup

Connect your CCCC working group to Slack for team collaboration.

## Overview

Slack integration uses Socket Mode for real-time messaging. It's ideal for:

- Team collaboration
- Enterprise environments
- Existing Slack workspaces

## Prerequisites

- A Slack workspace where you have admin rights
- CCCC installed and running

## Step 1: Create a Slack App

1. Go to [Slack API Apps](https://api.slack.com/apps)
2. Click **Create New App**
3. Choose **From scratch**
4. Enter app name (e.g., "CCCC Bot")
5. Select your workspace
6. Click **Create App**

## Step 2: Enable Socket Mode

Socket Mode allows the bot to receive events without exposing a public URL.

1. In your app settings, go to **Socket Mode**
2. Toggle **Enable Socket Mode** to ON
3. Click **Generate** to create an app-level token
4. Name it (e.g., "cccc-socket-token")
5. Add the scope `connections:write`
6. Click **Generate**
7. **Copy the token** (starts with `xapp-`)

::: tip
This is your App Token, used for the WebSocket connection.
:::

## Step 3: Configure Bot Permissions

1. Go to **OAuth & Permissions**
2. Under **Scopes** → **Bot Token Scopes**, add:

| Scope | Purpose |
|-------|---------|
| `chat:write` | Send messages |
| `channels:history` | Read public channel messages |
| `groups:history` | Read private channel messages |
| `im:history` | Read direct messages |
| `mpim:history` | Read group DMs |
| `files:read` | Read shared files |
| `files:write` | Upload files |
| `users:read` | Read user info |

## Step 4: Enable Event Subscriptions

1. Go to **Event Subscriptions**
2. Toggle **Enable Events** to ON
3. Under **Subscribe to bot events**, add:

| Event | Purpose |
|-------|---------|
| `message.channels` | Messages in public channels |
| `message.groups` | Messages in private channels |
| `message.im` | Direct messages |
| `message.mpim` | Group DMs |
| `app_mention` | When bot is @mentioned |

4. Click **Save Changes**

## Step 5: Install to Workspace

1. Go to **OAuth & Permissions**
2. Click **Install to Workspace**
3. Review permissions and click **Allow**
4. **Copy the Bot Token** (starts with `xoxb-`)

## Step 6: Set Environment Variables

```bash
# Add to your shell profile
export SLACK_BOT_TOKEN="xoxb-your-bot-token"
export SLACK_APP_TOKEN="xapp-your-app-token"
```

::: warning Two Tokens Required
Slack requires both tokens:
- **Bot Token** (`xoxb-`): For API calls
- **App Token** (`xapp-`): For Socket Mode connection
:::

## Step 7: Configure CCCC

### Option A: Via Web UI

1. Open the CCCC Web UI at `http://127.0.0.1:8848/`
2. Go to **Settings** (gear icon in header)
3. Navigate to the **IM Bridge** section
4. Select **Slack** as the platform
5. Enter your credentials:
   - **Bot Token Environment Variable**: `SLACK_BOT_TOKEN`
   - **App Token Environment Variable**: `SLACK_APP_TOKEN`
6. Click **Save**

### Option B: Via CLI

```bash
cccc im set slack \
  --bot-token-env SLACK_BOT_TOKEN \
  --app-token-env SLACK_APP_TOKEN
```

Both methods save to `group.yaml`:

```yaml
im:
  platform: slack
  bot_token_env: SLACK_BOT_TOKEN
  app_token_env: SLACK_APP_TOKEN
```

## Step 8: Start the Bridge

```bash
cccc im start
```

## Step 9: Subscribe in Slack

1. Invite the bot to a channel:
   ```
   /invite @your-bot-name
   ```
2. Send `/subscribe` in the channel
3. You should receive a confirmation

For direct messages:
1. Find the bot in your DMs
2. Send `/subscribe`

## Usage

### Sending Messages to Agents

In channels, @mention the bot first, then use the `/send` command:

```
@YourBotName /send Please implement the user authentication module
```

In direct messages with the bot, you can use `/send` directly:

```
/send Please implement the user authentication module
```

::: warning Important
- In channels, you must @mention the bot before the message is routed
- After mentioning the bot, plain text is treated as implicit send to foreman
- Use `/send` when you need explicit recipients like `@all` or `@peers`
:::

### Targeting Specific Agents

Use `@mention` syntax with the `/send` command (use CCCC's syntax, not Slack's):

```
/send @foreman Review the latest commits
/send @backend-agent Fix the API endpoint
/send @all Status update please
```

### Receiving Messages

After subscribing, you will automatically receive:
- Agent responses
- Status updates
- Error notifications

Use `/verbose` to toggle whether you see agent-to-agent messages.

### Thread Replies

Reply in threads to keep conversations organized. CCCC preserves thread context.

### File Sharing

Attach files to your message. They're uploaded to CCCC's blob storage, then forwarded to agents.

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
| `/pause` | Pause delivery |
| `/resume` | Resume delivery |
| `/verbose` | Toggle verbose mode |
| `/help` | Show help |

## Troubleshooting

### "invalid_auth" Error

Token is invalid or expired:

1. Go to **OAuth & Permissions**
2. Click **Reinstall to Workspace**
3. Update your `SLACK_BOT_TOKEN` environment variable

### "missing_scope" Error

Add the required scope:

1. Go to **OAuth & Permissions**
2. Add the missing scope under **Bot Token Scopes**
3. Reinstall the app

### Bot not receiving messages

1. Check Socket Mode is enabled
2. Verify `SLACK_APP_TOKEN` is correct
3. Ensure events are subscribed in **Event Subscriptions**
4. Check the bot is invited to the channel

### Connection drops

Socket Mode connections may drop occasionally. CCCC auto-reconnects, but if issues persist:

```bash
cccc im stop
cccc im start
```

## Advanced Configuration

### Channel Restrictions

Limit which channels the bot responds to:

```yaml
im:
  platform: slack
  bot_token_env: SLACK_BOT_TOKEN
  app_token_env: SLACK_APP_TOKEN
  allowed_channels:
    - C01234567  # Channel ID
    - C89012345
```

### Custom Bot Name

The display name is set in Slack:

1. Go to **App Home**
2. Under **Your App's Presence in Slack**
3. Edit **Display Name**

## Security Notes

- Bot tokens have broad access - limit to necessary workspaces
- Review channel membership regularly
- Consider using Enterprise Grid for additional controls
- Audit who can install apps in your workspace
