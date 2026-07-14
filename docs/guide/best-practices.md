# Best Practices

Tips for getting the most out of CCCC.

## Setting Up for Success

### Write a Good PROJECT.md

Place `PROJECT.md` at your project root. This is your project's "constitution":

```markdown
# Project Name

## Goal
One sentence describing what we're building.

## Constraints
- Must use TypeScript
- Follow existing code patterns
- No external dependencies without approval

## Architecture
Brief overview of the codebase structure.

## Current Focus
What we're working on right now.
```

Agents read this via `cccc_project_info` to understand context.

### Use Project Guidance

The Rust MCP server returns its packaged collaboration playbook through `cccc_help`. Put repository-specific rules in `AGENTS.md`, `CLAUDE.md`, or `PROJECT.md` inside the attached scope. `cccc_project_info` reads the project summary, while the agent runtime applies its native guidance files.

Keep guidance imperative, scoped, and testable. Avoid duplicating tool documentation or temporary task state in permanent project instructions.

### Choose the Right Agent Combination

| Scenario | Recommended Setup |
|----------|-------------------|
| Solo development | 1 Claude agent |
| Code + Review | Claude (impl) + Codex (review) |
| Full-stack project | Multiple specialized agents |
| Learning/exploration | 1 agent, interactive mode |

### Configure Runtimes Properly

Use recommended flags for autonomous operation:

```bash
# Claude Code
cccc actor add impl --runtime claude
# Uses: claude --dangerously-skip-permissions

# Codex
cccc actor add review --runtime codex
# Uses: codex --dangerously-bypass-approvals-and-sandbox --search
```

## Effective Communication

### Be Specific

❌ "Fix the bug"
✅ "Fix the login button not responding on mobile Safari"

❌ "Make it faster"
✅ "Optimize the getUserById query, it's taking 500ms"

### Use @Mentions Wisely

- `@all` for announcements, urgent shared constraints, or true team-wide questions
- `@foreman` for coordination decisions, planning, and routing work
- `@specific-agent` for targeted tasks
- Use task-backed delegation when a task needs a durable owner, done criterion, evidence, handoff, or acceptance trail

### Provide Context

Include relevant information:
- Error messages
- File paths
- Expected vs actual behavior
- Constraints or preferences

### Use Reply for Threads

Reply to keep conversations organized. Agents see the quoted context.

## Task Management

### Break Down Large Tasks

Instead of: "Implement user authentication"

Use milestones:
1. Database schema for users
2. Registration endpoint
3. Login endpoint
4. Session management
5. Password reset flow

### Set Clear Acceptance Criteria

For each task, define "done":
- Tests pass
- No lint errors
- Documentation updated
- Code reviewed

### Use the Context Panel

- **Vision**: Keep the project goal visible
- **Sketch**: Document the technical approach
- **Milestones**: Track major phases
- **Tasks**: Break down current work
- **Notes**: Capture learnings

## Multi-Agent Coordination

### Define Clear Roles

| Role | Responsibilities |
|------|------------------|
| Foreman | Coordinates, makes decisions, does work |
| Implementer | Writes code, follows specs |
| Reviewer | Reviews code, suggests improvements |
| Tester | Writes tests, finds bugs |

### Avoid Conflicts

- Assign different files/modules to different agents
- Use sequential workflows for shared resources
- Let foreman resolve conflicts

### Regular Sync Points

Periodically check:
- Is everyone aligned on the goal?
- Any blockers?
- Any conflicting changes?

## Troubleshooting Tips

### Agent Not Responding

1. Check the terminal tab for errors
2. Verify MCP setup: `cccc setup --runtime <name>`
3. Try restarting: click Restart in Web UI
4. Check daemon health + recent events: `cccc daemon status` and `cccc tail -n 100`

### Messages Not Delivered

1. Ensure agent is started (green indicator)
2. Check inbox: `cccc inbox --actor-id <id>`
3. Verify the `to` field is correct

### Context Getting Stale

If an agent seems confused:
1. Restart to clear context
2. Re-state the current goal
3. Reference relevant files explicitly

### Stuck in a Loop

If an agent keeps repeating:
1. Stop the agent
2. Clear the task
3. Restart with clearer instructions

## Security Best Practices

### Remote Access

- Create an **Admin Access Token** before any remote exposure
- Prefer Cloudflare Access or Tailscale over raw exposure
- Don't expose port 8848 directly to the internet

### Token Management

- Store access tokens in your password manager or secret store
- Don't commit tokens to git
- Rotate tokens periodically

### Review Agent Changes

- Check commits before pushing
- Use code review agents
- Set up CI/CD guardrails
