# Best Practices

Tips for getting the most out of CCCC.

## Setting Up for Success

### Keep Repository Instructions in AGENTS.md

Use `AGENTS.md` for repository instructions shared with your coding tools:
architecture notes, development conventions, constraints, and build/test commands.
Keep it concise and link to longer reference documents when needed. Keep the
team's current focus, task progress, blockers, and handoffs in CCCC's working
summary and task board.

Start with the [short AGENTS.md template](./agents-template) and the
[Builder + Reviewer Pair](./use-cases#use-case-1-builder-reviewer-pair) walkthrough.
Merge useful rules into the project's existing instructions; do not replace
them with a generic template. The template covers ownership, independent review,
evidence, integration and authority boundaries, leaving project-specific rules
and commands to the project.

The **AGENTS.md** section in Project Context views and edits the file at the
current workspace root. It does not combine ancestor or nested instruction
files, change runtime settings, or inject the file into prompts. Each runtime
controls discovery, precedence, and refresh. For example, Claude Code may prefer
an existing `CLAUDE.md`; configure its native instruction loading when necessary.
Saving a file does not prove that running sessions have reloaded it; see
[When instruction files change](#when-instruction-files-change).

Web Model and Dots agents can read repository files on demand with the existing
`cccc_file` tool, for example `cccc_file(rel_path="AGENTS.md")`. Local coding runtimes
use their own file and instruction-loading facilities.

### When instruction files change

Runtimes such as Claude Code and Codex read their instruction files when a
session starts and keep that copy. CCCC watches these files for the agents it
delivers to: `AGENTS.override.md`, `AGENTS.md`, `CLAUDE.md`, `CLAUDE.local.md`
and `.claude/CLAUDE.md`, in the agent's workspace and each directory above it.
After one of them changes, appears or is removed, the next CCCC message to each
agent working in that workspace that has room for it carries one line asking it
to check the current project instructions. A message that carries its full text
names the changed files; see below for Claude's shorter notification. The line
names paths only. CCCC does not reload anything itself, include file contents, or
change which instructions take precedence.

What this does not cover:

- Watching starts before launch for agents CCCC starts; when a connected
  session joins a Group, when the workspace an agent works in is switched, or
  when CCCC's daemon starts; otherwise at the agent's first message. A change
  made before that, or while the daemon was stopped, is not reported.
- Other files that these import with `@` are not followed, and instruction
  files in subdirectories and user-level files outside the workspace's
  directories are not watched.
- If a watched file cannot be read, for example because of its permissions, the
  message goes out without the line, and nothing is compared until the file can
  be read again.
- A Web Model agent is watched per agent and workspace. Starting a new
  conversation in the provider's app does not reset it.
- The line counts as delivered once the runtime accepts the message carrying
  it. That does not prove the model read it. After a rejected or uncertain
  delivery, CCCC keeps its earlier record, so a later message carries the line
  again if the files still differ from that record.

A connected Claude session receives a short form in its notification, which
names the file when one file with a short path changed and otherwise gives only
the number of changed files. When the notification has no room even for that,
or a Codex message would exceed Codex's command-line limit with it, the message
goes out without the line and the line waits for a later message.

CCCC no longer gives `PROJECT.md` special behavior, and the dedicated
`cccc_project_info` tool has been removed. Existing repository documents are left
untouched. Move reusable instructions into an existing `AGENTS.md` deliberately;
keep long project references as ordinary Markdown documents rather than
replacing or duplicating existing instructions.

### Customize the Protocol Reference

The help protocol reference is the group-specific CCCC collaboration contract. You can customize it per-group without turning it into a general reasoning or writing guide.

#### File Priority

CCCC composes help from two layers:

1. **Built-in contract**: the canonical message-delivery section always comes from `resources/cccc-help.md`.
2. **Group overlay (CCCC_HOME)**: `CCCC_HOME/groups/<group_id>/prompts/CCCC_HELP.md` adds common, role, and agent guidance without replacing that canonical section.

To customize local guidance, edit the group overlay (recommended: Web UI → Settings → Guidance).

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
- `## @actor: <actor_id>` - Only the specific agent sees this section
- Sections without markers are the shared/common content visible to everyone

In the Web structured Help editor, these untagged sections are surfaced as `Common Notes`, while the tagged blocks are surfaced as role sections and agent notes.

#### How Agents Consume Help

Agents access help content through MCP tools:

1. **`cccc_bootstrap`** - Returns the caller's `session`, `recovery`, `inbox_preview`, `context_hygiene` and `next_calls`.
   Coordination constraints are included as complete entries within the recovery budget. If the packet reports omitted constraints, read the full list with `cccc_context_get` before acting.
2. **`cccc_help`** - Returns help content on demand

Native sessions pass the explicit `group_id` from a received CCCC message or
their `cccc_session(action="list")` memberships. Bootstrap previews do not consume Mail;
use `cccc_inbox_read` to read and consume it.

The on-demand help payload is returned as:
```json
{
  "markdown": "<filtered content based on role/actor>",
  "source": "CCCC_HOME/.../prompts/CCCC_HELP.md or resources/cccc-help.md"
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

Choose a managed runtime by name:

```bash
# Claude Code
cccc actor add impl --runtime claude

# Codex
cccc actor add review --runtime codex
```

These commands register managed members. CCCC prepares their runtime launch
and MCP integration. The default managed Claude and Codex commands run without
the runtime's approval prompts (Codex also without its sandbox); review
[Autonomy defaults](./runtimes#autonomy-defaults) before assigning work.
See [Supported Runtimes](./runtimes) for confirmation
prompts and recovery behavior. Connecting an already running
native session uses the separate [Native sessions](./local-sessions) setup.

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

### Use the Shared Brief and Tasks

- **Brief**: Record the objective, current focus and shared constraints.
- **Tasks**: Track work that needs an owner, completion criteria or a durable
  handoff; read a task in full before replacing its notes or checklist.
- **Decisions and handoffs**: Keep the reason, evidence and next owner where
  other members can recover them.
- **Agent state**: Keep each member's recovery state current at real transitions.
  It is not proof that external files or services still have the recorded state.

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

### Review How the Team Works

`/cccc-self-evolution` asks the foreman for a short review. It reads what you said and how many
messages, scheduled reminders and open tasks each member has, then sends you at most three proposals,
each on its own card with its evidence. Standing rules, their removal and changes to members wait
for your pick; the foreman may still adjust routine work and tell you. When 10 more top-level tasks
are finished, the foreman's next task update also reminds it to consider a review.

The review text also asks the foreman to check earlier changes recorded as `optimization` tasks:
whether each was used the next time it applied and what happened. A rule that applied in real cases
without the intended effect may come back as a proposal to revise or remove it. The stand-up text
asks the foreman to suggest a review rather than run one, and to mark a question that needs a
deeper look with `Review:` in a related `optimization` task, or in a new planned one that
authorizes no change and points to any task or handoff that already holds the work. In a Group with
few tasks the stand-up may ask you whether to run a review now. When you ask for a review and
nothing needs your decision, the foreman is asked to say briefly what it read. These are
instructions to the model, not checks CCCC enforces.

Standing team rules go into **Constraints** in the Context panel. An agent reads them through
`cccc_bootstrap` when it starts or resumes: up to six as the budget allows, with a note when any are
left out. A long rule, or one about the project only, belongs in `AGENTS.md`.

## Troubleshooting Tips

### Agent Not Responding

1. Check the terminal tab for errors
2. Complete any pending runtime confirmation in its confirmation terminal.
3. Check daemon health and recent events with `cccc daemon status` and
   `cccc tail -n 100`, then follow the [recovery runbook](./operations).
   Managed members receive CCCC's MCP configuration at launch; an already
   running native session follows its runtime's own integration setup.
4. If a restart is appropriate, use **Restart** on the affected member. For
   managed Codex and Claude, this can resume the saved conversation rather
   than clearing its context; use a deliberate new session when that is needed.

### Messages Not Delivered

1. Verify the recipient and delivery mode. Mail waits in Inbox rather than
   prompting immediately.
2. In a POSIX shell, summarize all Mail still waiting as of the current time:

   ```bash
   cccc inbox-summary --cutoff "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
   ```

   The summary does not consume Mail. `cccc inbox` reads and consumes a member's
   Mail, so use the summary for this diagnostic.
3. Check the member's terminal and delivery status. A stored message or a
   running process is not proof that the agent received or read the message.

### Context Getting Stale

If an agent seems confused:
1. Recover the current brief, task and recent relevant messages.
2. Check the actual files, checkout and external facts before continuing.
3. Correct the shared state when it is stale. A process restart can resume the
   saved conversation; it is not a request to clear its context or history.

### Stuck in a Loop

If an agent keeps repeating:
1. Preserve the current work and identify the repeated action and its cause.
2. Interrupt the affected member if needed and within your authority, then give
   a smaller, concrete next step.
3. Choose recovery or a deliberate new session as appropriate for the runtime;
   do not erase tasks or session stores as a substitute for diagnosis.

## Security Best Practices

### Remote Access

- Create the first **Admin Access Token** with the one-time host-local code in `~/.cccc/web_bootstrap_token`; protected APIs remain locked while no administrator exists
- Keep public Web access behind HTTPS; plain HTTP is intended for trusted LAN use and still requires an Admin Access Token
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
