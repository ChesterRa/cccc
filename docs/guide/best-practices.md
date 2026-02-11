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

### Customize the Help Playbook

The help playbook is the collaboration contract that agents follow. You can customize it per-group.

#### File Priority

CCCC loads help content with the following priority:

1. **Group override (CCCC_HOME)**: `CCCC_HOME/groups/<group_id>/prompts/CCCC_HELP.md` (highest priority)
2. **Built-in default**: `cccc.resources/cccc-help.md` (fallback)

To customize, edit the group prompt override (recommended: Web UI → Settings → Guidance).

You can also locate the file path from the Web UI (it shows the exact override path per group).

#### Conditional Content Markers

You can show different content to different roles using conditional markers:

```markdown
# My Project Help

## General Rules (all roles see this)
- Follow the coding standards
- Write tests for new features

## @role: foreman
### Foreman-Only Section
- You are responsible for coordinating the team
- Make final decisions on architecture

## @role: peer
### Peer-Only Section
- Focus on your assigned tasks
- Report blockers to foreman

## @actor: impl-agent
### This Section Only Visible to impl-agent
- You handle the core implementation
```

**Marker syntax:**
- `## @role: foreman` - Only foreman sees this section
- `## @role: peer` - Only peers see this section
- `## @actor: <actor_id>` - Only the specific actor sees this section
- Sections without markers are visible to all roles

#### How Agents Consume Help

Agents access help content through MCP tools:

1. **`cccc_bootstrap`** - Returns help as part of session initialization
2. **`cccc_help`** - Returns help content on demand

The help is returned as:
```json
{
  "markdown": "<filtered content based on role/actor>",
  "source": "CCCC_HOME/.../prompts/CCCC_HELP.md or cccc.resources/cccc-help.md"
}
```

Agents read the markdown and follow the rules semantically. There's no special parsing - the content is written in clear, imperative language that AI understands naturally.

#### Tips for Effective Help Content

- Use imperative language: "MUST use MCP" instead of "should consider using MCP"
- Be specific: "Reply within 30 seconds" instead of "respond quickly"
- Structure with clear headings for easy navigation
- Include examples for complex workflows
- Keep sections focused - one topic per section

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

- `@all` for announcements or general questions
- `@foreman` for coordination decisions
- `@specific-agent` for targeted tasks

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
4. Check daemon logs: `cccc daemon logs -f`

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

- Always use `CCCC_WEB_TOKEN` for remote access
- Prefer Cloudflare Access or Tailscale over raw exposure
- Don't expose port 8848 directly to the internet

### Token Management

- Store tokens in environment variables
- Don't commit tokens to git
- Rotate tokens periodically

### Review Agent Changes

- Check commits before pushing
- Use code review agents
- Set up CI/CD guardrails
