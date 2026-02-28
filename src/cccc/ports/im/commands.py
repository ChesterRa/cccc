"""
IM command parser for CCCC.

Parses commands from IM messages:
- /send
- /subscribe, /unsubscribe
- /verbose
- /status, /context
- /pause, /resume
- /launch, /quit
- /help
- @<actor> mentions
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from enum import Enum
from typing import List, Optional, Tuple


class CommandType(str, Enum):
    # Subscription
    SUBSCRIBE = "subscribe"
    UNSUBSCRIBE = "unsubscribe"

    # Messaging
    SEND = "send"

    # Display control
    VERBOSE = "verbose"

    # Status
    STATUS = "status"
    CONTEXT = "context"

    # Control
    PAUSE = "pause"
    RESUME = "resume"
    LAUNCH = "launch"
    QUIT = "quit"

    # Help
    HELP = "help"

    # Not a command - regular message
    MESSAGE = "message"


@dataclass
class ParsedCommand:
    """Result of parsing an IM message."""

    type: CommandType
    text: str  # Original or remaining text
    mentions: List[str]  # @actor mentions
    args: List[str]  # Command arguments


def parse_message(text: str) -> ParsedCommand:
    """
    Parse an IM message into a command or regular message.

    Commands start with / and are case-insensitive.
    Mentions are @actor_id patterns.

    Examples:
        "/subscribe" -> CommandType.SUBSCRIBE
        "/status" -> CommandType.STATUS
        "@peer-a please review" -> CommandType.MESSAGE with mentions=["peer-a"]
        "hello world" -> CommandType.MESSAGE
    """
    text = (text or "").strip()
    if not text:
        return ParsedCommand(type=CommandType.MESSAGE, text="", mentions=[], args=[])

    # Extract mentions
    mentions = _extract_mentions(text)

    # Check for commands (must start with /)
    # Support @BotName /command format (Telegram group privacy mode)
    cmd_match = re.match(r"^(?:@\S+\s+)?/(\w+)(?:@\S+)?(?:\s+(.*))?$", text, re.IGNORECASE | re.DOTALL)

    if cmd_match:
        cmd_name = cmd_match.group(1).lower()
        cmd_args_str = (cmd_match.group(2) or "").strip()
        cmd_args = cmd_args_str.split() if cmd_args_str else []

        cmd_type = _map_command(cmd_name)
        return ParsedCommand(type=cmd_type, text=cmd_args_str, mentions=mentions, args=cmd_args)

    # Regular message
    return ParsedCommand(type=CommandType.MESSAGE, text=text, mentions=mentions, args=[])


def _extract_mentions(text: str) -> List[str]:
    """
    Extract @mentions from text.

    Supports:
    - @actor_id
    - @all
    - @foreman
    - @peers

    Returns list of mentioned targets (without @).
    """
    # Match @word patterns, but not @BotName at the start (Telegram bot mention)
    # Also exclude email-like patterns
    pattern = r"(?<!\S)@([a-zA-Z][a-zA-Z0-9_-]*)"
    matches = re.findall(pattern, text)

    # Filter out common bot mention patterns (usually CamelCase or ends with Bot)
    mentions = []
    for m in matches:
        # Skip if it looks like a bot name (ends with Bot/bot)
        if m.lower().endswith("bot"):
            continue
        ml = m.lower()
        # Preserve special selectors used by CCCC delivery.
        if ml in ("all", "peers", "foreman", "user"):
            mentions.append(f"@{ml}")
        else:
            mentions.append(ml)

    return mentions


def _map_command(cmd_name: str) -> CommandType:
    """Map command name to CommandType."""
    mapping = {
        "subscribe": CommandType.SUBSCRIBE,
        "sub": CommandType.SUBSCRIBE,
        "unsubscribe": CommandType.UNSUBSCRIBE,
        "unsub": CommandType.UNSUBSCRIBE,
        "send": CommandType.SEND,
        "verbose": CommandType.VERBOSE,
        "v": CommandType.VERBOSE,
        "status": CommandType.STATUS,
        "s": CommandType.STATUS,
        "context": CommandType.CONTEXT,
        "ctx": CommandType.CONTEXT,
        "pause": CommandType.PAUSE,
        "resume": CommandType.RESUME,
        "launch": CommandType.LAUNCH,
        "start": CommandType.LAUNCH,  # Alias
        "quit": CommandType.QUIT,
        "stop": CommandType.QUIT,  # Alias
        "help": CommandType.HELP,
        "h": CommandType.HELP,
    }
    return mapping.get(cmd_name, CommandType.MESSAGE)


def format_help(platform: str = "telegram") -> str:
    """Generate help text for IM commands."""
    platform = (platform or "").strip().lower()

    base = """CCCC Commands:

