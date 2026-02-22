# Telegram Setup

Connect your CCCC working group to Telegram for mobile access.

## Overview

Telegram is the easiest platform to set up. It's ideal for:

- Personal use
- Quick prototyping
- Individual developers

## Prerequisites

- A Telegram account
- CCCC installed and running

## Step 1: Create a Bot

1. Open Telegram and search for `@BotFather`
2. Start a chat and send `/newbot`
3. Follow the prompts:
   - Choose a display name (e.g., "My CCCC Bot")
   - Choose a username (must end in `bot`, e.g., `my_cccc_bot`)
4. BotFather will give you a token like:
   ```
   123456789:ABCdefGHIjklMNOpqrsTUVwxyz
   ```
5. **Save this token** — you'll need it in the next step

::: tip Recommended: Disable Group Privacy
If you plan to use the bot in group chats, disable group privacy so the bot can see all messages:

1. Send `/mybots` to BotFather
2. Select your bot → **Bot Settings** → **Group Privacy**
3. Set to **Disabled**
:::

## Step 2: Configure CCCC

### Option A: Via Web UI (Recommended)

1. Open the CCCC Web UI at `http://127.0.0.1:8848/`
2. Go to **Settings** (gear icon in header)
3. Navigate to the **IM Bridge** tab
4. Select **Telegram** as the platform
5. Enter your bot token:
   - Paste the token directly (e.g., `123456789:ABCdefGHIjklMNOpqrsTUVwxyz`)
   - Or enter an environment variable name (e.g., `TELEGRAM_BOT_TOKEN`)
6. Click **Save Config**

![CCCC IM Bridge Configuration](/images/cccc-im-bridge-telegram.png)

::: tip Security Best Practice
For production use, store the token in an environment variable instead of pasting it directly:

```bash
# Add to your shell profile (~/.bashrc, ~/.zshrc, etc.)
export TELEGRAM_BOT_TOKEN="your-token-here"
```

Then enter `TELEGRAM_BOT_TOKEN` in the Web UI. Never commit tokens to git.
:::

### Option B: Via CLI

```bash
# Using environment variable name
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN

# Verify configuration
cccc im config
```

Both methods save the configuration to your group's `group.yaml`:

```yaml
im:
  platform: telegram
  token_env: TELEGRAM_BOT_TOKEN
```

## Step 3: Start Bridge & Subscribe

### Start the Bridge

**Via Web UI**: Click **Save Config** — the bridge starts automatically and shows **Running** status.

**Via CLI**:

```bash
cccc im start
```

Verify it's running:

```bash
cccc im status
```

### Authorize in Telegram

New chats must be authorized before they can use the bot:

1. Open Telegram and find your bot (search by username)
2. Start a chat with the bot
3. Send `/subscribe` — the bot replies with a one-time binding key:
   ```
   abc123xyz
   ```
4. **Authorize the chat** (choose one):
   - **Web (recommended):** Open **CCCC Web → Settings → IM Bridge**, find it in **Pending Requests**, and click **Approve** (or paste the key in **Bind**)
   - **Foreman-assisted:** If foreman is online, send the key to foreman and ask foreman to bind it
   - **CLI:** Run `cccc im bind --key <key>` on the server
5. Once authorized, the chat can immediately send and receive messages — no further commands needed

For group chats:
1. Add the bot to your group
2. Follow the same authorization flow above
3. All authorized chats receive messages from CCCC

## Usage

### Sending Messages to Agents

Telegram supports two ways to send messages:

**In group chats** — @mention the bot and type your message directly:

```
@YourBotName Please implement the login feature
```

Or use the explicit `/send` command:

```
@YourBotName /send @all Status update please
```

**In direct messages** — just type your message:

```
Please implement the login feature
```

::: tip Implicit Send
When you @mention the bot (in groups) or send a direct message, plain text is automatically treated as `/send` to the foreman. You only need the explicit `/send` command when targeting specific agents like `@all` or `@peers`.
:::

### Targeting Specific Agents

Use `@mention` syntax with the `/send` command:

```
/send @foreman Please review the PR
/send @peer-1 Run the tests
/send @all Status update please
```

### Receiving Messages

After authorization, you will automatically receive:
- Agent responses
- Status updates
- Error notifications

Use `/verbose` to toggle whether you see agent-to-agent messages.

### File Attachments

Attach files to your message. They're downloaded and stored in CCCC's blob storage, then forwarded to agents.

## Commands Reference

| Command | Description |
|---------|-------------|
| `/subscribe` | Start authorization flow — generates a one-time binding key |
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

## Troubleshooting

### Bot not responding

1. Check if the bridge is running:
   ```bash
   cccc im status
   ```

2. Check logs for errors:
   ```bash
   cccc im logs -f
   ```

3. Verify token is correct — re-check with BotFather (`/mybots` → select bot → **API Token**)

### "Unauthorized" error

Your token is invalid. Get a new one from BotFather:

1. Send `/mybots` to BotFather
2. Select your bot
3. Click **API Token** → **Revoke current token**
4. Update your token in CCCC Settings (Web UI) or environment variable

### Messages not delivered

1. Ensure the chat is authorized (run `/subscribe` → bind the key via Web, foreman-assisted bind, or CLI)
2. Check that the CCCC daemon is running
3. Verify the bridge status in Web UI or via `cccc im status`

### Rate limiting

Telegram has rate limits. If you're sending many messages:
- Messages may be delayed
- Consider using `/verbose` to disable verbose mode and reduce traffic

## Security Notes

- Keep your bot token secret
- Consider enabling 2FA on your Telegram account
- Review who has access to chats where the bot is subscribed
- The bot can see all messages in groups where it's added
- **New chats require key-based authorization** before they can interact with the bot — send `/subscribe` to generate a one-time key, then confirm it from the server side (Web Settings > IM Bridge, foreman-assisted bind, or CLI); once bound, the chat is fully authorized
- Binding keys expire after **10 minutes**; generate a new one with `/subscribe` if it lapses
- The bind operation must be performed via CCCC Web Settings (IM Bridge), foreman-assisted bind, or `cccc im bind --key` on the server — Telegram users cannot self-authorize
