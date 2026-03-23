# Workflow Examples

Common patterns for using CCCC to coordinate AI agents.

## Terminology Alignment

This document follows the local glossary:

- `attach` sets the group's `authoritative_workspace`
- `shared` is the default lightweight execution interpretation unless an
  explicit isolated policy exists
- `group` is the main collaboration unit
- `profile` is reusable actor runtime configuration, not the live actor itself
- `status` updates should be read as operator-facing evidence, not as proof of
  every deeper runtime capability

## Solo Development with One Agent

The simplest setup: one agent assisting you with a project.

### Setup

```bash
cd /your/project
cccc attach .
cccc actor profile upsert --id assistant-shared --name "Assistant Shared" --runtime claude
cccc actor add assistant --profile-id assistant-shared
cccc
```

In current glossary wording, `cccc attach .` anchors the group to that project
as its `authoritative_workspace`.

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
cccc actor profile upsert --id impl-shared --name "Implementer Shared" --runtime claude
cccc actor profile upsert --id review-shared --name "Reviewer Shared" --runtime codex
cccc actor add implementer --profile-id impl-shared
cccc actor add reviewer --profile-id review-shared
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
cccc actor profile upsert --id architect-shared --name "Architect Shared" --runtime claude
cccc actor profile upsert --id frontend-shared --name "Frontend Shared" --runtime codex
cccc actor profile upsert --id backend-shared --name "Backend Shared" --runtime droid
cccc actor profile upsert --id tester-shared --name "Tester Shared" --runtime kimi
cccc actor add architect --profile-id architect-shared    # Design decisions
cccc actor add frontend --profile-id frontend-shared      # UI implementation
cccc actor add backend --profile-id backend-shared        # API implementation
cccc actor add tester --profile-id tester-shared          # Testing
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

## Related Glossary

- [group](/reference/glossary/group)
- [attach](/reference/glossary/attach)
- [authoritative_workspace](/reference/glossary/authoritative_workspace)
- [profile](/reference/glossary/profile)
- [shared](/reference/glossary/shared)
- [status](/reference/glossary/status)

## Change Log

- `2026-03-21`: Added local glossary alignment so workflow examples consistently describe group attachment, default shared execution, and status language.
- `2026-03-23`: Added concrete profile-backed actor setup examples so workflow docs reflect the new CLI profile linkage surface.
- `2026-03-23`: Added `profile` alignment so workflow examples keep reusable runtime configuration separate from live actor participation.
