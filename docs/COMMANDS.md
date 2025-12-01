# CCCC Command Reference

Complete cross-platform command reference for CCCC Pair.

## TUI Commands

All commands are accessible via Tab completion. Type `/` and press Tab to explore.

| Command | Description | Example |
|---------|-------------|---------|
| `/help` | Show full command list | `/help` |
| `/a <text>` | Send message to PeerA | `/a Review the auth logic` |
| `/b <text>` | Send message to PeerB | `/b Fix the failing test` |
| `/both <text>` | Send message to both peers | `/both Let's plan the next milestone` |
| `/pause` | Pause handoff delivery (messages saved to inbox) | `/pause` |
| `/resume` | Resume handoff delivery (sends NUDGE for pending) | `/resume` |
| `/restart peera\|peerb\|both` | Restart peer CLI process | `/restart peerb` |
| `/quit` | Exit CCCC (detach tmux) | `/quit` |
| `/setup` | Toggle Setup Panel | `/setup` |
| `/foreman on\|off\|status\|now` | Control Foreman (if enabled) | `/foreman status` |
| `/aux <prompt>` | Run Aux helper once | `/aux Run full test suite` |
| `/verbose on\|off` | Toggle peer summaries + Foreman CC | `/verbose off` |

## Natural Language Routing

You can also use routing prefixes for natural language input (no slash needed):

```
a: Review the authentication logic and suggest improvements
b: Implement the fix with comprehensive tests
both: Let's discuss the roadmap for next quarter
```

## Cross-Platform Command Matrix

| Category | Command | TUI | Telegram | Slack | Discord |
|----------|---------|-----|----------|-------|---------|
| **Routing** | Send to PeerA | `/a` | `/a` or `a:` | `a:` | `a:` |
| | Send to PeerB | `/b` | `/b` or `b:` | `b:` | `b:` |
| | Send to Both | `/both` | `/both` or `both:` | `both:` | `both:` |
| **Passthrough** | CLI to PeerA | — | `a!` or `/pa` | `a!` | `a!` |
| | CLI to PeerB | — | `b!` or `/pb` | `b!` | `b!` |
| | CLI to Both | — | `/pboth` | — | — |
| **Control** | Pause delivery | `/pause` | `/pause` | `!pause` | `!pause` |
| | Resume delivery | `/resume` | `/resume` | `!resume` | `!resume` |
| | Restart peer | `/restart` | `/restart` | `!restart` | `!restart` |
| | Quit/Exit | `/quit` | — | — | — |
| **Operations** | Foreman control | `/foreman` | `/foreman` | `!foreman` | `!foreman` |
| | Run Aux helper | `/aux` | `/aux` | `!aux` | `!aux` |
| | Toggle verbose | `/verbose` | `/verbose` | `!verbose` | `!verbose` |
| **Subscription** | Get chat ID | — | `/whoami` | — | — |
| | Subscribe | — | `/subscribe` | `!subscribe` | `!subscribe` |
| | Unsubscribe | — | `/unsubscribe` | `!unsubscribe` | `!unsubscribe` |
| **Utility** | Show status | — | `/status` | `!status` | `!status` |
| | Show help | `/help` | `/help` | `!help` | `!help` |
| | Setup panel | `/setup` | — | — | — |

> **Legend**: `/cmd` = slash prefix, `!cmd` = exclamation prefix, `x:` = colon routing, `x!` = passthrough, — = not available

## Keyboard Shortcuts

CCCC TUI includes rich keyboard support for efficiency.

| Shortcut | Action |
|----------|--------|
| `Tab` | Auto-complete commands |
| `Up / Down` | Navigate command history |
| `Ctrl+R` | Reverse search history |
| `Ctrl+A` | Jump to line start |
| `Ctrl+E` | Jump to line end |
| `Ctrl+W` | Delete word backward |
| `Ctrl+U` | Delete to line start |
| `Ctrl+K` | Delete to line end |
| `PageUp / PageDown` | Scroll timeline |
| `Ctrl+L` | Clear timeline |
| `/quit` | Exit CCCC |
| `Ctrl+B D` | Detach tmux (alternative exit) |

> **Pro tip**: Use `Ctrl+R` to quickly find and re-run previous commands without retyping.

## IM Bridge Commands

### Telegram Commands (slash prefix)

- `/a` `/b` `/both` — Routing aliases
- `/pa` `/pb` `/pboth` — Passthrough aliases (group-friendly)
- `/aux <prompt>` — Run Aux helper once
- `/foreman on|off|status|now` — Control Foreman scheduler
- `/restart peera|peerb|both` — Restart peer CLI
- `/pause` `/resume` — Pause/resume handoff delivery
- `/status` — Show system status
- `/verbose on|off` — Toggle peer summaries
- `/whoami` `/subscribe` `/unsubscribe` — Subscription management
- `/help` — Show command help

### Slack / Discord Commands (exclamation prefix)

- `!aux <prompt>` — Run Aux helper once
- `!foreman on|off|status|now` — Control Foreman scheduler
- `!restart peera|peerb|both` — Restart peer CLI
- `!pause` `!resume` — Pause/resume handoff delivery
- `!status` — Show system status
- `!verbose on|off` — Toggle peer summaries
- `!subscribe` `!unsubscribe` — Subscription management
- `!help` — Show command help

> **Note**: Telegram uses `/` prefix (native bot commands). Slack/Discord use `!` prefix (to avoid platform command interception).
