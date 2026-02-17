# Workflow Examples

Common patterns for using CCCC to coordinate AI agents.

## Solo Development with One Agent

The simplest setup: one agent assisting you with a project.

### Setup

```bash
cd /your/project
cccc attach .
cccc actor add assistant --runtime claude
cccc
```

### Workflow

1. Open the Web UI at http://127.0.0.1:8848/
2. Start the agent
3. Send tasks via chat: "Implement the login feature"
4. Watch the agent work in the terminal tab
5. Review changes and provide feedback

## Pair Programming with Two Agents

Use one agent for implementation and another for review.

### Setup

```bash
cccc actor add implementer --runtime claude
cccc actor add reviewer --runtime codex
cccc group start
```

### Workflow

1. Send implementation tasks to `@implementer`
2. When complete, ask `@reviewer` to review the changes
3. Iterate based on review feedback

### Tips

- The reviewer can catch bugs and suggest improvements
- Use different runtimes for diverse perspectives
- Keep tasks focused and specific

## Multi-Agent Team

For complex projects, use multiple specialized agents.

### Setup Example

```bash
cccc actor add architect --runtime claude    # Design decisions
cccc actor add frontend --runtime codex      # UI implementation
cccc actor add backend --runtime droid       # API implementation
cccc actor add tester --runtime copilot      # Testing
```

### Coordination

- The first enabled actor (architect) becomes foreman
- Foreman coordinates work across peers
- Use @mentions to direct tasks to specific agents
- Use Context panel for shared understanding

### Best Practices

- Define clear responsibilities for each agent
- Use milestones to track progress
- Regular check-ins to ensure alignment

## Remote Monitoring via Phone

Monitor and control your agents from anywhere.

### Setup Options

**Option 1: Cloudflare Tunnel (Recommended)**

```bash
# Quick (temporary URL)
cloudflared tunnel --url http://127.0.0.1:8848

# Stable (custom domain)
cloudflared tunnel create cccc
cloudflared tunnel route dns cccc cccc.yourdomain.com
cloudflared tunnel run cccc
```

**Option 2: IM Bridge**

```bash
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start
```

Then use your Telegram app to:
- Send messages to agents
- Receive status updates
- Control the group with slash commands

### Workflow

1. Set up remote access
2. Leave agents running on your development machine
3. Monitor and send commands from your phone
4. Receive notifications on important events

## Overnight Tasks

Run long-running tasks unattended.

### Setup

1. Define clear success criteria
2. Set up IM Bridge for notifications
3. Configure automation timeouts

### Example

```bash
# Configure notifications
cccc im set telegram --token-env TELEGRAM_BOT_TOKEN
cccc im start

# Start the task
cccc send "Please refactor the entire authentication module. Report progress every hour." --to @foreman
```

### Monitoring

- IM Bridge sends updates to your phone
- Check progress via Web UI when convenient
- Agents notify on completion or errors