📬 Subscription:
  /subscribe - start receiving messages
  /unsubscribe - stop receiving messages

📨 Messages:
  /send <message> - send using group default (default: foreman)
  /send @<agent> <message> - send to a specific agent
  /send @all <message> - send to all agents
  /send @peers <message> - send to non-foreman agents

👁 Display:
  /verbose - toggle verbose mode (show agent-to-agent messages)

📊 Status:
  /status - show group and agents status
  /context - show project context (vision/overview/tasks)

🎮 Control:
  /pause - pause message delivery
  /resume - resume message delivery
  /launch - start all agents
  /quit - stop all agents

❓ Help:
  /help - show this help"""

    if platform == "telegram":
        return (
            base
            + """

Telegram notes:
  - In groups, @mention the bot to route your message.
  - In direct chat, plain text is sent to foreman by default.
  - Use /send when you need explicit recipients (e.g. @all, @peers)."""
        )

    if platform in ("slack", "discord"):
        return (
            base
            + f"""

{platform.title()} notes:
  - In channels, @mention the bot to route your message.
  - After mention, plain text is sent to foreman by default.
  - Use /send for explicit recipients (e.g. @all, @peers)."""
        )

    return base


def format_status(
    group_title: str,
    group_state: str,
    running: bool,
    actors: List[dict],
) -> str:
    """Format status response."""
    def _actor_label(actor_doc: dict) -> str:
        actor_id = str(actor_doc.get("id") or "").strip() or "?"
        title = str(actor_doc.get("title") or "").strip()
        if title:
            return title
        return actor_id

    lines = [f"📊 {group_title}"]
    lines.append(f"State: {group_state} | Running: {'✓' if running else '✗'}")
    lines.append("")

    if not actors:
        lines.append("No actors configured")
    else:
        lines.append("Actors:")
        for actor in actors:
            actor_label = _actor_label(actor)
            role = actor.get("role", "peer")
            is_running = actor.get("running", False)
            runtime = actor.get("runtime", "codex")
            status_icon = "🟢" if is_running else "⚪"
            role_icon = "👑" if role == "foreman" else "👤"
            lines.append(f"  {status_icon} {role_icon} {actor_label} ({runtime})")

    return "\n".join(lines)


def format_context(context: dict) -> str:
    """Format context response."""
    lines = ["📋 Project Context"]
    lines.append("")

    vision = context.get("vision")
    if vision:
        lines.append(f"🎯 Vision: {vision[:200]}{'...' if len(vision) > 200 else ''}")
        lines.append("")

    overview = context.get("overview")
    if isinstance(overview, dict):
        manual = overview.get("manual")
        if isinstance(manual, dict):
            focus = manual.get("current_focus")
            if focus:
                lines.append(f"📐 Focus: {focus[:300]}{'...' if len(str(focus)) > 300 else ''}")
                lines.append("")

    active_tasks = context.get("active_tasks", [])
    if active_tasks:
        lines.append("📝 Active Tasks:")
        for t in active_tasks[:8]:
            status = t.get("status", "planned")
            icon = "✅" if status == "done" else "🔄" if status == "active" else "📋"
            assignee = t.get("assignee", "")
            assignee_str = f" → {assignee}" if assignee else ""
            parent = t.get("parent_id")
            parent_str = f" (↑{parent})" if parent else ""
            lines.append(f"  {icon} {t.get('name', t.get('title', '?'))}{assignee_str}{parent_str}")

    if not vision and not active_tasks:
        lines.append("No context set yet")

    return "\n".join(lines)
