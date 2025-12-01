# -*- coding: utf-8 -*-
"""
Format status and help messages for IM display.
Shared between Telegram, Slack, and Discord bridges.
"""
from __future__ import annotations
import json
import time
from pathlib import Path
from typing import Any, Dict, Optional


def format_help_for_im(prefix: str = '/') -> str:
    """
    Format help message for IM.

    Args:
        prefix: Command prefix ('/' for Telegram, '!' for Slack/Discord)

    Design principles:
    - Grouped by function
    - Visual hierarchy with section headers
    - Concise command descriptions
    - Easy to scan
    """
    p = prefix

    lines = [
        "\u2501\u2501\u2501 CCCC Commands \u2501\u2501\u2501",  # ━━━
        "",
        "\u25b8 Message Peers",  # ▸
        f"  a: or {p}a \u2192 send to PeerA",
        f"  b: or {p}b \u2192 send to PeerB",
        f"  both: or {p}both \u2192 send to both",
        "",
        "\u25b8 Control",
        f"  {p}pause \u2192 pause delivery",
        f"  {p}resume \u2192 resume delivery",
        f"  {p}restart [a|b|both] \u2192 restart CLI",
        "",
        "\u25b8 Agents",
        f"  {p}aux <prompt> \u2192 run Aux helper",
        f"  {p}foreman [on|off|now|status]",
        "",
        "\u25b8 Info",
        f"  {p}status \u2192 system status",
        f"  {p}verbose [on|off] \u2192 toggle summaries",
        "",
        "\u25b8 Account",
        f"  {p}subscribe \u2192 opt-in",
        f"  {p}unsubscribe \u2192 opt-out",
        f"  {p}whoami \u2192 show chat ID",
    ]

    return '\n'.join(lines)


def _load_yaml_safe(path: Path) -> Dict[str, Any]:
    """Load YAML file safely, with fallback to JSON."""
    if not path.exists():
        return {}
    try:
        import yaml
        return yaml.safe_load(path.read_text(encoding='utf-8')) or {}
    except Exception:
        try:
            return json.loads(path.read_text(encoding='utf-8'))
        except Exception:
            return {}


def format_status_for_im(state_dir: Path) -> str:
    """
    Format status for IM display.

    Reads from TWO sources:
    - settings/cli_profiles.yaml: CURRENT agent configuration (authoritative)
    - state/status.json: Runtime status (handoffs, paused, etc.)

    Design principles:
    - Health/running state first (most important)
    - Scannable with emojis for quick visual parsing
    - Actionable information only
    - Compact for mobile/IM viewing
    """
    home_dir = state_dir.parent  # .cccc/state -> .cccc
    settings_dir = home_dir / "settings"

    # Read CURRENT agent config from settings (authoritative source)
    cli_profiles = _load_yaml_safe(settings_dir / "cli_profiles.yaml")
    roles_cfg = cli_profiles.get('roles') or {}

    # Read runtime status from status.json
    st_path = state_dir / "status.json"
    try:
        st = json.loads(st_path.read_text(encoding='utf-8')) if st_path.exists() else {}
    except Exception:
        st = {}

    lines = []

    # === Header: Overall health ===
    paused = st.get('paused', False)

    if paused:
        health = "\u23f8 Paused"  # ⏸
    elif st:
        health = "\u25b6 Running"  # ▶
    else:
        health = "\u26a0 Offline"  # ⚠

    lines.append(f"\u2501\u2501\u2501 CCCC {health} \u2501\u2501\u2501")  # ━━━

    # === Peers: Read from settings (authoritative) ===
    peer_a_actor = (roles_cfg.get('peerA') or {}).get('actor') or 'unset'
    peer_b_actor = (roles_cfg.get('peerB') or {}).get('actor') or 'unset'
    aux_actor = (roles_cfg.get('aux') or {}).get('actor') or ''

    # CLI availability from runtime status (if available)
    setup = st.get('setup') or {}
    cli = setup.get('cli') or {}
    peer_a_ok = (cli.get('peerA') or {}).get('available', True)  # Assume OK if no status
    peer_b_ok = (cli.get('peerB') or {}).get('available', True)

    a_status = "\u2713" if peer_a_ok else "\u2717"  # ✓ or ✗
    b_status = "\u2713" if peer_b_ok else "\u2717"

    lines.append(f"\u25b8 PeerA: {peer_a_actor} {a_status}")  # ▸
    lines.append(f"\u25b8 PeerB: {peer_b_actor} {b_status}")

    # === Handoffs: Activity metrics ===
    reset = st.get('reset') or {}
    total = reset.get('handoffs_total', 0)
    h_a = reset.get('handoffs_peerA', 0)
    h_b = reset.get('handoffs_peerB', 0)

    lines.append(f"\u25b8 Handoffs: {total} (A:{h_a} B:{h_b})")

    # === Inbox: Count files directly from mailbox (authoritative) ===
    mailbox_dir = home_dir / "mailbox"
    try:
        inbox_a = len(list((mailbox_dir / "peerA" / "inbox").glob("*"))) if (mailbox_dir / "peerA" / "inbox").exists() else 0
    except Exception:
        inbox_a = 0
    try:
        inbox_b = len(list((mailbox_dir / "peerB" / "inbox").glob("*"))) if (mailbox_dir / "peerB" / "inbox").exists() else 0
    except Exception:
        inbox_b = 0

    lines.append(f"\u25b8 Inbox: A:{inbox_a} B:{inbox_b}")  # Always show

    # === Aux: Optional helper (from settings) ===
    if aux_actor and aux_actor != 'none':
        aux_info = st.get('aux') or {}
        aux_state = 'running' if aux_info.get('running') else 'idle'
        lines.append(f"\u25b8 Aux: {aux_actor} ({aux_state})")

    # === Foreman: From settings ===
    foreman_cfg = _load_yaml_safe(settings_dir / "foreman.yaml")
    foreman_enabled = foreman_cfg.get('enabled', False)
    foreman_agent = foreman_cfg.get('agent', '')

    if foreman_enabled and foreman_agent:
        foreman_st = st.get('foreman') or {}
        f_running = foreman_st.get('running', False)
        f_next = foreman_st.get('next_due', '')
        if f_running:
            f_state = 'running'
        elif f_next:
            f_state = f'next {f_next}'
        else:
            f_state = 'idle'
        lines.append(f"\u25b8 Foreman: {foreman_agent} ({f_state})")

    # === Timestamp ===
    ts = st.get('ts', '')
    if ts:
        # Show just time portion
        time_part = ts.split(' ')[-1] if ' ' in ts else ts
        lines.append(f"\u23f1 {time_part}")  # ⏱

    return '\n'.join(lines)


def format_status_compact(state_dir: Path) -> str:
    """
    Ultra-compact single-line status for notifications/headers.
    """
    st_path = state_dir / "status.json"
    try:
        st = json.loads(st_path.read_text(encoding='utf-8')) if st_path.exists() else {}
    except Exception:
        return "status: unavailable"

    paused = st.get('paused', False)
    reset = st.get('reset') or {}
    total = reset.get('handoffs_total', 0)

    status = "\u23f8" if paused else "\u25b6"  # ⏸ or ▶
    return f"{status} handoffs:{total}"
