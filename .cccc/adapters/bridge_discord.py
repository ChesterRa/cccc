#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Discord Bridge (MVP)
- Outbound: read outbox.jsonl and post to configured channels
- Inbound: on_message (optional) to route into mailbox via a:/b:/both:
"""
from __future__ import annotations
from pathlib import Path
from typing import Dict, Any, List, Tuple, Optional
import os, sys, json, time, re, threading, asyncio, hashlib, datetime, secrets
try:
    import fcntl  # type: ignore
except Exception:
    fcntl = None  # type: ignore

ROOT = Path.cwd(); HOME = ROOT/".cccc"
if str(HOME) not in sys.path: sys.path.insert(0, str(HOME))

try:
    from common.status_format import format_status_for_im, format_help_for_im  # type: ignore
except Exception:
    format_status_for_im = None  # type: ignore
    format_help_for_im = None  # type: ignore

def read_yaml(p: Path) -> Dict[str, Any]:
    # Back-compat shim to centralized config loader
    try:
        from common.config import read_config as _rc  # type: ignore
        return _rc(p)
    except Exception:
        try:
            import yaml  # type: ignore
            return yaml.safe_load(p.read_text(encoding='utf-8')) or {}
        except Exception:
            try:
                return json.loads(p.read_text(encoding='utf-8'))
            except Exception:
                return {}

def _now(): return time.strftime('%Y-%m-%d %H:%M:%S')
def _log(line: str):
    p = HOME/"state"/"bridge-discord.log"; p.parent.mkdir(parents=True, exist_ok=True)
    with p.open('a', encoding='utf-8') as f: f.write(f"{_now()} {line}\n")


def _enqueue_im_command(command: str, args: Dict[str, Any], *, source: str, channel: str,
                        wait_seconds: float = 1.5) -> Tuple[Optional[Dict[str, Any]], str]:
    request_id = f"{source}-{channel}-{int(time.time()*1000)}-{secrets.token_hex(4)}"
    payload = {
        'request_id': request_id,
        'command': command,
        'args': args,
        'source': source,
        'channel': channel,
        'ts': _now(),
    }
    queue_dir = HOME/"state"/"im_commands"
    processed_dir = queue_dir/"processed"
    try:
        queue_dir.mkdir(parents=True, exist_ok=True)
        processed_dir.mkdir(parents=True, exist_ok=True)
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

def _acquire_singleton_lock(name: str = "discord-bridge"):
    lf_path = HOME/"state"/f"{name}.lock"
    lf_path.parent.mkdir(parents=True, exist_ok=True)
    f = open(lf_path, 'w')
    try:
        if fcntl is not None:
            fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
        f.write(str(os.getpid()))
        f.flush()
    except Exception:
        try:
            _log("[warn] duplicate discord bridge instance detected; exiting")
        except Exception:
            pass
        sys.exit(0)
    return f

TAG_RE = re.compile(r"<\s*FROM_USER\s*>", re.I)
def _wrap_from_user(s: str) -> str:
    return s if TAG_RE.search(s or '') else f"<FROM_USER>\n{(s or '').strip()}\n</FROM_USER>\n"

def _route_from_text(text: str, default_route: str) -> Tuple[List[str], str]:
    t = (text or '').strip()
    # Strip leading mention (e.g., <@1234567890>) if present
    t = re.sub(r"^\s*<@!?\d+>\s+", "", t)
    # Support ASCII / fullwidth colon with explicit key capture: a:/b:/both:
    m = re.match(r"^(a|b|both)[:：]\s*", t, re.I)
    if m:
        kind = m.group(1).lower(); t = t[m.end():]
        if kind == 'a':
            return ['peerA'], t
        if kind == 'b':
            return ['peerB'], t
        return ['peerA','peerB'], t
    if default_route == 'a': return ['peerA'], t
    if default_route == 'b': return ['peerB'], t
    return ['peerA','peerB'], t

def _ensure_inbox(peer: str):
    base = HOME/"mailbox"/peer; inbox=base/"inbox"; proc=base/"processed"; state=HOME/"state"
    for d in (inbox, proc, state): d.mkdir(parents=True, exist_ok=True)
    return inbox, proc, state

def _next_seq(inbox: Path, proc: Path, state: Path, peer: str) -> str:
    counter = state/f"inbox-seq-{peer}.txt"
    try: val = int(counter.read_text().strip()) + 1
    except Exception:
        mx = 0
        for d in (inbox, proc):
            try:
                for f in d.iterdir():
                    n=f.name
                    if len(n)>=6 and n[:6].isdigit(): mx=max(mx,int(n[:6]))
            except Exception: pass
        val = mx+1
    try: counter.write_text(str(val))
    except Exception: pass
    return f"{val:06d}"

def _write_inbox(routes: List[str], text: str, mid: str):
    for peer in routes:
        inbox, proc, state = _ensure_inbox(peer)
        seq = _next_seq(inbox, proc, state, peer)
        (inbox/f"{seq}.{mid}.txt").write_text(_wrap_from_user(text), encoding='utf-8')
        (HOME/"mailbox"/peer/"inbox.md").write_text(_wrap_from_user(text), encoding='utf-8')

def _summarize(t: str, max_chars: int = 1500, max_lines: int = 12) -> str:
    if not t: return ''
    t = t.replace('\r\n','\n').replace('\r','\n')
    lines=[ln.rstrip() for ln in t.split('\n')]
    while lines and not lines[0].strip(): lines.pop(0)
    while lines and not lines[-1].strip(): lines.pop()
    kept=[]; empty=0
    for ln in lines:
        if not ln.strip(): empty+=1
        else: empty=0
        if empty<=1: kept.append(ln)
    kept = kept[:max_lines]
    out='\n'.join(kept).strip()
    return out if len(out)<=max_chars else out[:max_chars-1]+'…'

def _compose_safe(prefix: str, body: str, *, max_chars: int = 1500, max_lines: int = 12, hard_limit: int = 2000, margin: int = 32) -> str:
    """Compose a Discord-safe message under the 2000-char hard limit.
    - Summarize body to a safe window (hard_limit - prefix - margin)
    - Then clamp final message to hard_limit
    """
    safe_max = max(0, min(int(max_chars), hard_limit - len(prefix) - margin))
    body_sum = _summarize(str(body or ''), safe_max, max_lines)
    msg = f"{prefix}\n{body_sum}" if prefix else body_sum
    if len(msg) > hard_limit:
        msg = msg[:hard_limit-1] + '…'
    return msg

def _sha256_file(fp: Path) -> str:
    h = hashlib.sha256()
    with open(fp, 'rb') as f:
        for chunk in iter(lambda: f.read(1024*64), b''):
            h.update(chunk)
    return h.hexdigest()

def _today_dir(root: Path, sub: str) -> Path:
    dt = datetime.datetime.now().strftime('%Y%m%d')
    p = root/sub/dt
    p.mkdir(parents=True, exist_ok=True)
    return p

def main():
    _acquire_singleton_lock("discord-bridge")
    cfg = read_yaml(HOME/"settings"/"discord.yaml")
    token = os.environ.get(str(cfg.get('bot_token_env') or 'DISCORD_BOT_TOKEN')) or cfg.get('bot_token')
    chans_user = [int(x) for x in (cfg.get('channels') or {}).get('to_user', [])]
    chans_peer = [int(x) for x in (cfg.get('channels') or {}).get('to_peer_summary', [])]
    cursor_conf = (cfg.get('outbound') or {}).get('cursor', {})
    start_mode = str(cursor_conf.get('start_mode','tail'))
    replay_last = int(cursor_conf.get('replay_last',0))
    show_peers = bool(cfg.get('show_peer_messages', True))
    default_route = str(cfg.get('default_route','both')).lower() if cfg.get('default_route') else 'both'

    # Outbound consumer thread
    try:
        from adapters.outbox_consumer import OutboxConsumer  # type: ignore
    except Exception as e:
        _log(f"[error] OutboxConsumer import failed: {e}; exiting")
        sys.exit(1)
    oc = OutboxConsumer(HOME, seen_name='discord', start_mode=start_mode, replay_last=replay_last)
    # Dynamic subscriptions (channel IDs)
    SUBS_LOCK = threading.Lock()
    def _subs_path() -> Path:
        return HOME/"state"/"discord-subs.json"
    def load_subs() -> List[int]:
        p = _subs_path()
        try:
            if p.exists():
                arr = json.loads(p.read_text(encoding='utf-8')).get('channels') or []
                out=[]
                for x in arr:
                    try: out.append(int(x))
                    except Exception: pass
                return out
        except Exception:
            pass
        return []
    def save_subs(items: List[int]):
        p = _subs_path(); p.parent.mkdir(parents=True, exist_ok=True)
        try:
            p.write_text(json.dumps({'channels': list(dict.fromkeys(items))[-2000:]}, ensure_ascii=False, indent=2), encoding='utf-8')
        except Exception:
            pass
    SUBS = load_subs()

    send_queue: List[Tuple[int,str]] = []  # (channel_id, text)
    q_lock = threading.Lock()

    def enqueue(ch_list: List[int], text: str):
        with q_lock:
            for ch in ch_list:
                send_queue.append((ch, text))

    def on_to_user(ev: Dict[str,Any]) -> bool:
        p = str(ev.get('peer') or '').lower()
        src = str(ev.get('from') or '').lower()
        if src == 'foreman':
            if p in ('both','peerab','a,b'):
                prefix = "[FOREMAN→PeerA,PeerB]"
            else:
                label = 'PeerA' if 'peera' in p or p=='peera' else 'PeerB'
                prefix = f"[FOREMAN→{label}]"
        else:
            label = 'PeerA' if 'peera' in p or p=='peera' else 'PeerB'
            prefix = f"[{label}]"
        msg = _compose_safe(prefix, str(ev.get('text') or ''))
        with q_lock:
            with SUBS_LOCK:
                chs = list(dict.fromkeys((chans_user or []) + (SUBS or [])))
        if chs:
            enqueue(chs, msg)
            return True
        else:
            # Buffer until a channel subscribes
            try:
                PENDING_TO_USER
            except NameError:
                # define buffers if not present
                pass
            else:
                with threading.Lock():
                    PENDING_TO_USER.append(msg)
            return False

    def on_to_peer_summary(ev: Dict[str,Any]) -> bool:
        # Runtime override via shared bridge-runtime.json
        eff_show = show_peers
        try:
            rp = HOME/"state"/"bridge-runtime.json"
            if rp.exists():
                eff_show = bool((json.loads(rp.read_text(encoding='utf-8')) or {}).get('show_peer_messages', show_peers))
        except Exception:
            pass
        if not eff_show:
            return True
        frm = str(ev.get('from') or '')
        label = 'PeerA→PeerB' if frm in ('PeerA','peera','peera') else 'PeerB→PeerA'
        msg = _compose_safe(f"[{label}]", str(ev.get('text') or ''))
        with q_lock:
            with SUBS_LOCK:
                chs = list(dict.fromkeys((chans_peer or []) + (SUBS or [])))
        if chs:
            enqueue(chs, msg)
            return True
        return False

    t = threading.Thread(target=lambda: oc.loop(on_to_user, on_to_peer_summary), daemon=True)
    t.start()

    # Require token for any Discord operations
    if not token:
        _log("[error] DISCORD_BOT_TOKEN missing; exiting")
        sys.exit(1)

    # Live mode with discord.py
    try:
        import discord  # type: ignore
    except Exception as e:
        _log(f"[error] discord.py not installed: {e}; exiting")
        sys.exit(1)

    intents = discord.Intents.default()
    intents.message_content = True
    client = discord.Client(intents=intents)

    @client.event
    async def on_ready():
        _log(f"[ready] logged in as {client.user}")

    @client.event
    async def on_message(message: 'discord.Message'):
        try:
            if message.author == client.user:
                return
            text = message.content or ''
            low = (text or '').strip().lower()
            stripped = re.sub(r"^\s*<@!?\d+>\s+", "", text or '').strip()

            async def _send_reply(msg: str):
                try:
                    await message.channel.send(msg)
                except Exception:
                    pass

            if stripped and re.match(r'^[abAB][!！]', stripped):
                m = re.match(r'^([abAB])[!！]\s*(.*)$', stripped)
                cmd_body = m.group(2).strip() if m else ""
                if not cmd_body:
                    await _send_reply('Usage: a! <command> or b! <command>')
                else:
                    peer_key = 'a' if (m.group(1).lower() == 'a') else 'b'
                    result, req_id = _enqueue_im_command('passthrough', {'peer': peer_key, 'text': cmd_body}, source='discord', channel=str(message.channel.id))
                    if result and result.get('ok'):
                        reply = result.get('message') or f'Command sent to peer {peer_key.upper()}.'
                    elif result:
                        reply = f"Command error: {result.get('message')}"
                    else:
                        reply = f"Command queued (id={req_id})."
                    await _send_reply(reply)
                    _log(f"[cmd] passthrough peer={peer_key} ch={message.channel.id} req={req_id}")
                return

            # Pause/Resume handoff delivery: !pause, !resume
            if re.match(r'^!pause\b', stripped, re.I):
                result, req_id = _enqueue_im_command('pause', {}, source='discord', channel=str(message.channel.id), wait_seconds=2.0)
                reply = (result.get('message') if result else 'Pause request queued.')
                await _send_reply(f"⏸ {reply}")
                _log(f"[cmd] pause ch={message.channel.id} req={req_id}")
                return

            if re.match(r'^!resume\b', stripped, re.I):
                result, req_id = _enqueue_im_command('resume', {}, source='discord', channel=str(message.channel.id), wait_seconds=2.0)
                reply = (result.get('message') if result else 'Resume request queued.')
                await _send_reply(f"▶️ {reply}")
                _log(f"[cmd] resume ch={message.channel.id} req={req_id}")
                return

            # Help command: !help
            if re.match(r'^!help\b', stripped, re.I):
                if format_help_for_im:
                    help_txt = format_help_for_im('!')
                else:
                    help_txt = (
                        "!a !b !both - send to peers\n"
                        "!pause !resume - delivery control\n"
                        "!status - system status\n"
                        "!subscribe !unsubscribe - opt in/out"
                    )
                await _send_reply(help_txt)
                _log(f"[cmd] help ch={message.channel.id}")
                return

            # Status command: !status
            if re.match(r'^!status\b', stripped, re.I):
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
                await _send_reply(status_text)
                _log(f"[cmd] status ch={message.channel.id}")
                return

            if re.match(r'^!aux\b', stripped, re.I):
                parts = stripped.split(None, 1)
                prompt = parts[1].strip() if len(parts) > 1 else ''
                if not prompt:
                    await _send_reply('Usage: !aux <prompt>')
                    return
                result, req_id = _enqueue_im_command('aux_cli', {'prompt': prompt}, source='discord', channel=str(message.channel.id))
                if result and result.get('ok'):
                    reply = result.get('message') or 'Aux CLI executed.'
                elif result:
                    reply = result.get('message') or 'Aux CLI error.'
                else:
                    reply = f"Aux CLI request queued (id={req_id})."
                await _send_reply(reply[:3500])
                _log(f"[cmd] aux-cli ch={message.channel.id} req={req_id}")
                return

            if re.match(r'^!foreman\b', stripped, re.I):
                parts = stripped.split()
                action = parts[1].lower() if len(parts) > 1 else 'status'
                if action not in ('on','off','enable','disable','start','stop','status','now'):
                    await _send_reply('Usage: !foreman on|off|now|status')
                    return
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
                        await _send_reply(reply)
                        _log(f"[cmd] foreman status (fast) ch={message.channel.id}")
                        return
                    except Exception:
                        pass
                result, req_id = _enqueue_im_command('foreman', {'action': action}, source='discord', channel=str(message.channel.id))
                if result and result.get('ok'):
                    reply = result.get('message') or 'OK'
                elif result:
                    reply = result.get('message') or 'Foreman error'
                else:
                    reply = f"Foreman request queued (id={req_id})."
                await _send_reply(reply)
                _log(f"[cmd] foreman ch={message.channel.id} req={req_id} action={action}")
                return

            if re.match(r'^!restart\b', stripped, re.I):
                parts = stripped.split()
                target = parts[1].lower() if len(parts) > 1 else 'both'
                if target not in ('peera', 'peerb', 'both', 'a', 'b'):
                    await _send_reply('Usage: !restart peera|peerb|both')
                    return
                result, req_id = _enqueue_im_command('restart', {'target': target}, source='discord', channel=str(message.channel.id))
                if result and result.get('ok'):
                    reply = result.get('message') or 'OK'
                elif result:
                    reply = result.get('message') or 'Restart error'
                else:
                    reply = f"Restart request queued (id={req_id})."
                await _send_reply(reply)
                _log(f"[cmd] restart ch={message.channel.id} req={req_id} target={target}")
                return

            # Require routing prefixes to avoid forwarding general chatter (support fullwidth colon)
            has_prefix = bool(re.search(r"^\s*(a[:：]|b[:：]|both[:：])", stripped, re.I))
            if not has_prefix:
                if low not in ('!subscribe','!sub','!unsubscribe','!unsub','!verbose on','!verbose off') and not message.attachments:
                    # Drop chatter without explicit prefix; keep logs quiet in normal operation
                    return
            if low in ('!subscribe','!sub'):
                try:
                    with SUBS_LOCK:
                        if message.channel.id not in SUBS:
                            SUBS.append(message.channel.id); save_subs(SUBS)
                    await message.channel.send('Subscribed this channel for to_user/to_peer_summary.')
                except Exception:
                    pass
                # Flush pending to_user messages
                try:
                    # compute channels
                    with SUBS_LOCK:
                        chs = list(dict.fromkeys((chans_user or []) + (SUBS or [])))
                    try:
                        PENDING_TO_USER
                    except NameError:
                        pass
                    else:
                        while PENDING_TO_USER:
                            msg2 = PENDING_TO_USER.pop(0)
                            enqueue(chs, msg2)
                except Exception:
                    pass
                return
            if low in ('!verbose on','!verbose off'):
                val = (low.endswith('on'))
                rt_path = HOME/"state"/"bridge-runtime.json"; rt_path.parent.mkdir(parents=True, exist_ok=True)
                try:
                    cur = {}
                    if rt_path.exists():
                        cur = json.loads(rt_path.read_text(encoding='utf-8'))
                    cur['show_peer_messages'] = bool(val)
                    rt_path.write_text(json.dumps(cur, ensure_ascii=False, indent=2), encoding='utf-8')
                except Exception:
                    pass
                # Mirror verbose to Foreman cc_user in settings
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
                try:
                    await message.channel.send(f"Verbose set to: {'ON' if val else 'OFF'} (peer summaries + Foreman CC)")
                except Exception:
                    pass
                return
            if low in ('!unsubscribe','!unsub'):
                try:
                    with SUBS_LOCK:
                        SUBS2 = [x for x in SUBS if x != message.channel.id]
                        if len(SUBS2) != len(SUBS):
                            SUBS[:] = SUBS2; save_subs(SUBS)
                    await message.channel.send('Unsubscribed this channel.')
                except Exception:
                    pass
                return
            routes, body = _route_from_text(text, default_route)
            mid = f"dc-{int(time.time())}-{str(message.author.id)[-4:]}"
            # Save attachments if any
            try:
                files_cfg = (cfg.get('files') or {})
                inbound_root = Path(str(files_cfg.get('inbound_dir') or (HOME/"work"/"upload"/"inbound")))
                # Unified inbound layout: inbound/YYYYMMDD
                day = datetime.datetime.now().strftime('%Y%m%d')
                dest_dir = inbound_root/day
                dest_dir.mkdir(parents=True, exist_ok=True)
                refs = []
                # Only accept attachments if explicit routing prefix present in text
                for att in (message.attachments or []) if has_prefix else []:
                    safe = re.sub(r"[^A-Za-z0-9._-]", "_", att.filename or f"discord_{att.id}")
                    out = dest_dir/f"{mid}__{safe}"
                    try:
                        await att.save(out)
                        meta = {
                            'platform': 'discord', 'name': att.filename, 'bytes': out.stat().st_size,
                            'mime': att.content_type, 'sha256': _sha256_file(out), 'ts': int(time.time()), 'url_src': att.url,
                            'mid': mid,
                        }
                        with open(out.with_suffix(out.suffix+".meta.json"), 'w', encoding='utf-8') as mf:
                            json.dump(meta, mf, ensure_ascii=False, indent=2)
                        refs.append((out, meta))
                    except Exception as e:
                        _log(f"[error] save attachment failed: {e}")
                if refs:
                    extra = "\n".join([f"- {str(p)} ({m.get('mime','')},{m.get('bytes',0)} bytes)" for p,m in refs])
                    body = (body + ("\n\nFiles:\n" + extra if extra else "")).strip()
                    # Index inbound files with computed routes
                    try:
                        idx = HOME/"state"/"inbound-index.jsonl"; idx.parent.mkdir(parents=True, exist_ok=True)
                        for pth, mt in refs:
                            rec = { 'ts': int(time.time()), 'path': str(pth), 'platform': 'discord', **mt, 'routes': routes }
                            with idx.open('a', encoding='utf-8') as f:
                                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
                    except Exception:
                        pass
            except Exception:
                pass
            _write_inbox(routes, body, mid)
        except Exception as e:
            try:
                _log(f"[error] on_message: {e}")
            except Exception:
                pass

    async def sender_loop():
        # Ensure gateway and caches are ready before sending
        try:
            await client.wait_until_ready()
        except Exception:
            pass
        missing_warned: set[int] = set()
        while True:
            await asyncio.sleep(0.3)
            item=None
            with q_lock:
                if send_queue:
                    item = send_queue.pop(0)
            if item:
                ch_id, msg = item
                ch = client.get_channel(ch_id)
                try:
                    if not ch:
                        if ch_id not in missing_warned:
                            _log(f"[warn] send queue: channel not ready ({ch_id}); will retry shortly")
                            missing_warned.add(ch_id)
                        with q_lock:
                            send_queue.append((ch_id, msg))
                        continue
                    await ch.send(msg)
                except Exception as e:
                    _log(f"[error] send failed to {ch_id}: {e}")

    # Outbound files watcher
    async def _send_file(ch_id: int, fp: Path, caption: str):
        ch = client.get_channel(ch_id)
        if not ch:
            return
        try:
            await ch.send(content=_summarize(caption or '', 1500, 10), file=discord.File(str(fp)))
        except Exception as e:
            _log(f"[error] file send failed to {ch_id}: {e}")

    def watch_outbound_files():
        files_cfg = (cfg.get('files') or {})
        if not bool(files_cfg.get('enabled', True)):
            return
        out_root = Path(str(files_cfg.get('outbound_dir') or (HOME/"work"/"upload"/"outbound")))
        sent: Dict[str, float] = {}
        def _read_caption(fp: Path) -> str:
            cap = ''
            try:
                for sc in (fp.with_suffix(fp.suffix + '.caption.txt'), fp.with_name(fp.name + '.caption.txt')):
                    if sc.exists():
                        cap = sc.read_text(encoding='utf-8')
                        break
            except Exception:
                pass
            return cap
        def _detect_route_from_caption(cap: str) -> Tuple[str, str]:
            t = (cap or '').lstrip()
            m = re.match(r"^(a:|b:|both:)\s*", t, re.I)
            if m:
                tag = m.group(1).lower(); body = t[m.end():]
                return ({'a:':'peerA','b:':'peerB','both:':'both'}[tag], body)
            return ('', cap)
        def _route_sidecar(fp: Path) -> str:
            try:
                for sc in (fp.with_suffix(fp.suffix + '.route'), fp.with_name(fp.name + '.route')):
                    if sc.exists():
                        val = (sc.read_text(encoding='utf-8').strip() or '').lower()
                        if val in ('a','peera','peera','peera'.lower()):
                            return 'peerA'
                        if val in ('b','peerb','peerb','peerb'.lower()):
                            return 'peerB'
                        if val in ('both','all','ab','a+b'):
                            return 'both'
            except Exception:
                pass
            return ''
        def _iter_targets():
            # Flat-only scheme: scan outbound root directory for files
            if out_root.exists():
                for f in out_root.iterdir():
                    if f.is_file():
                        yield f
        while True:
            try:
                for f in _iter_targets():
                    if not f.is_file():
                        continue
                    nm = f.name.lower()
                    if nm.endswith('.sent.json') or nm.endswith('.meta.json') or nm.endswith('.caption.txt') or nm.endswith('.route'):
                        continue
                    # If a sent sidecar exists, skip and best-effort cleanup payload
                    if f.with_name(f.name + '.sent.json').exists():
                        try:
                            f.unlink()
                        except Exception:
                            pass
                        continue
                    key = str(f.resolve())
                    if key in sent and (time.time() - sent[key] < 3):
                        continue
                    cap0 = _read_caption(f)
                    route = _route_sidecar(f)
                    if not route:
                        route, cap0 = _detect_route_from_caption(cap0)
                    if not route:
                        route = 'both'
                    label = 'PeerA' if route=='peerA' else ('PeerB' if route=='peerB' else 'PeerA+PeerB')
                        # queue send for all configured + subscribed channels (to_user + to_peer_summary + SUBS)
                    with SUBS_LOCK:
                        all_chs = list(dict.fromkeys((chans_user or []) + (chans_peer or []) + (SUBS or [])))
                    for ch_id in all_chs:
                        try:
                            asyncio.run_coroutine_threadsafe(_send_file(ch_id, f, f"[{label}]\n" + cap0), client.loop)
                        except Exception as e:
                            _log(f"[error] schedule file send failed: {e}")
                    meta = {'platform':'discord','ts': int(time.time()), 'file': str(f.name)}
                    try:
                        with open(f.with_name(f.name + '.sent.json'), 'w', encoding='utf-8') as mf:
                            json.dump(meta, mf, ensure_ascii=False)
                    except Exception:
                        pass
                    # Delete payload and sidecars to avoid repeat sends
                    try:
                        for side in (
                            f.with_suffix(f.suffix + '.caption.txt'),
                            f.with_suffix(f.suffix + '.route'),
                            f.with_suffix(f.suffix + '.meta.json'),
                        ):
                            try:
                                if side.exists():
                                    side.unlink()
                            except Exception:
                                pass
                        f.unlink()
                    except Exception:
                        pass
                    sent[key] = time.time()
            except Exception:
                pass
            time.sleep(1.0)

    threading.Thread(target=watch_outbound_files, daemon=True).start()

    async def runner():
        await client.login(token)
        loop = asyncio.get_running_loop()
        loop.create_task(sender_loop())
        await client.connect(reconnect=True)

    asyncio.run(runner())

if __name__ == '__main__':
    main()
