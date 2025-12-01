# CCCC Frequently Asked Questions

## General

### Do I need to learn all the commands?

**No!** The Setup Panel uses point-and-click (↑↓ + Enter). Tab completion helps with commands in the Runtime Panel. Type `/help` anytime to see everything.

### Can I use CCCC without Telegram/Slack/Discord?

**Yes!** TUI works perfectly standalone. IM bridges are optional and add team collaboration, but they're not required.

### Does CCCC lock me into a specific UI?

**No.** Tmux TUI is the default, but the orchestrator is transport-agnostic. You can interact via TUI, IM bridges, or even direct file writes to the mailbox.

### What if I prefer typing commands over point-and-click?

**Go for it!** The Setup Panel is for convenience. Power users can use slash commands (e.g., `/a`, `/b`) or natural language routing (e.g., `a: Fix the bug`) exclusively.

### Can I run CCCC only locally (no cloud services)?

**Yes.** Everything works without any bridge. Peers run locally via CLI actors. No cloud dependencies unless you explicitly configure IM bridges.

---

## Peers & Collaboration

### Can two Peers really collaborate autonomously?

Yes. Through the mailbox mechanism, PeerA and PeerB automatically exchange messages and pass work results. Once you set a goal, they discuss solutions, divide work, implement, and review each other. You can intervene anytime, but it's not required.

### Do I need to watch the screen constantly?

No. This is CCCC's core value over single-agent systems. Set the task, and Peers progress autonomously. You can check progress via Telegram/Slack/Discord anytime, and items requiring human judgment will notify you via RFD.

### Which CLI is best?

Depends on your needs. Each CLI has different strengths and can be freely assigned to any role. Try the default configuration first, then adjust based on experience.

---

## Configuration

### Do I need to edit configuration files?

Basically no. The TUI Setup Panel supports point-and-click configuration. Advanced users can directly edit YAML files in `.cccc/settings/` for fine-grained control.

### Can I swap actors mid-session?

**Not yet.** Actors are bound at startup via Setup Panel. To change, exit (`/quit`) and restart `cccc run`. Future versions may support hot-swapping.

### How do I customize system prompts?

- Edit `PROJECT.md` at repo root (your project brief)
- Edit `.cccc/rules/PEERA.md` or `.cccc/rules/PEERB.md` (auto-generated, but safe to tweak)

---

## Troubleshooting

### What if a CLI actor is missing?

- TUI Setup Panel shows real-time availability checks
- If a CLI is missing, you'll see hints like "Install with `pip install claude-code`"
- Orchestrator won't start missing actors — right pane stays blank until CLI is available

### How do I debug orchestrator issues?

- Check `.cccc/state/status.json` for current state
- Check `.cccc/state/ledger.jsonl` for event log (includes auto-compact events with detailed diagnostics)
- Check `.cccc/state/orchestrator.log` for runtime logs
- Check `.cccc/logs/*.log` for detailed peer outputs
- Run `cccc doctor` to verify environment

### How do I troubleshoot auto-compact?

**Check if it's working:**
```bash
grep "auto-compact" .cccc/state/ledger.jsonl | tail -20
```

**Common skip reasons:**
- `insufficient-messages` — Peer hasn't done enough work yet (< 6 messages)
- `not-idle` — Peer has inflight/queued messages or hasn't been silent for 2 minutes
- `time-interval` — Not enough time since last compact (< 15 minutes)
- `actor-compact-disabled` — CLI actor doesn't support compact (check `agents.yaml`)

---

## Reset & State Management

### How do I reset state for a new task?

Use `cccc reset` to clear runtime state and start fresh:

```bash
# Basic reset: clears state/mailbox/logs/work and deletes POR/SUBPOR files
cccc reset

# Archive mode: moves POR/SUBPOR to timestamped archive before clearing
cccc reset --archive
```

This is useful when:
- Starting a completely new task after finishing the previous one
- Clearing accumulated inbox messages and runtime state
- Resetting POR/SUBPOR files to begin fresh planning

> **Note**: If the orchestrator is running, you'll be prompted to confirm. Consider running `cccc kill` first.

---

## Safety & Security

### What about safety?

- **Chats never change state** directly — only evidence (patches/tests/logs) does
- **"Done" means verified** — tests passed, logs stable, commit referenced
- **Irreversible changes** (schema, public API, releases) require explicit dual-sign from both peers
- **Soft path locks** prevent overlapping changes
- **Ledger audit trail** records every state transition

---

## Community

### Where can I see real-world examples?

Open an issue or start a discussion in the repo. We love shipping with teams who care about clarity, evidence, and taste.

### How do I connect with the CCCC community?

**Telegram**: [t.me/ccccpair](https://t.me/ccccpair)

Share workflows, troubleshoot issues, discuss features, and connect with other users building with CCCC. All questions and experience levels welcome!
