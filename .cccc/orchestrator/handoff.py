# -*- coding: utf-8 -*-
"""
Stateful handoff API (Context factory)
- Equivalent to legacy inline logic in orchestrator_tmux.py for send_handoff/keepalive,
  now modularized via explicit ctx wiring to keep behavior consistent.
"""
from __future__ import annotations
import re, time, hashlib
from types import SimpleNamespace
from typing import Any, Dict, Optional
from pathlib import Path

from .handoff_helpers import (
    _plain_text_without_tags_and_mid,
    _write_inbox_message,
    _append_suffix_inside,
    _compose_nudge,
)
from .logging_util import log_ledger

def make(ctx: Dict[str, Any]):
    home: Path = ctx['home']
    sys_refresh_every: int = int(ctx.get('system_refresh_every') or 3)

    def _maybe_selfcheck_multi(receiver_labels, pl_text: str, meaningful: bool = True) -> bool:
        try:
            if not ctx['self_check_enabled'] or ctx['in_self_check']['v'] or ctx['self_check_every'] <= 0:
                return False
            pl = pl_text or ""
            is_nudge = pl.strip().startswith("[NUDGE]")
            low_signal = False
            try:
                low_signal = ctx['is_low_signal'](pl, ctx['policies'])
            except Exception:
                low_signal = False
            if (not meaningful) or is_nudge or low_signal:
                return False
            # Increment per-peer counters
            did_inject = False
            for lbl in receiver_labels:
                ctx['handoffs_peer'][lbl] = int(ctx['handoffs_peer'].get(lbl, 0)) + 1
            for lbl in receiver_labels:
                cnt = ctx['handoffs_peer'][lbl]
                if (cnt % ctx['self_check_every']) == 0:
                    sc_index = cnt // ctx['self_check_every']
                    ctx['self_checks_done'][lbl] = sc_index
                    ctx['in_self_check']['v'] = True
                    try:
                        msg = ctx['self_check_text'].rstrip()
                        # Full system/project injection every K self-checks
                        lines = [msg]
                        try:
                            rules_path = (home/'rules'/('PEERA.md' if lbl=='PeerA' else 'PEERB.md')).as_posix()
                            lines.append(f"Rules: {rules_path}")
                            lines.append("Project: PROJECT.md")
                        except Exception:
                            pass
                        if sys_refresh_every > 0 and (sc_index % sys_refresh_every) == 0:
                            # Append PROJECT.md and SYSTEM full content when available
                            try:
                                proj_path = (Path.cwd()/"PROJECT.md")
                                if proj_path.exists():
                                    proj_txt = proj_path.read_text(encoding='utf-8', errors='replace')
                                    lines.append("\n--- PROJECT.md (full) ---\n" + proj_txt)
                            except Exception:
                                pass
                            try:
                                rules_txt = (home/'rules'/('PEERA.md' if lbl=='PeerA' else 'PEERB.md')).read_text(encoding='utf-8')
                            except Exception:
                                rules_txt = ''
                            if rules_txt:
                                lines.append("\n--- SYSTEM (full) ---\n" + rules_txt)
                        final = "\n".join(lines).strip()
                        ctx['send_handoff']('System', lbl, f"<FROM_SYSTEM>\n{final}\n</FROM_SYSTEM>\n")
                        log_ledger(home, {"from": "system", "kind": "self-check", "every": ctx['self_check_every'], "count": cnt, "peer": lbl})
                        ctx['request_por_refresh']("self-check", force=False)
                        did_inject = True
                    finally:
                        ctx['in_self_check']['v'] = False
            return did_inject
        except Exception:
            return False

    def send_handoff(sender_label: str, receiver_label: str, payload: str, require_mid: Optional[bool]=None, *, nudge_text: Optional[str]=None):
        # Backpressure: enqueue
        if ctx['inflight'][receiver_label] is not None:
            ctx['queued'][receiver_label].append({"sender": sender_label, "payload": payload})
            log_ledger(home, {"from": sender_label, "kind": "handoff-queued", "to": receiver_label, "chars": len(payload)})
            return
        # Drop empty body
        try:
            plain = _plain_text_without_tags_and_mid(payload)
            if not plain:
                log_ledger(home, {"from": sender_label, "kind": "handoff-drop", "to": receiver_label, "reason": "empty-body", "chars": len(payload)})
                return
        except Exception:
            pass
        # Schedule keepalive on progress lines
        try:
            if sender_label in ("PeerA","PeerB"):
                schedule_cb = ctx.get('schedule_keepalive')
                if schedule_cb:
                    schedule_cb(sender_label, payload)
        except Exception:
            pass
        # Inbound suffix
        def _suffix_for(receiver: str, sender: str) -> str:
            key = 'from_peer'
            if sender == 'User':
                key = 'from_user'
            elif sender == 'System':
                key = 'from_system'
            prof = ctx['profileA'] if receiver == 'PeerA' else ctx['profileB']
            cfg = (prof or {}).get('inbound_suffix', '')
            if isinstance(cfg, dict):
                return (cfg.get(key) or '').strip()
            if receiver == 'PeerA':
                return str(cfg).strip()
            if receiver == 'PeerB' and sender == 'User':
                return str(cfg).strip()
            return ''
        suf = _suffix_for(receiver_label, sender_label)
        if suf:
            payload = _append_suffix_inside(payload, suf)
        # Duplicate de-bounce
        h = hashlib.sha1(payload.encode('utf-8', errors='replace')).hexdigest()
        now = time.time()
        rs = [it for it in ctx['recent_sends'][receiver_label] if now - float(it.get('ts',0)) <= ctx['duplicate_window']]
        if any(it.get('hash') == h for it in rs):
            log_ledger(home, {"from": sender_label, "kind": "handoff-duplicate-drop", "to": receiver_label, "chars": len(payload)})
            return
        rs.append({"hash": h, "ts": now}); ctx['recent_sends'][receiver_label] = rs[-20:]
        # Inbox + NUDGE
        mid = ctx['new_mid']()
        text_with_mid = ctx['wrap_with_mid'](payload, mid)
        try:
            seq, _ = _write_inbox_message(home, receiver_label, text_with_mid, mid)
            if nudge_text and nudge_text.strip():
                if receiver_label == 'PeerA':
                    ctx['maybe_send_nudge'](home, 'PeerA', ctx['paneA'], ctx['profileA'], custom_text=nudge_text, force=True)
                else:
                    ctx['maybe_send_nudge'](home, 'PeerB', ctx['paneB'], ctx['profileB'], custom_text=nudge_text, force=True)
            else:
                ctx['send_nudge'](home, receiver_label, seq, mid, ctx['paneA'], ctx['paneB'], ctx['profileA'], ctx['profileB'], ctx['aux_mode'])
            try:
                ctx['last_nudge_ts'][receiver_label] = time.time()
            except Exception:
                pass
            status = "nudged"
        except Exception as e:
            status = f"failed:{e}"; seq = "000000"
        ctx['inflight'][receiver_label] = None
        log_ledger(home, {"from": sender_label, "kind": "handoff", "to": receiver_label, "status": status, "mid": mid, "seq": seq, "chars": len(payload)})
        try:
            import os
            if str(os.environ.get('CCCC_LOG_LEVEL','')).lower() == 'debug':
                print(f"[HANDOFF] {sender_label} → {receiver_label} ({len(payload)} chars, status={status}, seq={seq})")
        except Exception:
            pass

        # Self-check cadence + optional full system injection via shared helper
        try:
            meaningful = sender_label in ("User", "System", "PeerA", "PeerB")
            _maybe_selfcheck_multi([receiver_label], payload or "", meaningful)
        except Exception:
            pass

    return SimpleNamespace(send_handoff=send_handoff, maybe_selfcheck=_maybe_selfcheck_multi)
