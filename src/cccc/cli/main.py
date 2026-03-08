from __future__ import annotations

import argparse
import sys
from typing import Optional

from .common import *  # noqa: F401,F403
from .group_cmds import *  # noqa: F401,F403
from .actor_cmds import *  # noqa: F401,F403
from .messaging_cmds import *  # noqa: F401,F403
from .space_cmds import *  # noqa: F401,F403
from .im_cmds import *  # noqa: F401,F403
from .system_cmds import *  # noqa: F401,F403

def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="cccc", description="CCCC vNext (working group + scopes)")
    sub = p.add_subparsers(dest="cmd", required=True)

    p_attach = sub.add_parser("attach", help="Attach current path to a working group (auto-create if needed)")
    p_attach.add_argument("path", nargs="?", default=".", help="Path inside a repo/scope (default: .)")
    p_attach.add_argument("--group", dest="group_id", default="", help="Attach scope to an existing group_id (optional)")
    p_attach.set_defaults(func=cmd_attach)

    p_group = sub.add_parser("group", help="Working group operations")
    group_sub = p_group.add_subparsers(dest="action", required=True)

    p_group_create = group_sub.add_parser("create", help="Create an empty working group")
    p_group_create.add_argument("--title", default="working-group", help="Group title (default: working-group)")
    p_group_create.add_argument("--topic", default="", help="Group topic (optional)")
    p_group_create.set_defaults(func=cmd_group_create)

    p_group_show = group_sub.add_parser("show", help="Show group metadata")
    p_group_show.add_argument("group_id", help="Target group_id")
    p_group_show.set_defaults(func=cmd_group_show)

    p_group_update = group_sub.add_parser("update", help="Update group metadata (title/topic)")
    p_group_update.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_group_update.add_argument("--title", default=None, help="New title")
    p_group_update.add_argument("--topic", default=None, help="New topic (use empty string to clear)")
    p_group_update.add_argument("--by", default="user", help="Requester (default: user)")
    p_group_update.set_defaults(func=cmd_group_update)

    p_group_detach = group_sub.add_parser("detach-scope", help="Detach a workspace scope from a group")
    p_group_detach.add_argument("scope_key", help="Scope key to detach (see: cccc group show <id>)")
    p_group_detach.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_group_detach.add_argument("--by", default="user", help="Requester (default: user)")
    p_group_detach.set_defaults(func=cmd_group_detach_scope)

    p_group_delete = group_sub.add_parser("delete", help="Delete a group and its local state (destructive)")
    p_group_delete.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_group_delete.add_argument("--confirm", default="", help="Type the group_id to confirm deletion")
    p_group_delete.add_argument("--by", default="user", help="Requester (default: user)")
    p_group_delete.set_defaults(func=cmd_group_delete)

    p_group_use = group_sub.add_parser("use", help="Set group's active scope (must already be attached)")
    p_group_use.add_argument("group_id", help="Target group_id")
    p_group_use.add_argument("path", nargs="?", default=".", help="Path inside target scope (default: .)")
    p_group_use.set_defaults(func=cmd_group_use)

    p_group_start = group_sub.add_parser("start", help="Start a working group (spawn enabled actors)")
    p_group_start.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_group_start.add_argument("--by", default="user", help="Requester (default: user)")
    p_group_start.set_defaults(func=cmd_group_start)

    p_group_stop = group_sub.add_parser("stop", help="Stop a working group (stop all running actors)")
    p_group_stop.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_group_stop.add_argument("--by", default="user", help="Requester (default: user)")
    p_group_stop.set_defaults(func=cmd_group_stop)

    p_group_set_state = group_sub.add_parser("set-state", help="Set group state (active/idle/paused/stopped)")
    p_group_set_state.add_argument("state", choices=["active", "idle", "paused", "stopped"], help="New state")
    p_group_set_state.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_group_set_state.add_argument("--by", default="user", help="Requester (default: user)")
    p_group_set_state.set_defaults(func=cmd_group_set_state)

    p_groups = sub.add_parser("groups", help="List known working groups")
    p_groups.set_defaults(func=cmd_groups)

    p_use = sub.add_parser("use", help="Set the active working group (for send/tail defaults)")
    p_use.add_argument("group_id", help="Target group_id")
    p_use.set_defaults(func=cmd_use)

    p_active = sub.add_parser("active", help="Show the active working group")
    p_active.set_defaults(func=cmd_active)

    p_actor = sub.add_parser("actor", help="Manage long-session actors in a working group")
    actor_sub = p_actor.add_subparsers(dest="action", required=True)

    p_actor_list = actor_sub.add_parser("list", help="List actors (default: active group)")
    p_actor_list.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_list.set_defaults(func=cmd_actor_list)

    p_actor_add = actor_sub.add_parser("add", help="Add an actor (first actor = foreman, rest = peer)")
    p_actor_add.add_argument("actor_id", help="Actor id (e.g. peer-a, peer-b)")
    p_actor_add.add_argument("--title", default="", help="Display title (optional)")
    p_actor_add.add_argument(
        "--runtime",
        choices=["claude", "codex", "droid", "amp", "auggie", "neovate", "gemini", "cursor", "kilocode", "opencode", "copilot", "custom"],
        default="codex",
        help="Agent runtime (auto-sets command if not provided)",
    )
    p_actor_add.add_argument("--command", default="", help="Command to run (shell-like string; optional, auto-set by --runtime)")
    p_actor_add.add_argument("--env", action="append", default=[], help="Environment var (KEY=VAL), repeatable")
    p_actor_add.add_argument("--scope", default="", help="Default scope path for this actor (optional; must be attached)")
    p_actor_add.add_argument("--submit", choices=["enter", "newline", "none"], default="enter", help="Submit key (default: enter)")
    p_actor_add.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_add.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_add.set_defaults(func=cmd_actor_add)

    p_actor_rm = actor_sub.add_parser("remove", help="Remove an actor")
    p_actor_rm.add_argument("actor_id", help="Actor id")
    p_actor_rm.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_rm.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_rm.set_defaults(func=cmd_actor_remove)

    p_actor_start = actor_sub.add_parser("start", help="Set actor enabled=true (desired run-state)")
    p_actor_start.add_argument("actor_id", help="Actor id")
    p_actor_start.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_start.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_start.set_defaults(func=cmd_actor_start)

    p_actor_stop = actor_sub.add_parser("stop", help="Set actor enabled=false (desired run-state)")
    p_actor_stop.add_argument("actor_id", help="Actor id")
    p_actor_stop.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_stop.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_stop.set_defaults(func=cmd_actor_stop)

    p_actor_restart = actor_sub.add_parser("restart", help="Record restart intent and keep enabled=true")
    p_actor_restart.add_argument("actor_id", help="Actor id")
    p_actor_restart.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_restart.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_restart.set_defaults(func=cmd_actor_restart)

    p_actor_update = actor_sub.add_parser("update", help="Update an actor (title/command/env/scope/enabled/runtime)")
    p_actor_update.add_argument("actor_id", help="Actor id")
    p_actor_update.add_argument("--title", default=None, help="New title")
    p_actor_update.add_argument("--runtime", choices=["claude", "codex", "droid", "amp", "auggie", "neovate", "gemini", "cursor", "kilocode", "opencode", "copilot", "custom"], default=None, help="New runtime")
    p_actor_update.add_argument("--command", default=None, help="Replace command (shell-like string); use empty to clear")
    p_actor_update.add_argument("--env", action="append", default=[], help="Replace env with these KEY=VAL entries (repeatable)")
    p_actor_update.add_argument("--scope", default="", help="Set default scope path (must be attached)")
    p_actor_update.add_argument("--submit", choices=["enter", "newline", "none"], default=None, help="Submit key")
    p_actor_update.add_argument("--enabled", type=int, choices=[0, 1], default=None, help="Set enabled (1) or disabled (0)")
    p_actor_update.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_update.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_update.set_defaults(func=cmd_actor_update)

    p_actor_secrets = actor_sub.add_parser("secrets", help="Manage runtime-only secrets env (not in ledger)")
    p_actor_secrets.add_argument("actor_id", help="Actor id")
    p_actor_secrets.add_argument("--set", action="append", default=[], help="Set secret env (KEY=VALUE), repeatable")
    p_actor_secrets.add_argument("--unset", action="append", default=[], help="Unset secret key (KEY), repeatable")
    p_actor_secrets.add_argument("--clear", action="store_true", help="Clear all secrets for this actor")
    p_actor_secrets.add_argument("--keys", action="store_true", help="List configured keys (no values)")
    p_actor_secrets.add_argument("--restart", action="store_true", help="Restart actor after updating secrets")
    p_actor_secrets.add_argument("--by", default="user", help="Requester (default: user)")
    p_actor_secrets.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_actor_secrets.set_defaults(func=cmd_actor_secrets)

    p_inbox = sub.add_parser("inbox", help="List unread messages for an actor (chat messages + system notifications)")
    p_inbox.add_argument("--actor-id", required=True, help="Target actor id")
    p_inbox.add_argument("--by", default="user", help="Requester (default: user)")
    p_inbox.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_inbox.add_argument("--limit", type=int, default=50, help="Max messages to return (default: 50)")
    p_inbox.add_argument("--kind-filter", choices=["all", "chat", "notify"], default="all", help="Filter by message type: all (default), chat (messages only), notify (system notifications only)")
    p_inbox.add_argument("--mark-read", action="store_true", help="Mark returned messages as read up to the last one")
    p_inbox.set_defaults(func=cmd_inbox)

    p_read = sub.add_parser("read", help="Mark a message event as read for an actor")
    p_read.add_argument("event_id", help="Target message event id")
    p_read.add_argument("--actor-id", required=True, help="Target actor id")
    p_read.add_argument("--by", default="user", help="Requester (default: user)")
    p_read.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_read.set_defaults(func=cmd_read)

    p_prompt = sub.add_parser("prompt", help="Render a concise SYSTEM prompt for a group actor")
    p_prompt.add_argument("--actor-id", required=True, help="Target actor id")
    p_prompt.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_prompt.set_defaults(func=cmd_prompt)

    p_send = sub.add_parser("send", help="Append a chat message into the active group ledger (or --group)")
    p_send.add_argument("text", help="Message text")
    p_send.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_send.add_argument("--by", default="user", help="Sender label (default: user)")
    p_send.add_argument(
        "--to",
        action="append",
        default=[],
        help="Recipients/selectors (repeatable, supports comma-separated, e.g. --to peer-a --to @foreman,@peers)",
    )
    p_send.add_argument("--priority", choices=["normal", "attention"], default="normal", help="Message priority")
    p_send.add_argument("--reply-required", action="store_true", help="Require recipients to reply")
    p_send.add_argument("--path", default="", help="Send message under this scope (path inside repo/scope)")
    p_send.set_defaults(func=cmd_send)

    p_reply = sub.add_parser("reply", help="Reply to a message (IM-style, with quote)")
    p_reply.add_argument("event_id", help="Event ID of the message to reply to")
    p_reply.add_argument("text", help="Reply text")
    p_reply.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_reply.add_argument("--by", default="user", help="Sender label (default: user)")
    p_reply.add_argument(
        "--to",
        action="append",
        default=[],
        help="Recipients (default: original sender); repeatable, comma-separated",
    )
    p_reply.add_argument("--priority", choices=["normal", "attention"], default="normal", help="Message priority")
    p_reply.add_argument("--reply-required", action="store_true", help="Require recipients to reply")
    p_reply.set_defaults(func=cmd_reply)

    p_tail = sub.add_parser("tail", help="Tail the active group's ledger (or --group)")
    p_tail.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_tail.add_argument("-n", "--lines", type=int, default=50, help="Show last N lines (default: 50)")
    p_tail.add_argument("-f", "--follow", action="store_true", help="Follow (like tail -f)")
    p_tail.set_defaults(func=cmd_tail)

    p_ledger = sub.add_parser("ledger", help="Ledger maintenance (snapshot/compaction)")
    ledger_sub = p_ledger.add_subparsers(dest="action", required=True)

    p_ls = ledger_sub.add_parser("snapshot", help="Write a ledger snapshot under group state/")
    p_ls.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_ls.add_argument("--by", default="user", help="Requester (default: user)")
    p_ls.add_argument("--reason", default="manual", help="Reason label (default: manual)")
    p_ls.set_defaults(func=cmd_ledger_snapshot)

    p_lc = ledger_sub.add_parser("compact", help="Archive globally-read events to keep active ledger small")
    p_lc.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_lc.add_argument("--by", default="user", help="Requester (default: user)")
    p_lc.add_argument("--reason", default="manual", help="Reason label (default: manual)")
    p_lc.add_argument("--force", action="store_true", help="Force a compaction run (ignore thresholds)")
    p_lc.set_defaults(func=cmd_ledger_compact)

    p_daemon = sub.add_parser("daemon", help="Manage ccccd daemon")
    p_daemon.add_argument("action", choices=["start", "stop", "status"], help="Action")
    p_daemon.set_defaults(func=cmd_daemon)

    # IM Bridge commands
    p_im = sub.add_parser("im", help="Manage IM bridge (Telegram/Slack/Discord/Feishu/Lark/DingTalk)")
    im_sub = p_im.add_subparsers(dest="action", required=True)

    p_im_set = im_sub.add_parser("set", help="Set IM bridge configuration")
    p_im_set.add_argument("platform", choices=["telegram", "slack", "discord", "feishu", "dingtalk"], help="IM platform")
    p_im_set.add_argument("--token-env", default="", help="Environment variable name for token (telegram/discord)")
    p_im_set.add_argument("--bot-token-env", default="", help="Bot token env var (Slack: xoxb- for outbound)")
    p_im_set.add_argument("--app-token-env", default="", help="App token env var (Slack: xapp- for inbound Socket Mode)")
    p_im_set.add_argument("--app-key-env", default="", help="App ID (Feishu/Lark) / App Key (DingTalk) env var")
    p_im_set.add_argument("--app-secret-env", default="", help="App Secret (Feishu/Lark/DingTalk) env var")
    p_im_set.add_argument("--domain", default="", help="Feishu domain override: feishu (CN) or lark (Global)")
    p_im_set.add_argument("--robot-code-env", default="", help="Robot code env var (DingTalk; optional but recommended)")
    p_im_set.add_argument("--robot-code", default="", help="Robot code value directly (DingTalk; not recommended, prefer env var)")
    p_im_set.add_argument("--token", default="", help="Token value directly (not recommended, use env vars)")
    p_im_set.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_set.set_defaults(func=cmd_im_set)

    p_im_unset = im_sub.add_parser("unset", help="Remove IM bridge configuration")
    p_im_unset.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_unset.set_defaults(func=cmd_im_unset)

    p_im_config = im_sub.add_parser("config", help="Show IM bridge configuration")
    p_im_config.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_config.set_defaults(func=cmd_im_config)

    p_im_start = im_sub.add_parser("start", help="Start IM bridge")
    p_im_start.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_start.set_defaults(func=cmd_im_start)

    p_im_stop = im_sub.add_parser("stop", help="Stop IM bridge")
    p_im_stop.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_stop.set_defaults(func=cmd_im_stop)

    p_im_status = im_sub.add_parser("status", help="Show IM bridge status")
    p_im_status.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_status.set_defaults(func=cmd_im_status)

    p_im_logs = im_sub.add_parser("logs", help="Show IM bridge logs")
    p_im_logs.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_logs.add_argument("-n", "--lines", type=int, default=50, help="Number of lines to show (default: 50)")
    p_im_logs.add_argument("-f", "--follow", action="store_true", help="Follow log output (like tail -f)")
    p_im_logs.set_defaults(func=cmd_im_logs)

    p_im_bind = im_sub.add_parser("bind", help="Bind a pending authorization key to authorize a chat")
    p_im_bind.add_argument("--key", required=True, help="Authorization key from /subscribe")
    p_im_bind.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_bind.set_defaults(func=cmd_im_bind)

    p_im_authorized = im_sub.add_parser("authorized", help="List authorized chats")
    p_im_authorized.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_authorized.set_defaults(func=cmd_im_authorized)

    p_im_revoke = im_sub.add_parser("revoke", help="Revoke authorization for a chat")
    p_im_revoke.add_argument("--chat-id", required=True, dest="chat_id", help="Chat ID to revoke")
    p_im_revoke.add_argument("--thread-id", type=int, default=0, dest="thread_id", help="Thread ID (default: 0)")
    p_im_revoke.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_im_revoke.set_defaults(func=cmd_im_revoke)

    p_web = sub.add_parser("web", help="Run web server only (requires daemon to be running)")
    p_web.add_argument("--host", default="127.0.0.1", help="Bind host (default: 127.0.0.1)")
    p_web.add_argument("--port", type=int, default=8848, help="Bind port (default: 8848)")
    p_web.add_argument(
        "--mode",
        choices=["normal", "exhibit"],
        default="normal",
        help="Web mode: normal (read/write) or exhibit (read-only) (default: normal)",
    )
    p_web.add_argument("--exhibit", action="store_true", help="Shortcut for: --mode exhibit")
    p_web.add_argument("--reload", action="store_true", help="Enable autoreload (dev)")
    p_web.add_argument("--log-level", default="info", help="Uvicorn log level (default: info)")
    p_web.set_defaults(func=cmd_web)

    p_mcp = sub.add_parser("mcp", help="Run the MCP server (stdio mode, for agent runtimes)")
    p_mcp.set_defaults(func=cmd_mcp)

    p_setup = sub.add_parser("setup", help="Setup MCP for agent runtimes (configure MCP, print guidance)")
    p_setup.add_argument(
        "--runtime",
        choices=["claude", "codex", "droid", "amp", "auggie", "neovate", "gemini", "cursor", "kilocode", "opencode", "copilot", "custom"],
        default="",
        help="Target runtime (default: all supported runtimes)",
    )
    p_setup.add_argument("--path", default=".", help="Project path (default: current directory)")
    p_setup.set_defaults(func=cmd_setup)

    p_doctor = sub.add_parser("doctor", help="Check environment and show available agent runtimes")
    p_doctor.add_argument("--all", action="store_true", help="Show all known runtimes (not just primary ones)")
    p_doctor.set_defaults(func=cmd_doctor)

    p_runtime = sub.add_parser("runtime", help="Manage agent runtimes")
    runtime_sub = p_runtime.add_subparsers(dest="action", required=True)

    p_runtime_list = runtime_sub.add_parser("list", help="List available agent runtimes")
    p_runtime_list.add_argument("--all", action="store_true", help="Show all known runtimes (not just primary ones)")
    p_runtime_list.set_defaults(func=cmd_runtime_list)

    p_space = sub.add_parser("space", help="Manage Group Space provider-backed shared memory")
    space_sub = p_space.add_subparsers(dest="action", required=True)

    p_space_status = space_sub.add_parser("status", help="Show Group Space status")
    p_space_status.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_space_status.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_status.set_defaults(func=cmd_space_status)

    p_space_credential = space_sub.add_parser("credential", help="Manage Group Space provider credentials")
    space_credential_sub = p_space_credential.add_subparsers(dest="credential_action", required=True)

    p_space_credential_status = space_credential_sub.add_parser("status", help="Show provider credential status (masked)")
    p_space_credential_status.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_credential_status.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_credential_status.set_defaults(func=cmd_space_credential_status)

    p_space_credential_set = space_credential_sub.add_parser("set", help="Set provider credential (write-only)")
    p_space_credential_set.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_credential_set.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_credential_set.add_argument("--auth-json", default="", help="Provider auth JSON payload")
    p_space_credential_set.add_argument("--auth-json-file", default="", help="Path to provider auth JSON file")
    p_space_credential_set.set_defaults(func=cmd_space_credential_set)

    p_space_credential_clear = space_credential_sub.add_parser("clear", help="Clear stored provider credential")
    p_space_credential_clear.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_credential_clear.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_credential_clear.set_defaults(func=cmd_space_credential_clear)

    p_space_health = space_sub.add_parser("health", help="Run Group Space provider health check")
    p_space_health.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_health.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_health.set_defaults(func=cmd_space_health)

    p_space_auth = space_sub.add_parser("auth", help="Manage Group Space provider auth flow")
    space_auth_sub = p_space_auth.add_subparsers(dest="auth_action", required=True)

    p_space_auth_status = space_auth_sub.add_parser("status", help="Show provider auth flow status")
    p_space_auth_status.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_auth_status.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_auth_status.set_defaults(func=cmd_space_auth_status)

    p_space_auth_start = space_auth_sub.add_parser("start", help="Start provider auth flow")
    p_space_auth_start.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_auth_start.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_auth_start.add_argument(
        "--timeout-seconds",
        type=int,
        default=900,
        help="Auth flow timeout seconds (60-1800, default: 900)",
    )
    p_space_auth_start.set_defaults(func=cmd_space_auth_start)

    p_space_auth_cancel = space_auth_sub.add_parser("cancel", help="Cancel provider auth flow")
    p_space_auth_cancel.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_auth_cancel.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_auth_cancel.set_defaults(func=cmd_space_auth_cancel)

    p_space_bind = space_sub.add_parser("bind", help="Bind group to a provider remote space")
    p_space_bind.add_argument("remote_space_id", nargs="?", default="", help="Provider remote space/notebook ID (optional; auto-create when omitted)")
    p_space_bind.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_space_bind.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_bind.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_bind.set_defaults(func=cmd_space_bind)

    p_space_unbind = space_sub.add_parser("unbind", help="Unbind group from provider remote space")
    p_space_unbind.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_space_unbind.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_unbind.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_unbind.set_defaults(func=cmd_space_unbind)

    p_space_sync = space_sub.add_parser("sync", help="Synchronize repo space/ resources to provider")
    p_space_sync.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_space_sync.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_sync.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_sync.add_argument("--force", action="store_true", help="Force full reconcile even if no local changes detected")
    p_space_sync.set_defaults(func=cmd_space_sync)

    p_space_ingest = space_sub.add_parser("ingest", help="Submit an ingest job to Group Space")
    p_space_ingest.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_space_ingest.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_ingest.add_argument("--kind", choices=["context_sync", "resource_ingest"], default="context_sync", help="Job kind")
    p_space_ingest.add_argument("--payload", default="{}", help="JSON object payload (default: {})")
    p_space_ingest.add_argument("--idempotency-key", default="", help="Optional idempotency key for dedupe")
    p_space_ingest.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_ingest.set_defaults(func=cmd_space_ingest)

    p_space_query = space_sub.add_parser("query", help="Query Group Space provider-backed memory")
    p_space_query.add_argument("query", help="Query text")
    p_space_query.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_space_query.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_query.add_argument("--options", default="{}", help="JSON object options (supported: source_ids)")
    p_space_query.set_defaults(func=cmd_space_query)

    p_space_jobs = space_sub.add_parser("jobs", help="List/retry/cancel Group Space jobs")
    space_jobs_sub = p_space_jobs.add_subparsers(dest="jobs_action", required=True)

    p_space_jobs_list = space_jobs_sub.add_parser("list", help="List jobs")
    p_space_jobs_list.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_space_jobs_list.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_jobs_list.add_argument("--state", choices=["pending", "running", "succeeded", "failed", "canceled"], default="", help="Optional state filter")
    p_space_jobs_list.add_argument("--limit", type=int, default=50, help="Max jobs to return (default: 50)")
    p_space_jobs_list.set_defaults(func=cmd_space_jobs_list)

    p_space_jobs_retry = space_jobs_sub.add_parser("retry", help="Retry a failed/canceled job")
    p_space_jobs_retry.add_argument("job_id", help="Job ID")
    p_space_jobs_retry.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_space_jobs_retry.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_jobs_retry.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_jobs_retry.set_defaults(func=cmd_space_jobs_retry)

    p_space_jobs_cancel = space_jobs_sub.add_parser("cancel", help="Cancel a pending/running job")
    p_space_jobs_cancel.add_argument("job_id", help="Job ID")
    p_space_jobs_cancel.add_argument("--group", default="", help="Target group_id (default: active group)")
    p_space_jobs_cancel.add_argument("--provider", choices=["notebooklm"], default="notebooklm", help="Provider (default: notebooklm)")
    p_space_jobs_cancel.add_argument("--by", default="user", help="Requester (default: user)")
    p_space_jobs_cancel.set_defaults(func=cmd_space_jobs_cancel)

    p_ver = sub.add_parser("version", help="Show version")
    p_ver.set_defaults(func=cmd_version)

    p_status = sub.add_parser("status", help="Show overall CCCC status (daemon, groups, actors)")
    p_status.set_defaults(func=cmd_status)

    return p

def main(argv: Optional[list[str]] = None) -> int:
    if argv is None:
        argv = sys.argv[1:]
    if len(argv) == 0:
        return int(_default_entry())
    parser = build_parser()
    args = parser.parse_args(argv)
    return int(args.func(args))

if __name__ == "__main__":
    raise SystemExit(main())
