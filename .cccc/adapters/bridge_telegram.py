#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Telegram Bridge (MVP)
- Network mode: gate by token/allowlist (long-poll getUpdates).
- Inbound: messages -> .cccc/mailbox/<peer>/inbox (numbered files with [MID]); supports a:/b:/both: routing.
- Outbound: read .cccc/state/outbox.jsonl (single source) and send concise summaries to chat(s).
"""
from __future__ import annotations
from pathlib import Path
from typing import Dict, Any, List, Tuple, Optional
import os, sys, time, json, re, threading, secrets
import urllib.request, urllib.parse
try:
    import fcntl  # POSIX lock for inbox sequencing
except Exception:
    fcntl = None  # type: ignore

ROOT = Path.cwd()
HOME = ROOT/".cccc"
# Ensure we can import modules from .cccc BEFORE importing from common.*
if str(HOME) not in sys.path:
    sys.path.insert(0, str(HOME))

try:
    from common.config import read_config as _read_config  # type: ignore
except Exception:
    _read_config = None

try:
    from common.status_format import format_status_for_im, format_help_for_im  # type: ignore
except Exception:
    format_status_for_im = None  # type: ignore
    format_help_for_im = None  # type: ignore

def read_yaml(p: Path) -> Dict[str, Any]:
    if _read_config is not None:
        try:
            return _read_config(p)
        except Exception:
            pass
    # Fallback parsers
    try:
        import yaml as _y
        return _y.safe_load(p.read_text(encoding='utf-8')) or {}
    except Exception:
        try:
            return json.loads(p.read_text(encoding='utf-8'))
        except Exception:
            return {}

def _now():
    import time
    return time.strftime('%Y-%m-%d %H:%M:%S')

def _acquire_singleton_lock(name: str = "telegram-bridge"):
    """Prevent multiple bridge instances from running concurrently (avoids duplicate replies).
    Returns an open file handle holding an exclusive lock for process lifetime.
    """
    lf_path = HOME/"state"/f"{name}.lock"
    lf_path.parent.mkdir(parents=True, exist_ok=True)
    f = open(lf_path, 'w')
    try:
        if fcntl is not None:
            fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        # record pid for diagnostics
        f.write(str(os.getpid()))
        f.flush()
    except Exception:
        # Another instance holds the lock
        try:
            print("[telegram_bridge] Another instance is already running. Exiting.")
            _append_log(HOME/"state"/"bridge-telegram.log", "[warn] duplicate instance detected; exiting")
        except Exception:
            pass
        sys.exit(0)
    return f

def _write_text(p: Path, s: str):
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(s, encoding='utf-8')

def _append_log(p: Path, line: str):
    p.parent.mkdir(parents=True, exist_ok=True)
    with p.open('a', encoding='utf-8') as f:
        f.write(f"{_now()} {line}\n")


def _enqueue_im_command(command: str, args: Dict[str, Any], *, source: str, chat_id: int,
                        wait_seconds: float = 1.5) -> Tuple[Optional[Dict[str, Any]], str]:
    request_id = f"{source}-{chat_id}-{int(time.time()*1000)}-{secrets.token_hex(4)}"
    payload = {
        'request_id': request_id,
        'command': command,
        'args': args,
        'source': source,
        'chat_id': chat_id,
        'ts': _now(),
    }
    queue_dir = HOME/"state"/"im_commands"
    processed_dir = queue_dir/"processed"
    try:
        queue_dir.mkdir(parents=True, exist_ok=True)
    except Exception:
        pass
    path = queue_dir/f"{request_id}.json"
    tmp = path.with_suffix('.tmp')
    try:
        tmp.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding='utf-8')
        tmp.replace(path)
    except Exception:
        return None, request_id
    deadline = time.time() + max(0.2, float(wait_seconds))
    result_path = processed_dir/f"{request_id}.result.json"
    while time.time() < deadline:
        if result_path.exists():
            try:
                data = json.loads(result_path.read_text(encoding='utf-8'))
                return data, request_id
            except Exception:
                break
        time.sleep(0.1)
    return None, request_id

def _mid() -> str:
    import uuid, time
    return f"tg-{int(time.time())}-{uuid.uuid4().hex[:6]}"

def _route_from_text(text: str, default_route: str):
    t = text.strip()
    # Support plain prefixes: a:/b:/both: (ASCII or fullwidth colon)
    m = re.match(r"^(a|b|both)[:：]\s*", t, re.I)
    if m:
        kind = m.group(1).lower()
        t = t[m.end():]
        if kind == 'a':
            return ['peerA'], t
        if kind == 'b':
            return ['peerB'], t
        return ['peerA','peerB'], t
    # Support slash commands (group privacy mode): /a …, /b …, /both …, with optional @BotName
    m2 = re.match(r"^/(a|b|both)(?:@\S+)?\s+", t, re.I)
    if m2:
        cmd = m2.group(1).lower()
        t = t[m2.end():]
        if cmd == 'a':
            return ['peerA'], t
        if cmd == 'b':
            return ['peerB'], t
        return ['peerA','peerB'], t
    # Support mention form: @BotName a: … or @BotName /a …
    m3 = re.match(r"^@\S+\s+(a|b|both)[:：]\s*", t, re.I)
    if m3:
        kind = m3.group(1).lower()
        t = t[m3.end():]
        if kind == 'a':
            return ['peerA'], t
        if kind == 'b':
            return ['peerB'], t
        return ['peerA','peerB'], t
    m4 = re.match(r"^@\S+\s+/(a|b|both)(?:@\S+)?\s+", t, re.I)
    if m4:
        cmd = m4.group(1).lower()
        t = t[m4.end():]
        if cmd == 'a':
            return ['peerA'], t
        if cmd == 'b':
            return ['peerB'], t
        return ['peerA','peerB'], t
    if default_route == 'a':
        return ['peerA'], t
    if default_route == 'b':
        return ['peerB'], t
    return ['peerA','peerB'], t

def _wrap_with_mid(payload: str, mid: str) -> str:
    """Insert [MID: …] after the first recognized opening tag if present;
    otherwise prefix at the top. Keeps wrappers as the first line for peers.
    Recognized tags: FROM_USER, FROM_PeerA, FROM_PeerB, TO_PEER, TO_USER, FROM_SYSTEM
    """
    marker = f"[MID: {mid}]"
    try:
        m = re.search(r"<(\s*(FROM_USER|FROM_PeerA|FROM_PeerB|TO_PEER|TO_USER|FROM_SYSTEM)\s*)>", payload, re.I)
        if m:
            start, end = m.span()
            head = payload[:end]
            tail = payload[end:]
            # Ensure single newline after the tag
            if not head.endswith("\n"):
                head = head + "\n"
            return head + marker + "\n" + tail.lstrip("\n")
        else:
            return marker + "\n" + payload
    except Exception:
        return marker + "\n" + payload

TAG_RE = re.compile(r"<\s*(FROM_USER|FROM_PeerA|FROM_PeerB|TO_PEER|TO_USER|FROM_SYSTEM)\s*>", re.I)
def _wrap_user_if_needed(body: str) -> str:
    """Ensure inbound payload is inside <FROM_USER> … when no known tags are present."""
    if TAG_RE.search(body or ''):
        return body
    b = (body or '').strip()
    return f"<FROM_USER>\n{b}\n</FROM_USER>\n" if b else b

def _ensure_dirs(home: Path, peer: str) -> Tuple[Path, Path, Path]:
    base = home/"mailbox"/peer
    inbox_dir = base/"inbox"
    proc_dir = base/"processed"
    state = home/"state"
    inbox_dir.mkdir(parents=True, exist_ok=True)
    proc_dir.mkdir(parents=True, exist_ok=True)
    state.mkdir(parents=True, exist_ok=True)
    return inbox_dir, proc_dir, state

def _next_seq(inbox: Path, processed: Path, state: Path, peer: str) -> str:
    lock_path = state/f"inbox-seq-{peer}.lock"
    counter_path = state/f"inbox-seq-{peer}.txt"
    def compute_from_fs() -> int:
        mx = 0
        for d in (inbox, processed):
            try:
                for f in d.iterdir():
                    n = f.name
                    if len(n) >= 6 and n[:6].isdigit():
                        mx = max(mx, int(n[:6]))
            except Exception:
                pass
        return mx + 1
    def compute() -> int:
        try:
            return int(counter_path.read_text().strip()) + 1
        except Exception:
            return compute_from_fs()
    if fcntl is not None:
        with open(lock_path, 'w') as lf:
            try:
                fcntl.flock(lf, fcntl.LOCK_EX)
            except Exception:
                pass
            val = compute()
            try:
                with open(counter_path, 'w') as cf:
                    cf.write(str(val))
            except Exception:
                pass
            try:
                fcntl.flock(lf, fcntl.LOCK_UN)
            except Exception:
                pass
        return f"{val:06d}"
    # Fallback without fcntl
    val = compute()
    try:
        counter_path.write_text(str(val))
    except Exception:
        pass
    return f"{val:06d}"

def _deliver_inbound(home: Path, routes: List[str], payload: str, mid: str):
    """Write numbered inbox files per peer to integrate with orchestrator NUDGE.
    Also write inbox.md as a last-resort for bridge mode users.
    """
    # Lazy preamble handled centrally in orchestrator; adapter no longer injects.
    st = {"PeerA": True, "PeerB": True}
    for peer in routes:
        inbox_dir, proc_dir, state = _ensure_dirs(home, peer)
        seq = _next_seq(inbox_dir, proc_dir, state, peer)
        fname = f"{seq}.{mid}.txt"
        final = payload
        _write_text(inbox_dir/fname, final)
        # Best-effort: also mirror to inbox.md for adapter users
        _write_text((home/"mailbox"/peer/"inbox.md"), final)

def _append_ledger(entry: Dict[str, Any]):
    try:
        entry = {"ts": _now(), **entry}
        lp = HOME/"state"/"ledger.jsonl"
        lp.parent.mkdir(parents=True, exist_ok=True)
        with lp.open('a', encoding='utf-8') as f:
            f.write(json.dumps(entry, ensure_ascii=False) + "\n")
    except Exception:
        pass

def _runtime_path() -> Path:
    # Unified runtime for all bridges
    return HOME/"state"/"bridge-runtime.json"

def load_runtime() -> Dict[str, Any]:
    p = _runtime_path()
    try:
        if p.exists():
            return json.loads(p.read_text(encoding='utf-8'))
    except Exception:
        pass
    # Back-compat: fall back to legacy telegram-runtime.json
    try:
        legacy = HOME/"state"/"telegram-runtime.json"
        if legacy.exists():
            return json.loads(legacy.read_text(encoding='utf-8'))
    except Exception:
        pass
    return {}

def save_runtime(obj: Dict[str, Any]):
    p = _runtime_path()
    try:
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps(obj, ensure_ascii=False, indent=2), encoding='utf-8')
    except Exception:
        pass

def _summarize(text: str, max_chars: int, max_lines: int = 8) -> str:
    """Summarize while preserving line breaks for readability.
    - Normalize newlines, trim trailing spaces
    - Collapse consecutive blank lines
    - Keep at most max_lines; then cap by max_chars
    """
    if not text:
        return ""
    t = text.replace("\r\n", "\n").replace("\r", "\n").replace("\t", "  ")
    lines = [ln.rstrip() for ln in t.split("\n")]
    # strip leading/trailing empty lines
    while lines and not lines[0].strip():
        lines.pop(0)
    while lines and not lines[-1].strip():
        lines.pop()
    # collapse multiple blank lines
    kept = []
    empty = 0
    for ln in lines:
        if not ln.strip():
            empty += 1
            if empty <= 1:
                kept.append("")
        else:
            empty = 0
            kept.append(ln)
    # limit lines
    kept = kept[:max_lines]
    out = "\n".join(kept).strip()
    if len(out) > max_chars:
        out = out[: max(0, max_chars - 1) ] + "…"
    return out

def _subs_path() -> Path:
    return HOME/"state"/"telegram-subs.json"

def load_subs() -> List[int]:
    p = _subs_path()
    try:
        if p.exists():
            arr = json.loads(p.read_text(encoding='utf-8'))
            out = []
            for x in arr:
                try:
                    out.append(int(x))
                except Exception:
                    pass
            return out
    except Exception:
        pass
    return []

def save_subs(items: List[int]):
    p = _subs_path()
    try:
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps(sorted(set(int(x) for x in items))), encoding='utf-8')
    except Exception:
        pass

def main():
    cfg = read_yaml(HOME/"settings"/"telegram.yaml")
    # Real network path: gate by token and allowlist; long-poll getUpdates; send concise summaries
    _acquire_singleton_lock("telegram-bridge")
    token_env = str(cfg.get('token_env') or 'TELEGRAM_BOT_TOKEN')
    # Token is injected by parent process into env[token_env]; do not consult other env by default here
    token = os.environ.get(token_env, '')
    def _coerce_allowlist(val) -> set:
        def to_int(x):
            try:
                return int(str(x).strip())
            except Exception:
                return None
        if isinstance(val, (list, tuple, set)):
            out = set()
            for x in val:
                v = to_int(x)
                if v is not None:
                    out.add(v)
            return out
        if isinstance(val, str):
            s = val.strip().strip('"\'')
            if not s:
                return set()
            # Try JSON-style list first
            if s.startswith('[') and s.endswith(']'):
                try:
                    arr = json.loads(s)
                    return _coerce_allowlist(arr)
                except Exception:
                    pass
            # Fallback: split by comma/whitespace and brackets
            s2 = s.strip('[]')
            parts = re.split(r"[\s,]+", s2)
            out = set()
            for p in parts:
                v = to_int(p)
                if v is not None:
                    out.add(v)
            return out
        return set()

    allow_raw = cfg.get('allow_chats') or []
    allow_cfg = _coerce_allowlist(allow_raw)
    subs = set(load_subs())
    allow = set(allow_cfg) | subs
    policy = str(cfg.get('autoregister') or 'off').lower()
    max_auto = int(cfg.get('max_auto_subs') or 3)
    discover = bool(cfg.get('discover_allowlist', False))
    if not token or (not allow and not discover and policy != 'open'):
        print("[telegram_bridge] Missing token or allowlist; set discover_allowlist or configure allow_chats.")
        sys.exit(1)

    def tg_api(method: str, params: Dict[str, Any], *, timeout: int = 35) -> Dict[str, Any]:
        base = f"https://api.telegram.org/bot{token}/{method}"
        # Use JSON consistently to avoid encoding issues with non-ASCII text
        data = json.dumps(params, ensure_ascii=False).encode('utf-8')
        req = urllib.request.Request(base, data=data, method='POST')
        req.add_header('Content-Type', 'application/json; charset=utf-8')
        req.add_header('Accept', 'application/json')
        try:
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                body = resp.read().decode('utf-8', errors='replace')
                return json.loads(body)
        except Exception as e:
            # Attach HTTP status and partial body when available for diagnostics
            http_status = None; err_text = ''
            try:
                import urllib.error as _ue
                if isinstance(e, _ue.HTTPError):
                    http_status = e.code
                    try:
                        err_text = e.read().decode('utf-8','ignore')[:300]
                    except Exception:
                        err_text = ''
            except Exception:
                pass
            _append_log(HOME/"state"/"bridge-telegram.log", f"[error] api {method}: {e} status={http_status} body={err_text}")
            out = {"ok": False, "error": str(e)}
            if http_status is not None:
                out["http_status"] = http_status
            if err_text:
                out["error_text"] = err_text
            return out

    def tg_poll(offset: int) -> Tuple[int, List[Dict[str, Any]]]:
        # Use POST for consistency
        res = tg_api('getUpdates', {
            'offset': offset,
            'timeout': 25,
            'allowed_updates': json.dumps(["message", "edited_message", "channel_post", "callback_query"])  # type: ignore
        }, timeout=35)
        updates = []
        new_offset = offset
        if res.get('ok') and isinstance(res.get('result'), list):
            for u in res['result']:
                try:
                    uid = int(u.get('update_id'))
                    new_offset = max(new_offset, uid + 1)
                    updates.append(u)
                except Exception:
                    pass
        return new_offset, updates

    def redact(s: str) -> str:
        pats = cfg.get('redact_patterns') or []
        out = s
        for p in pats:
            try:
                out = re.sub(p, '[REDACTED]', out)
            except Exception:
                continue
        return out

    def is_cmd(s: str, name: str) -> bool:
        return re.match(rf"^/{name}(?:@\S+)?(?:\s|$)", s.strip(), re.I) is not None

    outlog = HOME/"state"/"bridge-telegram.log"
    _append_log(outlog, "[net] bridge started")
    # Outbound watcher (send summaries when to_user changes; debounced per peer)
    debounce = int(cfg.get('debounce_seconds') or 3)
    max_chars = int(cfg.get('max_msg_chars') or 900)
    max_lines = int(cfg.get('max_msg_lines') or 8)
    peer_debounce = int(cfg.get('peer_debounce_seconds') or debounce)
    peer_max_chars = int(cfg.get('peer_message_max_chars') or 600)
    peer_max_lines = int(cfg.get('peer_message_max_lines') or 6)
    runtime = load_runtime()
    show_peers_default = bool(cfg.get('show_peer_messages', True))
    show_peers = bool(runtime.get('show_peer_messages', show_peers_default))

    # Routing policy
    routing = cfg.get('routing') or {}
    require_explicit = bool(routing.get('require_explicit', True))
    allow_prefix = bool(routing.get('allow_prefix', True))
    require_mention = bool(routing.get('require_mention', False))
    dm_conf = cfg.get('dm') or {}
    dm_route_default = str(dm_conf.get('route_default', 'both'))
    hints = cfg.get('hints') or {}
    hint_cooldown = int(hints.get('cooldown_seconds', 300))

    # Files policy
    files_conf = cfg.get('files') or {}
    files_enabled = bool(files_conf.get('enabled', True))
    max_mb = int(files_conf.get('max_mb', 16))
    max_bytes = max_mb * 1024 * 1024
    allowed_mime = [str(x) for x in (files_conf.get('allowed_mime') or [])]
    inbound_dir = Path(files_conf.get('inbound_dir') or HOME/"work"/"upload"/"inbound")
    outbound_dir = Path(files_conf.get('outbound_dir') or HOME/"work"/"upload"/"outbound")

    # Hint cooldown memory { (chat_id,user_id): ts }
    hint_last: Dict[Tuple[int,int], float] = {}

    def _mime_allowed(m: str) -> bool:
        if not allowed_mime:
            return True
        for pat in allowed_mime:
            if pat.endswith('/*'):
                if m.startswith(pat[:-1]):
                    return True
            if m.lower() == pat.lower():
                return True
        return False

    def _sanitize_name(name: str) -> str:
        name = re.sub(r"[^A-Za-z0-9_.\-]+", "_", name)
        return name[:120] or f"file_{int(time.time())}"

    def _save_file_from_telegram(file_id: str, orig_name: str, chat_id: int, mid: str) -> Tuple[Path, Dict[str,Any]]:
        meta: Dict[str,Any] = {}
        # getFile
        res = tg_api('getFile', {'file_id': file_id}, timeout=20)
        if not res.get('ok'):
            raise RuntimeError(f"getFile failed: {res}")
        file_path = (res.get('result') or {}).get('file_path')
        if not file_path:
            raise RuntimeError("file_path missing")
        url = f"https://api.telegram.org/file/bot{token}/{file_path}"
        # Prepare path
        day = time.strftime('%Y%m%d')
        safe = _sanitize_name(orig_name or os.path.basename(file_path))
        # Unify: inbound/<platform>/<chat_id>/<YYYYMMDD>
        out_dir = inbound_dir/"telegram"/str(chat_id)/day
        out_dir.mkdir(parents=True, exist_ok=True)
        out_path = out_dir/f"{mid}__{safe}"
        # Download
        with urllib.request.urlopen(url, timeout=60) as resp, open(out_path, 'wb') as f:
            data = resp.read()
            if len(data) > max_bytes:
                raise RuntimeError(f"file too large: {len(data)} bytes > {max_bytes}")
            f.write(data)
            meta['bytes'] = len(data)
        # Hash
        import hashlib
        h = hashlib.sha256()
        with open(out_path, 'rb') as f:
            while True:
                chunk = f.read(1024*64)
                if not chunk: break
                h.update(chunk)
        meta['sha256'] = h.hexdigest()
        meta['path'] = str(out_path)
        meta['name'] = safe
        return out_path, meta

    def _maybe_hint(chat_id: int, user_id: int):
        now = time.time()
        key = (chat_id, user_id)
        if now - float(hint_last.get(key, 0)) < hint_cooldown:
            return
        hint_last[key] = now
        tg_api('sendMessage', {'chat_id': chat_id, 'text': 'No route detected. Prefix with /a /b /both or a: b: both: to route.'}, timeout=15)
    # Per-peer rate tracking for outbound chat messages (keys: 'peerA'|'peerB')
    last_sent_ts = {"peerA": 0.0, "peerB": 0.0}
    last_seen = {"peerA": "", "peerB": ""}

    # Outbound mailbox baseline persistence removed; OutboxConsumer handles structured baselines

    # Delete-on-success semantics for outbound files; no persistent sent-cache needed


    def _preflight_msg(peer_key: str, msg: str, min_interval_s: float) -> tuple:
        """Return (ok, reason). Validate JSON-encodability and per-peer rate only.
        Size constraints are applied by summarization before this point.
        """
        try:
            _ = json.dumps({'m': msg})
        except Exception as e:
            return False, f'json-encode-failed: {e}'
        now = time.time(); last = float(last_sent_ts.get(peer_key, 0.0) or 0.0)
        if now - last < float(min_interval_s or 0.0):
            return False, f'rate-limited: interval<{min_interval_s}s'
        return True, ''

    def _compose_safe(prefix: str, body: str, cfg_limit: int, max_lines_cfg: int) -> str:
        # Telegram hard limit: 4096 chars per message
        HARD = 4096
        # Keep a small margin to account for prefixes/newlines
        margin = 32
        safe_max = max(0, min(int(cfg_limit or HARD), HARD - len(prefix) - margin))
        # First, summarize within safe window
        body_sum = _summarize(redact(body), safe_max, max_lines_cfg)
        msg = f"{prefix}\n{body_sum}" if prefix else body_sum
        if len(msg) > HARD:
            msg = msg[:HARD-1] + '…'
        return msg

    def _send_with_one_retry(chat_id: int, method_params: Dict[str, Any]) -> bool:
        res = tg_api('sendMessage', method_params, timeout=15)
        if bool(res.get('ok')):
            return True
        # One retry after a short pause
        try:
            time.sleep(1.0)
        except Exception:
            pass
        res2 = tg_api('sendMessage', method_params, timeout=15)
        return bool(res2.get('ok'))

    def send_summary(peer: str, text: str) -> bool:
        """Send to_user summary. Returns True iff at least one chat received the message."""
        label = "PeerA" if peer == 'peerA' else "PeerB"
        prefix = f"[{label}]"
        minint = float(str(cfg.get('to_user_min_interval_s') or cfg.get('peer_summary_min_interval_s') or 5))
        # Build safe message respecting Telegram hard cap
        msg = _compose_safe(prefix, text, int(cfg.get('max_msg_chars') or max_chars), int(cfg.get('max_msg_lines') or max_lines))
        ok, reason = _preflight_msg(peer, msg, minint)
        if not ok:
            _append_ledger({'kind':'bridge-outbox-blocked','route':'to_user','from': label, 'reason': reason})
            return False
        # Rebuild allowlist dynamically (config allowlist ∪ current subscriptions)
        dynamic_allow = set(allow_cfg) | set(load_subs())
        if not dynamic_allow:
            _append_ledger({'kind':'bridge-outbox-blocked','route':'to_user','from': label, 'reason': 'no-allowed-chats'})
            return False
        delivered = 0
        for chat_id in sorted(dynamic_allow):
            ok_send = _send_with_one_retry(chat_id, {'chat_id': chat_id, 'text': msg, 'disable_web_page_preview': True})
            if ok_send:
                delivered += 1
            else:
                _append_log(outlog, f"[error] to_user send chat={chat_id} err=failed-after-retry")
                _append_ledger({'kind':'bridge-outbox-error','route':'to_user','from':label,'chat':chat_id,'error':'failed-after-retry'})
        if delivered > 0:
            _append_log(outlog, f"[outbound] sent {label} {len(msg)} chars to {delivered} chats")
            _append_ledger({"kind":"bridge-outbound","to":"telegram","route":"to_user","from":label,"chars":len(msg),"chats":delivered})
            last_sent_ts[peer] = time.time()
        # Always commit cursor (do not block queue): even when 0 delivered, treat as handled
        return True

    def send_foreman(owner_peer: str, text: str) -> bool:
        # owner_peer in {'peerA','peerB','both'}
        if owner_peer == 'both':
            to_label = "PeerA,PeerB"
            bucket = 'peerA'  # use a single throttle bucket for Foreman
        else:
            to_label = "PeerA" if owner_peer == 'peerA' else "PeerB"
            bucket = owner_peer
        prefix = f"[FOREMAN→{to_label}]"
        minint = float(str(cfg.get('to_user_min_interval_s') or cfg.get('peer_summary_min_interval_s') or 5))
        msg = _compose_safe(prefix, text, int(cfg.get('max_msg_chars') or max_chars), int(cfg.get('max_msg_lines') or max_lines))
        ok, reason = _preflight_msg(bucket, msg, minint)
        if not ok:
            _append_ledger({'kind':'bridge-outbox-blocked','route':'foreman_to_user','from':'FOREMAN', 'reason': reason})
            return False
        dynamic_allow = set(allow_cfg) | set(load_subs())
        if not dynamic_allow:
            _append_ledger({'kind':'bridge-outbox-blocked','route':'foreman_to_user','from':'FOREMAN','reason':'no-allowed-chats'})
            return False
        delivered = 0
        for chat_id in sorted(dynamic_allow):
            ok_send = _send_with_one_retry(chat_id, {'chat_id': chat_id, 'text': msg, 'disable_web_page_preview': True})
            if ok_send:
                delivered += 1
            else:
                _append_log(outlog, f"[error] foreman send chat={chat_id} err=failed-after-retry")
                _append_ledger({'kind':'bridge-outbox-error','route':'foreman_to_user','from':'FOREMAN','chat':chat_id,'error':'failed-after-retry'})
        if delivered > 0:
            _append_log(outlog, f"[outbound] sent FOREMAN→{to_label} {len(msg)} chars to {delivered} chats")
            _append_ledger({"kind":"bridge-outbound","to":"telegram","route":"foreman_to_user","from":"FOREMAN","owner":to_label,"chars":len(msg),"chats":delivered})
            last_sent_ts[bucket] = time.time()
        return True

    def send_peer_summary(sender_peer: str, text: str) -> bool:
        label = "PeerA→PeerB" if sender_peer == 'peerA' else "PeerB→PeerA"
        from_label = "PeerA" if sender_peer == 'peerA' else "PeerB"
        to_label = "PeerB" if sender_peer == 'peerA' else "PeerA"
        prefix = f"[{label}]"
        minint = float(str(cfg.get('peer_summary_min_interval_s') or cfg.get('to_user_min_interval_s') or 5))
        msg = _compose_safe(prefix, text, int(cfg.get('peer_message_max_chars') or peer_max_chars), int(cfg.get('peer_message_max_lines') or peer_max_lines))
        ok, reason = _preflight_msg(sender_peer, msg, minint)
        if not ok:
            _append_ledger({'kind':'bridge-outbox-blocked','route':'to_peer','from': from_label, 'to': to_label, 'reason': reason})
            return False
        dynamic_allow = set(allow_cfg) | set(load_subs())
        if not dynamic_allow:
            _append_ledger({'kind':'bridge-outbox-blocked','route':'to_peer','from': from_label, 'to': to_label, 'reason': 'no-allowed-chats'})
            return False
        delivered = 0
        for chat_id in sorted(dynamic_allow):
            ok_send = _send_with_one_retry(chat_id, {'chat_id': chat_id, 'text': msg, 'disable_web_page_preview': True})
            if ok_send:
                delivered += 1
            else:
                _append_log(outlog, f"[error] to_peer_summary send chat={chat_id} err=failed-after-retry")
                _append_ledger({'kind':'bridge-outbox-error','route':'to_peer','from':from_label,'to':to_label,'chat':chat_id,'error':'failed-after-retry'})
        if delivered > 0:
            _append_log(outlog, f"[outbound] sent {label} {len(msg)} chars to {delivered} chats")
            _append_ledger({"kind":"bridge-outbound","to":"telegram","route":"to_peer","from":from_label,"to":to_label,"chars":len(msg),"chats":delivered})
            last_sent_ts['peerA' if sender_peer=='peerA' else 'peerB'] = time.time()
        # Always commit cursor: even when 0 delivered
        return True

    def _normalize_peer_label(raw: str, *, default: str = 'peerA') -> str:
        v = (raw or '').strip().lower()
        if v in ('peera', 'peer_a', 'a'):  # fast path
            return 'peerA'
        if v in ('peerb', 'peer_b', 'b'):
            return 'peerB'
        if 'peera' in v:
            return 'peerA'
        if 'peerb' in v:
            return 'peerB'
        return default

    def on_to_user(ev: Dict[str, Any]) -> bool:
        peer_key_raw = str(ev.get('peer') or '')
        peer_key = _normalize_peer_label(peer_key_raw, default='peerA')
        text = str(ev.get('text') or '')
        if not text:
            return True
        src = str(ev.get('from') or '').lower()
        if src == 'foreman':
            owner_raw = str(ev.get('owner') or peer_key_raw)
            owner = owner_raw.strip().lower()
            if owner in ('both','peerab','a,b'):
                return bool(send_foreman('both', text))
            return bool(send_foreman(_normalize_peer_label(owner_raw, default=peer_key), text))
        return bool(send_summary(peer_key, text))

    def on_to_peer_summary(ev: Dict[str, Any]) -> bool:
        nonlocal show_peers
        try:
            runtime_now = load_runtime()
            show_peers = bool(runtime_now.get('show_peer_messages', show_peers_default))
        except Exception:
            pass
        if not show_peers:
            return True
        text = str(ev.get('text') or '')
        if not text:
            return True
        sender_peer = _normalize_peer_label(str(ev.get('from') or ''), default='peerA')
        return bool(send_peer_summary(sender_peer, text))

    def watch_outputs():
        outbound_conf = cfg.get('outbound') or {}
        cursor_conf = outbound_conf.get('cursor') or {}
        start_mode = str(cursor_conf.get('start_mode') or 'tail')
        replay_last = int(cursor_conf.get('replay_last') or 0)
        try:
            from adapters.outbox_consumer import OutboxConsumer  # type: ignore
        except Exception as e:
            _append_ledger({'kind': 'error', 'where': 'telegram.outbox_consumer', 'error': f'import failed: {e}'})
            return
        try:
            poll_s = float(cfg.get('outbox_poll_seconds') or 2.0)
            oc = OutboxConsumer(HOME, seen_name='telegram', start_mode=start_mode, replay_last=replay_last, poll_seconds=poll_s)
            _append_ledger({'kind':'bridge-consumer-start','seen':'telegram','start_mode':start_mode,'replay_last':replay_last})
        except Exception as e:
            _append_ledger({'kind': 'error', 'where': 'telegram.outbox_consumer', 'error': str(e)})
            raise

        try:
            threading.Thread(target=lambda: oc.loop(on_to_user, on_to_peer_summary), daemon=True).start()
        except Exception as e:
            _append_ledger({'kind':'error','where':'telegram.outbox_consumer','error': str(e)})
            raise
        # Outbound files watcher state
        # Track attempts within this run only (avoid rapid duplicates if filesystem timestamps don't change)
        sent_files: Dict[str, float] = {}
        def _is_image(path: Path) -> bool:
            return path.suffix.lower() in ('.jpg','.jpeg','.png','.gif','.webp')
        def _send_file(peer: str, fp: Path, caption: str) -> bool:
            cap = f"[{ 'PeerA' if peer=='peerA' else 'PeerB' }]\n" + _summarize(redact(caption or ''), max_chars, max_lines)
            # Choose send method: sidecar override > dir/ext heuristic
            method = 'sendPhoto' if _is_image(fp) or fp.parent.name == 'photos' else 'sendDocument'
            any_fail = False
            try:
                sidecars = [fp.with_suffix(fp.suffix + '.sendas'), fp.with_name(fp.name + '.sendas')]
                for sc in sidecars:
                    if sc.exists():
                        try:
                            m = (sc.read_text(encoding='utf-8').strip() or '').lower()
                            if m == 'photo':
                                method = 'sendPhoto'
                            elif m == 'document':
                                method = 'sendDocument'
                        except Exception:
                            pass
                        break
            except Exception:
                pass
            for chat_id in allow:
                try:
                    with open(fp, 'rb') as f:
                        data = f.read()
                    # Use multipart/form-data via urllib is complex; rely on Telegram auto-download for MVP: send link not possible.
                    # For simplicity in MVP, fall back to sendDocument by URL is not allowed; so we will skip if too large to read.
                    # Here we implement minimal upload using `urllib.request` with manual boundary.
                    boundary = f"----cccc{int(time.time()*1000)}"
                    def _multipart(fields, files):
                        crlf = "\r\n"; lines=[]
                        for k,v in fields.items():
                            lines.append(f"--{boundary}")
                            lines.append(f"Content-Disposition: form-data; name=\"{k}\"")
                            lines.append("")
                            lines.append(str(v))
                        for k, (filename, content, mime) in files.items():
                            lines.append(f"--{boundary}")
                            lines.append(f"Content-Disposition: form-data; name=\"{k}\"; filename=\"{filename}\"")
                            lines.append(f"Content-Type: {mime}")
                            lines.append("")
                            lines.append(content)
                        lines.append(f"--{boundary}--")
                        body = b""
                        for part in lines:
                            if isinstance(part, bytes):
                                body += part + b"\r\n"
                            else:
                                body += part.encode('utf-8') + b"\r\n"
                        return body, boundary
                    api_url = f"https://api.telegram.org/bot{token}/{method}"
                    fields = { 'chat_id': chat_id, 'caption': cap }
                    import mimetypes
                    mt = mimetypes.guess_type(fp.name)[0] or ''
                    if method=='sendPhoto':
                        mime = mt if mt.startswith('image/') else 'image/jpeg'
                    else:
                        mime = mt or 'application/octet-stream'
                    files = { ('photo' if method=='sendPhoto' else 'document'): (fp.name, data, mime) }
                    body, bnd = _multipart(fields, files)
                    req = urllib.request.Request(api_url, data=body, method='POST')
                    req.add_header('Content-Type', f'multipart/form-data; boundary={bnd}')
                    with urllib.request.urlopen(req, timeout=60) as resp:
                        _ = resp.read()
                except Exception as e:
                    any_fail = True
                    try:
                        import urllib.error as _ue
                        if isinstance(e, _ue.HTTPError):
                            try:
                                detail = e.read().decode('utf-8','ignore')
                            except Exception:
                                detail = ''
                            _append_log(outlog, f"[error] outbound-file send {fp}: {e} {detail[:200]}")
                        else:
                            _append_log(outlog, f"[error] outbound-file send {fp}: {e}")
                    except Exception:
                        _append_log(outlog, f"[error] outbound-file send {fp}: {e}")
            _append_log(outlog, f"[outbound-file] {fp}")
            _append_ledger({"kind":"bridge-file-outbound","peer":peer,"path":str(fp)})
            # Delete file and sidecars only when all sends succeeded
            if not any_fail:
                # Minimal file ACK: write a sidecar with sent metadata before deleting the payload
                try:
                    import hashlib as _hl, datetime as _dt
                    sha = _hl.sha256(data).hexdigest() if 'data' in locals() else ''
                    sent_meta = {
                        "sent": True,
                        "ts": _dt.datetime.now(_dt.timezone.utc).astimezone().isoformat(),
                        "bytes": len(data) if 'data' in locals() else None,
                        "sha256": sha,
                        "method": method,
                        "peer": peer,
                        "name": fp.name,
                    }
                    (fp.with_suffix(fp.suffix + '.sent.json')).write_text(json.dumps(sent_meta, ensure_ascii=False, indent=2), encoding='utf-8')
                    _append_ledger({"kind":"bridge-file-sent","peer":peer,"path":str(fp),"bytes":sent_meta.get('bytes'),"sha256":sha})
                except Exception as _e:
                    _append_log(outlog, f"[warn] failed to write sent sidecar for {fp}: {_e}")
                try:
                    for side in (
                        fp.with_suffix(fp.suffix + '.caption.txt'),
                        fp.with_suffix(fp.suffix + '.sendas'),
                        fp.with_name(fp.name + '.sendas'),
                        fp.with_suffix(fp.suffix + '.meta.json'),
                    ):
                        try:
                            if side.exists():
                                side.unlink()
                        except Exception:
                            pass
                    fp.unlink()
                except Exception as de:
                    _append_log(outlog, f"[warn] failed to delete outbound file {fp}: {de}")
                return True
            return False
        
        # Optional reset on start: baseline|archive|clear
        # Prefer nested 'outbound.reset_on_start', fallback to top-level 'reset_on_start'. Default 'baseline'.
        reset_mode = str((outbound_conf.get('reset_on_start') or cfg.get('reset_on_start') or 'baseline')).lower()
        try:
            if reset_mode in ('archive','clear'):
                arch = HOME/'state'/'outbound-archive'; arch.mkdir(parents=True, exist_ok=True)
                # Clear/archive outbound files to avoid blasting residual uploads (single-level per peer)
                for peer in ('peerA','peerB'):
                    d = outbound_dir/peer
                    if not d.exists():
                        continue
                    for fp in sorted(d.glob('*')):
                        if fp.is_dir():
                            continue
                            nm = str(fp.name).lower()
                            if nm.endswith('.caption.txt') or nm.endswith('.sendas') or nm.endswith('.meta.json'):
                                continue
                            if reset_mode == 'archive':
                                import time as _t
                                ts = _t.strftime('%Y%m%d-%H%M%S')
                                dest_dir = arch/peer
                                dest_dir.mkdir(parents=True, exist_ok=True)
                                try:
                                    (dest_dir/f"{fp.name}-{ts}").write_bytes(fp.read_bytes())
                                except Exception:
                                    pass
                            try:
                                fp.unlink()
                            except Exception:
                                pass
                # After clearing, outbound directory is empty; no extra bookkeeping needed
        except Exception:
            pass
        # No mailbox baseline: OutboxConsumer ensures baseline for structured events

        def _read_caption(fp: Path) -> str:
            cap_fp = fp.with_suffix(fp.suffix + '.caption.txt')
            if cap_fp.exists():
                try:
                    return cap_fp.read_text(encoding='utf-8').strip()
                except Exception:
                    return ''
            return ''
        def _detect_route_from_caption(cap: str) -> Tuple[str, str]:
            t = (cap or '').lstrip()
            m = re.match(r"^(a:|b:|both:)\s*", t, re.I)
            if m:
                tag = m.group(1).lower(); body = t[m.end():]
                return ({'a:':'peerA','b:':'peerB','both:':'both'}[tag], body)
            return ('', cap)
        def _route_sidecar(fp: Path) -> str:
            for sc in (fp.with_suffix(fp.suffix + '.route'), fp.with_name(fp.name + '.route')):
                if sc.exists():
                    try:
                        val = (sc.read_text(encoding='utf-8').strip() or '').lower()
                        if val in ('a','peera','peera','peera'.lower(), 'peera'):
                            return 'peerA'
                        if val in ('b','peerb','peerb','peerb'.lower(), 'peerb'):
                            return 'peerB'
                        if val in ('both','all','ab','a+b'):
                            return 'both'
                    except Exception:
                        pass
            return ''
        def _iter_targets():
            # Flat-only scheme: scan outbound root directory for files
            if outbound_dir.exists():
                for f in sorted(outbound_dir.glob('*')):
                    if f.is_file():
                        yield f
        while True:
            now = time.time()
            # Peer↔Peer summaries now come from outbox (no file polling)
            # Outbound files (flat-only directory)
            try:
                for fp in _iter_targets():
                    if fp.is_dir():
                        continue
                    name=str(fp.name).lower()
                    if name.endswith('.caption.txt') or name.endswith('.sendas') or name.endswith('.meta.json') or name.endswith('.sent.json'):
                        continue
                    cap = _read_caption(fp)
                    route = _route_sidecar(fp)
                    if not route:
                        route, cap = _detect_route_from_caption(cap)
                    if not route:
                        route = 'both'
                    peer = 'peerA' if route=='peerA' else ('peerB' if route=='peerB' else 'peerA')
                    # Send and delete on success; on failure, keep file for retry
                    _send_file(peer, fp, cap)
            except Exception as e:
                _append_log(outlog, f"[error] watch_outbound: {e}")
            time.sleep(1.0)

    t_out = threading.Thread(target=watch_outputs, daemon=True)
    t_out.start()

    # Inbound poll loop
    offset_path = HOME/"state"/"telegram-offset.json"
    try:
        off = int(json.loads(offset_path.read_text()).get('offset', 0)) if offset_path.exists() else 0
    except Exception:
        off = 0
    default_route = str(cfg.get('default_route') or 'both')
    while True:
        off, updates = tg_poll(off)
        if updates:
            offset_path.parent.mkdir(parents=True, exist_ok=True)
            offset_path.write_text(json.dumps({"offset": off}), encoding='utf-8')
        for u in updates:
            # Handle inline button callbacks (currently unused; just ack)
            if u.get('callback_query'):
                cq = u['callback_query']
                try:
                    tg_api('answerCallbackQuery', {'callback_query_id': cq.get('id')}, timeout=10)
                except Exception:
                    pass
                continue
            msg = u.get('message') or u.get('edited_message') or u.get('channel_post') or {}
            chat = (msg.get('chat') or {})
            chat_id = int(chat.get('id', 0) or 0)
            chat_type = str(chat.get('type') or '')
            if chat_id not in allow:
                text = (msg.get('text') or '').strip()
                if policy == 'open' and is_cmd(text, 'subscribe'):
                    # Auto-register with cap
                    cur = set(load_subs())
                    if chat_id in cur:
                        tg_api('sendMessage', {'chat_id': chat_id, 'text': 'Already subscribed (allowlist)'}, timeout=15)
                    elif len(cur) >= max_auto:
                        tg_api('sendMessage', {'chat_id': chat_id, 'text': 'Subscription limit reached; contact admin.'}, timeout=15)
                    else:
                        cur.add(chat_id); save_subs(sorted(cur)); allow.add(chat_id)
                        tg_api('sendMessage', {'chat_id': chat_id, 'text': 'Subscribed. This chat will receive summaries. Send /unsubscribe to leave.'}, timeout=15)
                        _append_log(outlog, f"[subscribe] chat={chat_id}")
                        _append_ledger({"kind":"bridge-subscribe","chat":chat_id})
                    continue
                if policy == 'open' and is_cmd(text, 'unsubscribe'):
                    # Allow unsub from non-allowed (no-op) for idempotence
                    cur = set(load_subs()); removed = chat_id in cur
                    if removed:
                        cur.discard(chat_id); save_subs(sorted(cur))
                    tg_api('sendMessage', {'chat_id': chat_id, 'text': 'Unsubscribed' if removed else 'Not subscribed'}, timeout=15)
                    _append_log(outlog, f"[unsubscribe] chat={chat_id}")
                    _append_ledger({"kind":"bridge-unsubscribe","chat":chat_id})
                    continue
                # Discovery or closed policy: log and optionally reply to whoami/help
                _append_log(outlog, f"[drop] message from not-allowed chat={chat_id}")
                _append_ledger({"kind":"bridge-drop","reason":"not-allowed","chat":chat_id})
                if discover and is_cmd(text, 'whoami'):
                    tg_api('sendMessage', {'chat_id': chat_id, 'text': f"chat_id={chat_id} (not allowed; send /subscribe to opt-in)"}, timeout=15)
                elif is_cmd(text, 'help'):
                    help_txt = "You are not subscribed. Send /subscribe to opt-in first.\n/whoami shows your chat_id; /unsubscribe to leave."
                    tg_api('sendMessage', {'chat_id': chat_id, 'text': help_txt}, timeout=15)
                elif policy == 'open':
                    tg_api('sendMessage', {'chat_id': chat_id, 'text': 'Not subscribed. Send /subscribe to opt-in, /unsubscribe to leave.'}, timeout=15)
                continue
            text = (msg.get('text') or '').strip()
            caption = (msg.get('caption') or '').strip()
            is_dm = (chat_type == 'private')
            route_source = text or caption

            # Passthrough via a!/b! (DM recommended; in groups requires privacy off or @mention)
            if text and re.match(r'^\s*[abAB][!！]', text):
                m = re.match(r'^\s*([abAB])[!！]\s*(.*)$', text)
                cmd_body = m.group(2).strip() if m else ""
                if not cmd_body:
                    tg_api('sendMessage', {'chat_id': chat_id, 'text': 'Usage: a! <command> or b! <command>'}, timeout=15)
                else:
                    peer_key = 'a' if (m.group(1).lower() == 'a') else 'b'
                    result, req_id = _enqueue_im_command('passthrough', {'peer': peer_key, 'text': cmd_body}, source='telegram', chat_id=chat_id)
                    if result and result.get('ok'):
                        reply = result.get('message') or f'Command sent to peer {peer_key.upper()}.'
                    elif result:
                        reply = f"Command error: {result.get('message')}"
                    else:
                        reply = f"Command queued (id={req_id})."
                    tg_api('sendMessage', {'chat_id': chat_id, 'text': reply}, timeout=15)
                    _append_log(outlog, f"[cmd] passthrough peer={peer_key} chat={chat_id} req={req_id}")
                continue

            # Slash passthrough aliases for group-friendly usage: /pa /pb [/pboth]
            if is_cmd(text, 'pa') or is_cmd(text, 'pb') or is_cmd(text, 'pboth'):
                pieces = text.split(None, 1)
                body = pieces[1].strip() if len(pieces) > 1 else ''
                if not body:
                    tg_api('sendMessage', {'chat_id': chat_id, 'text': 'Usage: /pa <command> or /pb <command> (optional: /pboth <command>)'}, timeout=15)
                    continue
                peer_key = 'a' if is_cmd(text, 'pa') else ('b' if is_cmd(text, 'pb') else 'both')
                result, req_id = _enqueue_im_command('passthrough', {'peer': peer_key, 'text': body}, source='telegram', chat_id=chat_id)
                if result and result.get('ok'):
                    reply = result.get('message') or (f'Command sent to peer {peer_key.upper()}' if peer_key != 'both' else 'Command sent to both peers')
                elif result:
                    reply = f"Command error: {result.get('message')}"
                else:
                    reply = f"Command queued (id={req_id})."
                tg_api('sendMessage', {'chat_id': chat_id, 'text': reply}, timeout=15)
                _append_log(outlog, f"[cmd] passthrough peer={peer_key} chat={chat_id} req={req_id}")
                continue

            stripped = text.strip()
            if is_cmd(text, 'aux'):
                pieces = text.split(None, 1)
                prompt = pieces[1].strip() if len(pieces) > 1 else ''
                if not prompt:
                    tg_api('sendMessage', {'chat_id': chat_id, 'text': 'Usage: /aux <prompt>'}, timeout=15)
                    continue
                result, req_id = _enqueue_im_command('aux_cli', {'prompt': prompt}, source='telegram', chat_id=chat_id)
                if result and result.get('ok'):
                    reply = result.get('message') or 'Aux CLI executed.'
                elif result:
                    reply = result.get('message') or 'Aux CLI error.'
                else:
                    reply = f"Aux CLI request queued (id={req_id})."
                tg_api('sendMessage', {'chat_id': chat_id, 'text': reply[:3900]}, timeout=15)
                _append_log(outlog, f"[cmd] aux-cli chat={chat_id} req={req_id}")
                continue

            # Foreman control: /foreman on|off|now|status
            if is_cmd(text, 'foreman'):
                parts = text.split()
                action = parts[1].lower() if len(parts) > 1 else 'status'
                if action not in ('on','off','enable','disable','start','stop','status','now'):
                    tg_api('sendMessage', {'chat_id': chat_id, 'text': 'Usage: /foreman on|off|now|status'}, timeout=15)
                    continue
                # Fast path for status: read config/state directly for immediate feedback
                if action == 'status':
                    try:
                        fc_p = HOME/"settings"/"foreman.yaml"
                        st_p = HOME/"state"/"foreman.json"
                        fc = read_yaml(fc_p) if fc_p.exists() else {}
                        st = {}
                        try:
                            st = json.loads(st_p.read_text(encoding='utf-8')) if st_p.exists() else {}
                        except Exception:
                            st = {}
                        now = time.time()
                        def age(ts):
                            try:
                                tsf = float(ts or 0)
                                return f"{int(max(0, now - tsf))}s" if tsf else '-'
                            except Exception:
                                return '-'
                        allowed = bool(fc.get('allowed', fc.get('enabled', False)))
                        enabled = bool(fc.get('enabled', False))
                        running = bool(st.get('running', False))
                        next_due = st.get('next_due_ts') or 0
                        next_in = f"{int(max(0, next_due - now))}s" if next_due else '-'
                        last_rc = st.get('last_rc') if ('last_rc' in st) else '-'
                        last_out = st.get('last_out_dir') or '-'
                        reply = (
                            f"Foreman status: {'ON' if enabled else 'OFF'} allowed={'YES' if allowed else 'NO'} "
                            f"agent={fc.get('agent','reuse_aux')} interval={fc.get('interval_seconds','?')}s cc_user={'ON' if fc.get('cc_user',True) else 'OFF'}\n"
                            f"running={'YES' if running else 'NO'} next_in={next_in} last_start={age(st.get('last_start_ts'))} "
                            f"last_hb={age(st.get('last_heartbeat_ts'))} last_end={age(st.get('last_end_ts'))} last_rc={last_rc} out={last_out}"
                        )
                        tg_api('sendMessage', {'chat_id': chat_id, 'text': reply}, timeout=15)
                        _append_log(outlog, f"[cmd] foreman status (fast) chat={chat_id}")
                        continue
                    except Exception:
                        # Fallback to queued path
                        pass
                # on/off/now path: enqueue and wait briefly
                result, req_id = _enqueue_im_command('foreman', {'action': action}, source='telegram', chat_id=chat_id, wait_seconds=2.5)
                reply = (result.get('message') if result else f'Foreman request queued (id={req_id}).')
                tg_api('sendMessage', {'chat_id': chat_id, 'text': reply}, timeout=15)
                _append_log(outlog, f"[cmd] foreman action={action} chat={chat_id} req={req_id}")
                continue

            # Restart control: /restart peera|peerb|both
            if is_cmd(text, 'restart'):
                parts = text.split()
                target = parts[1].lower() if len(parts) > 1 else 'both'
                if target not in ('peera', 'peerb', 'both', 'a', 'b'):
                    tg_api('sendMessage', {'chat_id': chat_id, 'text': 'Usage: /restart peera|peerb|both'}, timeout=15)
                    continue
                result, req_id = _enqueue_im_command('restart', {'target': target}, source='telegram', chat_id=chat_id, wait_seconds=6.0)
                reply = (result.get('message') if result else f'Restart request queued (id={req_id}).')
                tg_api('sendMessage', {'chat_id': chat_id, 'text': reply}, timeout=15)
                _append_log(outlog, f"[cmd] restart target={target} chat={chat_id} req={req_id}")
                continue

            # Pause/Resume handoff delivery: /pause, /resume
            if is_cmd(text, 'pause'):
                result, req_id = _enqueue_im_command('pause', {}, source='telegram', chat_id=chat_id, wait_seconds=2.0)
                reply = (result.get('message') if result else 'Pause request queued.')
                tg_api('sendMessage', {'chat_id': chat_id, 'text': f"⏸ {reply}"}, timeout=15)
                _append_log(outlog, f"[cmd] pause chat={chat_id} req={req_id}")
                continue

            if is_cmd(text, 'resume'):
                result, req_id = _enqueue_im_command('resume', {}, source='telegram', chat_id=chat_id, wait_seconds=2.0)
                reply = (result.get('message') if result else 'Resume request queued.')
                tg_api('sendMessage', {'chat_id': chat_id, 'text': f"▶️ {reply}"}, timeout=15)
                _append_log(outlog, f"[cmd] resume chat={chat_id} req={req_id}")
                continue

            # Meta commands (must be before require_mention/require_explicit checks)
            if is_cmd(text, 'help'):
                if format_help_for_im:
                    help_txt = format_help_for_im('/')
                else:
                    help_txt = (
                        "/a /b /both - send to peers\n"
                        "/pause /resume - delivery control\n"
                        "/status - system status\n"
                        "/subscribe /unsubscribe - opt in/out"
                    )
                tg_api('sendMessage', {'chat_id': chat_id, 'text': help_txt}, timeout=15)
                continue

            if is_cmd(text, 'whoami'):
                tg_api('sendMessage', {'chat_id': chat_id, 'text': f"chat_id={chat_id}"}, timeout=15)
                _append_log(outlog, f"[meta] whoami chat={chat_id}")
                continue

            # Unsubscribe for already-subscribed users
            if is_cmd(text, 'unsubscribe'):
                cur = set(load_subs()); removed = chat_id in cur
                if removed:
                    cur.discard(chat_id); save_subs(sorted(cur)); allow.discard(chat_id)
                tg_api('sendMessage', {'chat_id': chat_id, 'text': 'Unsubscribed' if removed else 'Not subscribed'}, timeout=15)
                _append_log(outlog, f"[unsubscribe] chat={chat_id}{' (noop)' if not removed else ''}")
                _append_ledger({"kind":"bridge-unsubscribe","chat":chat_id,"removed":removed})
                continue

            # Status command (read-only, allowed without routing prefix)
            if is_cmd(text, 'status'):
                if format_status_for_im:
                    status_text = format_status_for_im(HOME / "state")
                else:
                    # Fallback if import failed
                    st_path = HOME/"state"/"status.json"
                    try:
                        st = json.loads(st_path.read_text(encoding='utf-8')) if st_path.exists() else {}
                    except Exception:
                        st = {}
                    paused = st.get('paused', False)
                    reset = st.get('reset') or {}
                    total = reset.get('handoffs_total', 0)
                    status_text = f"{'Paused' if paused else 'Running'} | handoffs: {total}"
                tg_api('sendMessage', {'chat_id': chat_id, 'text': status_text}, timeout=15)
                _append_log(outlog, f"[cmd] status chat={chat_id}")
                continue

            # Enforce mention in group if configured
            if (not is_dm) and require_mention:
                ents = msg.get('entities') or []
                mentions = any(e.get('type')=='mention' for e in ents)
                if not mentions:
                    _maybe_hint(chat_id, int((msg.get('from') or {}).get('id', 0) or 0))
                    continue
            # Enforce explicit routing for groups
            # FIX: Add word boundary \b to prevent /boht matching /b
            has_explicit = bool(re.match(r"^(?:/(?:a|b|both)\b(?:@\S+)?|(?:a[:：]|b[:：]|both[:：]))", (route_source or '').strip(), re.I))
            dr = dm_route_default if is_dm else default_route
            if (not is_dm) and require_explicit and not has_explicit and not (msg.get('document') or msg.get('photo')):
                _maybe_hint(chat_id, int((msg.get('from') or {}).get('id', 0) or 0))
                continue
            # Reply routing: if message contains only a route and replies to another message,
            # use the replied message's content/files.
            rmsg = msg.get('reply_to_message') or {}
            if rmsg and has_explicit and not (text.strip().split(maxsplit=1)[1:] if text else []) and not caption:
                rtext = (rmsg.get('text') or rmsg.get('caption') or '').strip()
                if rtext:
                    route_source = rtext
                if files_enabled and (rmsg.get('document') or rmsg.get('photo')):
                    metas = []
                    try:
                        if rmsg.get('document'):
                            doc = rmsg['document']
                            fn = doc.get('file_name') or 'document.bin'
                            mime = doc.get('mime_type') or 'application/octet-stream'
                            if _mime_allowed(mime):
                                midf = _mid()
                                path, meta = _save_file_from_telegram(doc.get('file_id'), fn, chat_id, midf)
                                meta.update({'mime': mime, 'caption': rtext, 'mid': midf}); metas.append(meta)
                        if rmsg.get('photo'):
                            ph = sorted(rmsg['photo'], key=lambda p: int(p.get('file_size') or 0))[-1]
                            fn = 'photo.jpg'; mime = 'image/jpeg'
                            midf = _mid(); path, meta = _save_file_from_telegram(ph.get('file_id'), fn, chat_id, midf)
                            meta.update({'mime': mime, 'caption': rtext, 'mid': midf}); metas.append(meta)
                    except Exception as e:
                        tg_api('sendMessage', {'chat_id': chat_id, 'text': f'Failed to receive quoted file: {e}'}, timeout=15)
                        _append_log(outlog, f"[error] inbound-file-reply: {e}")
                        metas = []
                    if metas:
                        routes, _ = _route_from_text(text or '/both', dr)
                        lines = ["<FROM_USER>", f"[MID: {_mid()}]"]
                        if rtext:
                            lines.append(f"Quoted: {redact(rtext)[:200]}")
                        for mta in metas:
                            rel = os.path.relpath(mta['path'], start=ROOT)
                            lines.append(f"File: {rel}")
                            lines.append(f"SHA256: {mta['sha256']}  Size: {mta['bytes']}  MIME: {mta['mime']}")
                            try:
                                side = Path(mta['path']).with_suffix(Path(mta['path']).suffix + '.meta.json')
                                side.write_text(json.dumps({
                                    'chat_id': chat_id,
                                    'path': rel,
                                    'sha256': mta['sha256'],
                                    'bytes': mta['bytes'],
                                    'mime': mta['mime'],
                                    'caption': rtext,
                                    'mid': mta.get('mid'),
                                    'ts': time.strftime('%Y-%m-%d %H:%M:%S')
                                }, ensure_ascii=False, indent=2), encoding='utf-8')
                            except Exception:
                                pass
                        # Append inbound index (reply case)
                        try:
                            idx = HOME/"state"/"inbound-index.jsonl"; idx.parent.mkdir(parents=True, exist_ok=True)
                            for mta in metas:
                                rec = {
                                    'ts': int(time.time()), 'path': mta['path'], 'platform': 'telegram',
                                    'routes': routes, 'mid': mta.get('mid'), 'mime': mta['mime'], 'bytes': mta['bytes'], 'sha256': mta['sha256']
                                }
                                with idx.open('a', encoding='utf-8') as f:
                                    f.write(json.dumps(rec, ensure_ascii=False) + "\n")
                        except Exception:
                            pass
                        lines.append("</FROM_USER>")
                        payload = "\n".join(lines) + "\n"
                        _deliver_inbound(HOME, routes, payload, _mid())
                        _append_log(outlog, f"[inbound-file-reply] routes={routes} files={len(metas)} chat={chat_id}")
                        _append_ledger({'kind': 'bridge-file-inbound', 'chat': chat_id, 'routes': routes,
                                        'files': [{'path': m['path'], 'sha256': m['sha256']} for m in metas]})
                        continue
# Inbound files
            if files_enabled and (msg.get('document') or msg.get('photo')):
                if (not is_dm) and require_explicit and not has_explicit:
                    _maybe_hint(chat_id, int((msg.get('from') or {}).get('id', 0) or 0))
                    continue
                metas = []
                try:
                    if msg.get('document'):
                        doc = msg['document']
                        fn = doc.get('file_name') or 'document.bin'
                        mime = doc.get('mime_type') or 'application/octet-stream'
                        if not _mime_allowed(mime):
                            tg_api('sendMessage', {'chat_id': chat_id, 'text': f'File type not allowed: {mime}'}, timeout=15)
                            continue
                        midf = _mid()
                        path, meta = _save_file_from_telegram(doc.get('file_id'), fn, chat_id, midf)
                        meta.update({'mime': mime, 'caption': caption, 'mid': midf})
                        metas.append(meta)
                    if msg.get('photo'):
                        ph = sorted(msg['photo'], key=lambda p: int(p.get('file_size') or 0))[-1]
                        fn = 'photo.jpg'
                        mime = 'image/jpeg'
                        midf = _mid()
                        path, meta = _save_file_from_telegram(ph.get('file_id'), fn, chat_id, midf)
                        meta.update({'mime': mime, 'caption': caption, 'mid': midf})
                        metas.append(meta)
                    # Build inbox payload
                    routes, _ = _route_from_text(route_source or '', dr)
                    lines = ["<FROM_USER>"]
                    lines.append(f"[MID: {_mid()}]")
                    if caption:
                        lines.append(f"Caption: {redact(caption)}")
                    for mta in metas:
                        rel = os.path.relpath(mta['path'], start=ROOT)
                        lines.append(f"File: {rel}")
                        lines.append(f"SHA256: {mta['sha256']}  Size: {mta['bytes']}  MIME: {mta['mime']}")
                        # write sidecar meta json
                        try:
                            side = Path(mta['path']).with_suffix(Path(mta['path']).suffix + '.meta.json')
                            side.write_text(json.dumps({
                                'chat_id': chat_id,
                                'path': rel,
                                'sha256': mta['sha256'],
                                'bytes': mta['bytes'],
                                'mime': mta['mime'],
                                'caption': caption,
                                'mid': mta.get('mid'),
                                'ts': time.strftime('%Y-%m-%d %H:%M:%S')
                            }, ensure_ascii=False, indent=2), encoding='utf-8')
                        except Exception:
                            pass
                    # Append inbound index (normal case)
                    try:
                        idx = HOME/"state"/"inbound-index.jsonl"; idx.parent.mkdir(parents=True, exist_ok=True)
                        for mta in metas:
                            rec = {
                                'ts': int(time.time()), 'path': mta['path'], 'platform': 'telegram',
                                'routes': routes, 'mid': mta.get('mid'), 'mime': mta['mime'], 'bytes': mta['bytes'], 'sha256': mta['sha256']
                            }
                            with idx.open('a', encoding='utf-8') as f:
                                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
                    except Exception:
                        pass
                    lines.append("</FROM_USER>")
                    payload = "\n".join(lines) + "\n"
                    _deliver_inbound(HOME, routes, payload, _mid())
                    _append_log(outlog, f"[inbound-file] routes={routes} files={len(metas)} chat={chat_id}")
                    _append_ledger({"kind":"bridge-file-inbound","chat":chat_id,"routes":routes,"files":[{"path":m['path'],"sha256":m['sha256'],"bytes":m['bytes'],"mime":m['mime']} for m in metas]})
                    continue
                except Exception as e:
                    tg_api('sendMessage', {'chat_id': chat_id, 'text': f'Failed to receive file: {e}'}, timeout=15)
                    _append_log(outlog, f"[error] inbound-file: {e}")
                    continue
            # minimal commands
            if is_cmd(text, 'subscribe'):
                if policy == 'open':
                    cur = set(load_subs()); added = chat_id not in cur
                    if added:
                        cur.add(chat_id); save_subs(sorted(cur)); allow.add(chat_id)
                    tg_api('sendMessage', {
                        'chat_id': chat_id,
                        'text': 'Subscribed. This chat will receive summaries. Send /unsubscribe to leave.' if added else 'Already subscribed (allowlist)'
                    }, timeout=15)
                    _append_log(outlog, f"[subscribe] chat={chat_id}{' (noop)' if not added else ''}")
                    _append_ledger({"kind":"bridge-subscribe","chat":chat_id,"noop": (not added)})
                else:
                    tg_api('sendMessage', {'chat_id': chat_id, 'text': 'Self-subscribe disabled; contact admin.'}, timeout=15)
                continue
            # Verbose toggle (alias for legacy showpeers): /verbose on|off
            if re.match(r"^\s*/?verbose\s+(on|off)\b", text.strip().lower()):
                val = text.strip().lower().endswith('on')
                # Update runtime show_peer_messages
                try:
                    rt_path = HOME/"state"/"bridge-runtime.json"; rt_path.parent.mkdir(parents=True, exist_ok=True)
                    cur = {}
                    if rt_path.exists():
                        cur = json.loads(rt_path.read_text(encoding='utf-8'))
                    cur['show_peer_messages'] = bool(val)
                    rt_path.write_text(json.dumps(cur, ensure_ascii=False, indent=2), encoding='utf-8')
                except Exception:
                    pass
                # Mirror to foreman cc_user
                try:
                    import yaml
                    fc_p = HOME/"settings"/"foreman.yaml"
                    if fc_p.exists():
                        fc = yaml.safe_load(fc_p.read_text(encoding='utf-8')) or {}
                    else:
                        fc = {}
                    fc.setdefault('enabled', False)
                    fc.setdefault('interval_seconds', 900)
                    fc.setdefault('agent', 'reuse_aux')
                    fc.setdefault('prompt_path', './FOREMAN_TASK.md')
                    fc['cc_user'] = bool(val)
                    fc_p.write_text(yaml.safe_dump(fc, allow_unicode=True, sort_keys=False), encoding='utf-8')
                except Exception:
                    pass
                tg_api('sendMessage', {'chat_id': chat_id, 'text': f"Verbose set to: {'ON' if val else 'OFF'} (peer summaries + Foreman CC)"}, timeout=15)
                continue
            # Default: route conversational text to peers via mailbox
            if route_source:
                routes, body = _route_from_text(route_source, dr)
                stripped = redact(body).strip()
                if stripped:
                    mid_val = _mid()
                    payload = _wrap_with_mid(_wrap_user_if_needed(stripped), mid_val)
                    _deliver_inbound(HOME, routes, payload, mid_val)
                    _append_log(outlog, f"[inbound-text] routes={routes} len={len(stripped)} chat={chat_id}")
                    _append_ledger({
                        'kind': 'bridge-inbound',
                        'platform': 'telegram',
                        'chat': chat_id,
                        'routes': routes,
                        'mid': mid_val,
                        'bytes': len(stripped.encode('utf-8', 'ignore'))
                    })
                else:
                    _maybe_hint(chat_id, int((msg.get('from') or {}).get('id', 0) or 0))
                continue
if __name__ == '__main__':
    main()
