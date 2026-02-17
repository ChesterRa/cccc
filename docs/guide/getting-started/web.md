# Web UI Quick Start

Get started with CCCC using the Web interface.

## Step 1: Start CCCC

Open a terminal and run:

```bash
cccc
```

This starts both the daemon and the Web UI.

## Step 2: Open the Web UI

Open your browser and navigate to:

```
http://127.0.0.1:8848/
```

You should see the CCCC Web interface.

## Step 3: Create a Working Group

1. Click the **+** button in the sidebar
2. Or attach an existing project:

```bash
# In another terminal
cd /path/to/your/project
cccc attach .
```

3. Refresh the Web UI to see your new group

## Step 4: Add Your First Agent

1. Click **Add Actor** in the header
2. Fill in the form:
   - **Actor ID**: e.g., `assistant`
   - **Runtime**: Select your installed CLI (e.g., Claude)
   - **Runner**: PTY (terminal) or Headless
3. Click **Create**

## Step 5: Configure MCP (First Time Only)

If this is your first time using CCCC with this runtime:

```bash
cccc setup --runtime claude   # or codex, droid, etc.
```

This configures the agent to communicate with CCCC.

## Step 6: Start the Agent

1. Find your agent in the tabs
2. Click the **Play** button to start it
3. Wait for the agent to initialize

The agent's terminal output appears in the tab.

## Step 7: Send Your First Message

1. Click the **Chat** tab
2. Type a message in the input box:
   ```
   Hello! Please introduce yourself.
   ```
3. Press `Ctrl+Enter` / `Cmd+Enter`, or click Send

## Step 8: Watch the Agent Work

1. Switch to the agent's tab to see terminal output
2. Watch as the agent processes your request
3. Responses appear in the Chat tab

## Adding More Agents

To add a second agent for collaboration:

1. Click **Add Actor** again
2. Use a different ID (e.g., `reviewer`)
3. Optionally use a different runtime
4. Start the agent

Now you can:
- Send to all: Just type a message
- Send to specific agent: Use `@assistant` or `@reviewer`

## Using the Context Panel

Click **Context** to open the side panel:

- **Vision**: Set the project goal
- **Sketch**: Document the approach
- **Tasks**: Track work items
- **Notes**: Record learnings

Agents can read and update this shared context.

## Web UI Features

| Feature | How to Access |
|---------|---------------|
| Switch groups | Click group in sidebar |
| Agent terminal | Click agent tab |
| Send message | Chat tab input |
| @mention | Type `@` for autocomplete |
| Reply to message | Click reply icon |
| Settings | Gear icon in header |
| Theme | Click moon/sun icon |

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Ctrl+Enter` / `Cmd+Enter` | Send message |
| `Enter` | New line |
| `@` | Open mention menu |
| `Escape` | Cancel reply / Close menu |
| `↑` `↓` | Navigate mention menu |
| `Tab` / `Enter` | Select mention |

## Troubleshooting

### Web UI not loading?

1. Check daemon is running:
   ```bash
   cccc daemon status
   ```

2. Try a different port:
   ```bash
   CCCC_WEB_PORT=9000 cccc
   ```

### Agent won't start?

1. Check the terminal tab for errors
2. Verify MCP setup:
   ```bash
   cccc setup --runtime <name>
   ```

### Can't see my project?

Run `cccc attach .` in your project directory, then refresh the Web UI.

## Next Steps

- [Workflows](/guide/workflows) - Learn collaboration patterns
- [Web UI Guide](/guide/web-ui) - Detailed UI documentation
- [IM Bridge](/guide/im-bridge/) - Set up mobile access
