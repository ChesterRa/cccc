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
3. Send quick requests via chat, and use task-backed delegation when the work needs an owner, outcome, or evidence trail
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

1. Send implementation tasks to `@implementer`, or use task-backed delegation when the work needs completion evidence
2. When complete, ask `@reviewer` to review the changes
3. Iterate based on review feedback

### Tips

- The reviewer can catch bugs and suggest improvements
- Use different runtimes for diverse perspectives
- Keep tasks focused and specific

## Pull Request Review with Codex

Use this pattern when you want Codex to act as a reviewer while another agent or human prepares the change.

### Setup

```bash
cccc actor add implementer --runtime claude
cccc actor add codex-reviewer --runtime codex
cccc group start
```

### Workflow

1. Send the implementation task to `@implementer` with explicit acceptance criteria
2. Ask `@codex-reviewer` to inspect the diff before you open or merge the pull request
3. Include the target branch, changed files, test command, and risk areas in the review request
4. Require the reviewer to separate blocking findings from suggestions
5. Apply fixes, rerun validation, and ask for one final review pass

### Example

```bash
cccc tracked-send "Implement the smallest safe fix for the failing login test. Reply with changed files and validation evidence." \
  --to implementer \
  --title "Fix login regression" \
  --outcome "The login regression is fixed, tests pass, and risks are documented"

cccc tracked-send "Review the current git diff for correctness, missing tests, and release risk. Report blocking findings first, then suggestions." \
  --to codex-reviewer \
  --title "Review login regression fix" \
  --outcome "Blocking findings and non-blocking suggestions are reported with file references"
```

### Tips

- Keep the review request tied to the current diff or branch
- Ask for evidence: failing test, passing test, or exact command output
- Route follow-up fixes back to the implementer instead of letting review feedback sprawl
- Use `reply_required` or task-backed delegation for release-critical reviews

## Multi-Agent Team

For complex projects, use multiple specialized agents.

### Setup Example

```bash
cccc actor add architect --runtime claude    # Design decisions
cccc actor add frontend --runtime codex      # UI implementation
cccc actor add backend --runtime droid       # API implementation
cccc actor add tester --runtime kimi         # Testing
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
cccc tracked-send "Please refactor the authentication module and report progress every hour." \
  --to @foreman \
  --title "Refactor authentication module" \
  --outcome "Refactor is complete, risks are reported, and validation evidence is provided"
```

### Monitoring

- IM Bridge sends updates to your phone
- Check progress via Web UI when convenient
- Agents notify on completion or errors
