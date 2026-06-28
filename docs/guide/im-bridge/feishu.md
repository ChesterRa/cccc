# Feishu (Lark) Setup

Connect your CCCC working group to Feishu for enterprise collaboration in China.

## Overview

Feishu (飞书, also known as Lark internationally) is ideal for:

- Chinese enterprises
- Teams already using Feishu
- Bytedance ecosystem users

## Prerequisites

- Feishu enterprise account with admin access
- CCCC installed and running

## Step 1: Create an App

1. Go to [Feishu Open Platform](https://open.feishu.cn/app)
2. Click **Create Custom App**
3. Fill in the app information:
   - App Name (e.g., "CCCC Bot")
   - App Description
   - App Icon
4. Click **Create**

## Step 2: Configure Permissions

1. Go to **Permissions & Scopes**
2. Click **Add permission scopes to app**
3. Add the following scopes exactly as shown below
4. Click **Confirm and Apply**

Copyable scope configuration:

```json
{
  "scopes": {
    "tenant": [
      "im:message.group_at_msg:readonly",
      "im:message.p2p_msg:readonly",
      "im:message:send_as_bot",
      "im:message:readonly",
      "im:resource"
    ]
  }
}
```
::: tip Required Permissions
| Scope | Purpose |
|-------|---------|
| `im:message.group_at_msg:readonly` | Receive group messages that mention the bot |
| `im:message.p2p_msg:readonly` | Receive direct messages sent to the bot |
| `im:message:send_as_bot` | Send CCCC replies as the bot |
| `im:message:readonly` | Read inbound message metadata/content needed by the bridge |
| `im:resource` | Upload and download Feishu message images/files |
:::

`im:resource` is the only Feishu resource/file scope CCCC needs. Do not add
`corehr:file:download` or other HR/document scopes for IM attachments.

## Step 3: Configure CCCC

1. In your app dashboard, go to **Credentials & Basic Info**
2. Copy **App ID** and **App Secret**

::: warning Security
Keep your App Secret confidential. Never commit it to version control.
:::

### Option A: Via Web UI

1. Open the CCCC Web UI at `http://127.0.0.1:8848/`
2. Go to **Settings** (gear icon in header)
3. Navigate to the **IM Bridge** tab
4. Select **Feishu/Lark** as the platform
5. Enter your credentials:
   - **App ID**: Your Feishu App ID (e.g., `cli_a9e92055a5b89bc6`)
   - **App Secret**: Your Feishu App Secret
6. Click **Save Config**

![CCCC IM Bridge Configuration](/images/cccc-im-bridge-feishu.png)

### Option B: Via CLI

First set environment variables:

```bash
export FEISHU_APP_ID="cli_your_app_id"
export FEISHU_APP_SECRET="your_app_secret"
```

Then configure CCCC:

```bash
cccc im set feishu \
  --app-key-env FEISHU_APP_ID \
  --app-secret-env FEISHU_APP_SECRET
```

Both methods save to `group.yaml`:

```yaml
im:
  platform: feishu
  feishu_app_id_env: FEISHU_APP_ID
  feishu_app_secret_env: FEISHU_APP_SECRET
```

## Step 4: Start the Bridge

### Via Web UI

Click the **Save Config** button in the IM Bridge settings. The bridge will start automatically and show **Running** status.

### Via CLI

```bash
cccc im start
```

Verify it's running:

```bash
cccc im status
```

## Step 5: Enable Persistent Connection (Recommended)

::: warning Prerequisite
The CCCC IM Bridge must be running before you can configure event subscriptions. Enable persistent connection so CCCC can receive events via a long connection (no public callback URL required for this mode).
:::

1. Go back to [Feishu Open Platform](https://open.feishu.cn/app)
2. Navigate to your app → **Events & Callbacks**
3. In **Event Configuration** tab, find **Subscription mode**
4. Select **Receive events through persistent connection** (Recommended)
5. Click **Save**

![Feishu Event Configuration - Long Polling](/images/feishu-event-config.png)

## Step 6: Configure Event Subscriptions

1. In **Event Subscriptions**, click **Add Events**
2. Subscribe to the following events:

| Event | Purpose |
|-------|---------|
| `im.message.receive_v1` | Receive messages |
| `im.message.message_read_v1` | Read receipts |

3. Click **Save**

## Step 7: Enable Bot

::: tip Why This Step?
You must enable the Bot capability so users can find and chat with your bot after the app is published.
:::

1. In the sidebar, go to **Features** → **Bot**
2. In **Bot Setting**, fill in the **Get started** field with a greeting message (e.g., "cccc im")
3. Click **Save**

![Feishu Bot Setting](/images/feishu-bot-setting.png)

## Step 8: Publish the App

1. Go to **Version Management & Release**
2. Create a new version
3. Submit for review (enterprise apps may auto-approve)
4. Once approved, publish to your organization

## Step 9: Subscribe in Feishu

1. Find the bot in your Feishu app
2. Start a chat or add to a group
3. Send `/subscribe`
4. Confirm the subscription

## Usage

### Sending Messages to Agents

Feishu supports two ways to send messages:

**In group chats** — @mention the bot and type your message directly:

```
@YourBotName 请实现登录功能
```

Or use the explicit `/send` command for specifying recipients:

```
@YourBotName /send @all Status update please
```

**In direct messages** — just type your message:

```
请实现登录功能
```

::: tip Implicit Send
When you @mention the bot (in groups) or send a direct message, plain text is automatically treated as `/send` to the foreman. You only need the explicit `/send` command when targeting specific agents like `@all` or `@peers`.
:::

### Targeting Specific Agents

Use `@mention` syntax with the `/send` command:

```
/send @backend Check the API endpoints
/send @all Status update please
/send @peers Please review the PR
```

### Receiving Messages

After subscribing, you will automatically receive:
- Agent responses
- Status updates
- Error notifications

Use `/verbose` to toggle whether you see agent-to-agent messages.

### File Sharing

Attach files to your message. Feishu files are downloaded and stored in CCCC's blob storage, then forwarded to agents.

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

## Troubleshooting

### "Invalid app_id" Error

1. Verify your App ID in the Feishu Open Platform
2. Check environment variable is set correctly
3. Ensure the app is published and approved

### "Permission denied" Error

1. Go to **Permissions & Scopes**
2. Add the missing permission
3. Submit a new version for approval

If outbound attachments appear as text-only messages or the bridge log contains
`Access denied` for `/im/v1/files`, add `im:resource`. Feishu requires this
tenant scope before CCCC can upload files and send them as real Feishu file messages.

### Bot not receiving messages

1. Check Event Subscriptions are configured
2. Verify the app is installed in the chat
3. Ensure the app version is published
4. Make sure CCCC IM Bridge is running

### Token expired

CCCC auto-refreshes tokens, but if issues persist:

```bash
cccc im stop
cccc im start
```

## Advanced Configuration

CCCC currently supports:

- Outbound messages via REST APIs
- Inbound messages via persistent connection (Python `lark-oapi`)

Webhook callbacks (developer server URL), message cards, and encryption settings are not configured through CCCC at the moment.

## Security Notes

- Store credentials in environment variables or a secrets manager
- Use the minimal required permissions
- Review app access regularly
- Enable platform encryption for sensitive communications (optional)
- Audit app usage through Feishu admin console
