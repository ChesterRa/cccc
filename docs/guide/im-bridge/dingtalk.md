# DingTalk Setup

Connect your CCCC working group to DingTalk for enterprise collaboration.

## Overview

DingTalk (钉钉) is ideal for:

- Chinese enterprises
- Alibaba ecosystem users
- Teams already using DingTalk

CCCC uses DingTalk Stream mode (persistent WebSocket connection) for inbound messages and DingTalk Open APIs for outbound messages. No public URL is required.

## Prerequisites

- DingTalk enterprise account with admin access
- CCCC installed and running

## Step 1: Create an Application

1. Go to [DingTalk Open Platform](https://open.dingtalk.com/)
2. Log in with your enterprise admin account
3. Click **Application Development** → **Internal Development**
4. Click **Create Application**
5. Fill in:
   - Application Name (e.g., "CCCC Bot")
   - Application Description
   - Application Icon
6. Click **Confirm**

## Step 2: Configure Permissions

1. Go to **Permissions**
2. Apply for the following permissions:

| Permission | Purpose |
|------------|---------|
| `Robot.SingleChat.ReadWrite` | Single chat robot management |
| `qyapi_robot_sendmsg` | Robot proactive message sending |
| `qyapi_chat_read` | Read group basic info |
| `qyapi_chat_manage` | Manage group chats (create, update, send messages) |
| `Card.Streaming.Write` | AI Card streaming writes |
| `Card.Instance.Write` | Card instance writes |

3. Click to enable each permission (no approval needed for internal apps)

## Step 3: Enable Robot

1. In **Application Capabilities** → **Robot**
2. Enable the robot capability
3. Configure robot settings:
   - Robot name
   - Robot avatar

## Step 4: Publish the Application

1. Go to **Version Management**
2. Create a new version
3. Configure visibility:
   - All employees
   - Specific departments
   - Specific users
4. Publish the version

## Step 5: Configure & Start CCCC

1. In your application, go to **Credentials & Basic Info**
2. Copy **AppKey** and **AppSecret**
3. (Optional) Copy **RobotCode** if shown in your Robot settings (CCCC can sometimes learn it after the first inbound message, but configuring it upfront is more reliable for attachments)

### Option A: Via Web UI

1. Open the CCCC Web UI at `http://127.0.0.1:8848/`
2. Go to **Settings** (gear icon in header)
3. Navigate to the **IM Bridge** tab
4. Select **DingTalk** as the platform
5. Enter your credentials:
   - **App Key**: Your DingTalk AppKey
   - **App Secret**: Your DingTalk AppSecret
6. Click **Save Config** — the bridge will start automatically and show **Running** status

### Option B: Via CLI

First set environment variables:

```bash
export DINGTALK_APP_KEY="your_app_key"
export DINGTALK_APP_SECRET="your_app_secret"
export DINGTALK_ROBOT_CODE="your_robot_code"  # optional but recommended
```

Then configure and start the bridge:

```bash
cccc im set dingtalk \
  --app-key-env DINGTALK_APP_KEY \
  --app-secret-env DINGTALK_APP_SECRET \
  --robot-code-env DINGTALK_ROBOT_CODE

cccc im start
```

Verify it's running:

```bash
cccc im status
```

Both methods save to `group.yaml`:

```yaml
im:
  platform: dingtalk
  dingtalk_app_key_env: DINGTALK_APP_KEY
  dingtalk_app_secret_env: DINGTALK_APP_SECRET
  dingtalk_robot_code_env: DINGTALK_ROBOT_CODE
```

## Step 6: Subscribe in DingTalk

1. Find the robot in your DingTalk application
2. Add it to a group chat or start a direct conversation
3. Send `/subscribe`
4. Confirm the subscription

## Usage

### Sending Messages to Agents

DingTalk supports two ways to send messages:

**Direct message (implicit send)** — just type your message:

```
请检查一下代码质量
```

**Explicit `/send` command** — for specifying recipients:

```
/send @foreman Please check the code quality
/send @all Status update please
```

::: tip Implicit Send
DingTalk messages are always directed at the bot (via @mention in groups or direct chat), so plain text is automatically treated as `/send` to the foreman. You only need the explicit `/send` command when targeting specific agents.
:::

### Targeting Specific Agents

Use `@mention` syntax with the `/send` command:

```
/send @foreman Please assign today's development tasks
/send @reviewer Please review the latest commits
/send @all Status update please
```

### Receiving Messages

After subscribing, you will automatically receive:
- Agent responses
- Status updates
- Error notifications

Use `/verbose` to toggle whether you see agent-to-agent messages.

### Message Types

DingTalk supports various message types:

- **Text**: Plain text messages
- **Markdown**: Formatted text
- **Link**: URL cards
- **ActionCard**: Interactive cards with buttons
- **AI Card Streaming**: Agent responses are delivered as AI Cards with a real-time typewriter effect. Content streams in progressively as it is generated. If streaming fails (e.g., card API unavailable), the message automatically falls back to plain text delivery.

CCCC automatically selects the appropriate format.

### File Sharing

Attach files to your message. DingTalk files are downloaded and stored in CCCC's blob storage, then forwarded to agents.

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

### "Invalid appkey" Error

1. Verify AppKey in DingTalk Open Platform
2. Check environment variable is set correctly
3. Ensure the application is published

### "No permission" Error

1. Check required permissions are granted
2. Verify the app is visible to the user
3. Ensure the app version is published

### Robot not responding

1. Check if the robot is added to the chat
2. Verify the bridge is running:
   ```bash
   cccc im status
   ```
3. Check logs:
   ```bash
   cccc im logs -f
   ```

### Connection drops

If the connection drops unexpectedly:

1. Check network connectivity
2. Restart the bridge:
   ```bash
   cccc im stop
   cccc im start
   ```

## Security Notes

- Keep your AppSecret confidential and rotate it periodically
- Use the minimal required permissions
- Review robot/app access regularly
- Audit message logs regularly
- Limit robot visibility to necessary employees
