#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Slack Bridge (MVP)
- Outbound: read .cccc/state/outbox.jsonl (single source) and post messages to configured channels
- Inbound: Socket Mode (optional) to accept messages and route to mailbox inbox with a:/b:/both: prefixes
"""
from __future__ import annotations
from pathlib import Path
from typing import Dict, Any, List, Tuple, Optional
import os, sys, json, time, re, threading, hashlib, datetime, urllib.request, urllib.parse, secrets
from urllib.error import HTTPError, URLError
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
    # Back-compat shim: delegate to common config reader
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

def _append_log(line: str):
    p = HOME/"state"/"bridge-slack.log"; p.parent.mkdir(parents=True, exist_ok=True)
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

def _acquire_singleton_lock(name: str = "slack-bridge"):
    """Prevent multiple slack bridge instances from running concurrently."""
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
            _append_log("[warn] duplicate slack bridge instance detected; exiting")
        except Exception:
            pass
        sys.exit(0)
    return f

def _route_from_text(text: str, default_route: str) -> Tuple[List[str], str]:
    t = (text or '').strip()
    # Support ASCII and fullwidth colon after explicit key: a:/b:/both:
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

def _ensure_inbox_dirs(peer: str) -> Tuple[Path, Path, Path]:
    base = HOME/"mailbox"/peer; inbox = base/"inbox"; proc = base/"processed"; state = HOME/"state"
    for d in (inbox, proc, state): d.mkdir(parents=True, exist_ok=True)
    return inbox, proc, state

def _next_seq(inbox: Path, proc: Path, state: Path, peer: str) -> str:
    lock = state/f"inbox-seq-{peer}.lock"; counter = state/f"inbox-seq-{peer}.txt"
    def compute() -> int:
        try: return int(counter.read_text().strip()) + 1
        except Exception:
            mx = 0
            for d in (inbox, proc):
                try:
                    for f in d.iterdir():
                        n=f.name
                        if len(n)>=6 and n[:6].isdigit(): mx=max(mx,int(n[:6]))
                except Exception: pass
            return mx+1
    try:
        import fcntl
        with open(lock,'w') as lf:
            try: fcntl.flock(lf, fcntl.LOCK_EX)
            except Exception: pass
            val = compute()
            try:
                with open(counter,'w') as cf: cf.write(str(val))
            except Exception: pass
            try: fcntl.flock(lf, fcntl.LOCK_UN)
            except Exception: pass
    except Exception:
        val = compute();
        try: counter.write_text(str(val))
        except Exception: pass
    return f"{val:06d}"

def _wrap_from_user(body: str) -> str:
    if re.search(r"<\s*FROM_USER\s*>", body or '', re.I): return body
    return f"<FROM_USER>\n{(body or '').strip()}\n</FROM_USER>\n"

def _write_inbox(routes: List[str], text: str, mid: str):
    for peer in routes:
        inbox, proc, state = _ensure_inbox_dirs(peer)
        seq = _next_seq(inbox, proc, state, peer)
        (inbox/f"{seq}.{mid}.txt").write_text(_wrap_from_user(text), encoding='utf-8')
        (HOME/"mailbox"/peer/"inbox.md").write_text(_wrap_from_user(text), encoding='utf-8')

def _summarize(text: str, max_chars: int = 1200, max_lines: int = 12) -> str:
    if not text: return ''
    t = text.replace('\r\n','\n').replace('\r','\n').replace('\t','  ')
    lines=[ln.rstrip() for ln in t.split('\n')]
    while lines and not lines[0].strip(): lines.pop(0)
    while lines and not lines[-1].strip(): lines.pop()
    kept=[]; empty=0
    for ln in lines:
        if not ln.strip(): empty+=1; 
        else: empty=0
        if empty<=1: kept.append(ln)
    kept = kept[:max_lines]
    out='\n'.join(kept).strip()
    return out if len(out)<=max_chars else out[:max_chars-1]+'…'

def _sha256_file(fp: Path) -> str:
    h = hashlib.sha256()
    with open(fp, 'rb') as f:
        for chunk in iter(lambda: f.read(1024*64), b''):
            h.update(chunk)
    return h.hexdigest()

def _today_dir(root: Path, sub: str) -> Path:
    dt = datetime.datetime.now().strftime('%Y%m%d')
    # root is expected to be the inbound_dir already (e.g., .cccc/work/upload/inbound)
    p = root/sub/dt
    p.mkdir(parents=True, exist_ok=True)
    return p

def main():
    _acquire_singleton_lock("slack-bridge")
    cfg = read_yaml(HOME/"settings"/"slack.yaml")
    app_token = os.environ.get(str(cfg.get('app_token_env') or 'SLACK_APP_TOKEN')) or cfg.get('app_token')
    bot_token = os.environ.get(str(cfg.get('bot_token_env') or 'SLACK_BOT_TOKEN')) or cfg.get('bot_token')
    channels_to_user = [str(x) for x in (cfg.get('channels') or {}).get('to_user', [])]
    channels_peer = [str(x) for x in (cfg.get('channels') or {}).get('to_peer_summary', [])]
    reset = str((cfg.get('outbound') or {}).get('reset_on_start', 'baseline'))
    show_peers = bool(cfg.get('show_peer_messages', True))
    default_route = str(cfg.get('default_route','both')).lower() if cfg.get('default_route') else 'both'

    # Require bot token for any Slack operations
    if not bot_token:
        _append_log("[error] SLACK_BOT_TOKEN missing; exiting")
        sys.exit(1)

    # Outbound consumer
    try:
        from adapters.outbox_consumer import OutboxConsumer  # type: ignore
    except Exception as e:
        _append_log(f"[error] OutboxConsumer import failed: {e}; exiting")
        sys.exit(1)
    oc = OutboxConsumer(HOME, seen_name='slack', start_mode=str((cfg.get('outbound') or {}).get('cursor',{}).get('start_mode','tail')), replay_last=int((cfg.get('outbound') or {}).get('cursor',{}).get('replay_last',0)))

    # Dynamic channel subscriptions persist under state; used to avoid editing YAML.
    SUBS_LOCK = threading.Lock()
    def _subs_path() -> Path:
        return HOME/"state"/"slack-subs.json"
    def load_subs() -> List[str]:
        p = _subs_path()
        try:
            if p.exists():
                arr = json.loads(p.read_text(encoding='utf-8')).get('channels') or []
                return [str(x) for x in arr]
        except Exception:
            pass
        return []
    def save_subs(items: List[str]):
        p = _subs_path(); p.parent.mkdir(parents=True, exist_ok=True)
        try:
            p.write_text(json.dumps({'channels': list(dict.fromkeys(items))[-2000:]}, ensure_ascii=False, indent=2), encoding='utf-8')
        except Exception:
            pass
    SUBS = load_subs()

    PENDING_LOCK = threading.Lock()
    PENDING_TO_USER: List[Dict[str, Any]] = []  # buffer when no channels yet

    def _flush_pending():
        with PENDING_LOCK:
            if not PENDING_TO_USER:
                return
            with SUBS_LOCK:
                chs = list(dict.fromkeys((channels_to_user or []) + (SUBS or [])))
            if not chs:
                return
            try:
                while PENDING_TO_USER:
                    ev = PENDING_TO_USER.pop(0)
                    msg = ev.get('msg','')
                    # Enqueue for sender loop
                    try:
                        SEND_QUEUE
                    except NameError:
                        # Fallback to direct send if sender loop not yet set up
                        send_text(chs, msg)
                    else:
                        with SEND_LOCK:
                            SEND_QUEUE.append(msg)
            except Exception as e:
                _append_log(f"[error] slack flush pending failed: {e}")

    def send_text(chs: List[str], text: str) -> bool:
        ok_any = False
        try:
            from slack_sdk import WebClient  # type: ignore
            cli = WebClient(token=bot_token)
            for ch in chs:
                try:
                    cli.chat_postMessage(channel=ch, text=text)
                    ok_any = True
                    time.sleep(0.3)
                except Exception as e:
                    _append_log(f"[error] slack post failed to {ch}: {e}")
        except Exception as e:
            _append_log(f"[error] slack post failed: {e}")
        return ok_any

    # Simple sender queue to tolerate brief readiness gaps (e.g., channels not yet subscribed)
    SEND_LOCK = threading.Lock()
    SEND_QUEUE: List[str] = []

    def _sender_loop():
        warned = False
        while True:
            item = None
            with SEND_LOCK:
                if SEND_QUEUE:
                    item = SEND_QUEUE.pop(0)
            if item is None:
                time.sleep(0.2); continue
            with SUBS_LOCK:
                chs = list(dict.fromkeys((channels_to_user or []) + (SUBS or [])))
            if not chs:
                if not warned:
                    _append_log("[warn] slack sender: no channels ready; will retry queued message")
                    warned = True
                with SEND_LOCK:
                    SEND_QUEUE.append(item)
                time.sleep(0.5)
                continue
            warned = False
            if not send_text(chs, item):
                _append_log("[warn] slack sender: post failed; will retry queued message")
                with SEND_LOCK:
                    SEND_QUEUE.append(item)
                time.sleep(0.5)

    threading.Thread(target=_sender_loop, daemon=True).start()

    def on_to_user(ev: Dict[str,Any]) -> bool:
        p = str(ev.get('peer') or '').lower()
        src = str(ev.get('from') or '').lower()
        if src == 'foreman':
            if p in ('both','peerab','a,b'):
                prefix = "[FOREMAN→PeerA,PeerB]\n"
            else:
                label = 'PeerA' if 'peera' in p or p=='peera' else 'PeerB'
                prefix = f"[FOREMAN→{label}]\n"
        else:
            label = 'PeerA' if 'peera' in p or p=='peera' else 'PeerB'
            prefix = f"[{label}]\n"
        msg = prefix + _summarize(str(ev.get('text') or ''))
        with SUBS_LOCK:
            chs = list(dict.fromkeys((channels_to_user or []) + (SUBS or [])))
        if chs:
            with SEND_LOCK:
                SEND_QUEUE.append(msg)
            return True
        else:
            # Buffer until first channel is available and warn for diagnostics
            _append_log("[warn] no slack channels configured/subscribed for to_user; buffering until subscribe or channels configured")
            with PENDING_LOCK:
                PENDING_TO_USER.append({'msg': msg})
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
        msg = f"[{label}]\n" + _summarize(str(ev.get('text') or ''))
        with SUBS_LOCK:
            chs = list(dict.fromkeys((channels_peer or []) + (SUBS or [])))
        if chs:
            send_text(chs, msg)
            return True
        return False

    th = threading.Thread(target=lambda: oc.loop(on_to_user, on_to_peer_summary), daemon=True)
    th.start()

    # Prepare WebClient (required for outbound; exit if slack_sdk missing)
    try:
        from slack_sdk.web import WebClient  # type: ignore
    except Exception as e:
        _append_log(f"[error] slack_sdk not installed: {e}; exiting")
        sys.exit(1)
    web = WebClient(token=bot_token)
    # Discover bot user id to ignore self-messages (prevent echo loops)
    BOT_USER_ID = ""
    try:
        auth = web.auth_test()
        BOT_USER_ID = str(auth.get('user_id') or '')
        _append_log(f"[info] slack bot user_id={BOT_USER_ID}")
    except Exception as e:
        _append_log(f"[warn] slack auth_test failed: {e}")
    client = None
    socket_mode_available = False
    if app_token:
        try:
            from slack_sdk.socket_mode import SocketModeClient  # type: ignore
            from slack_sdk.socket_mode.response import SocketModeResponse  # type: ignore
            client = SocketModeClient(app_token=app_token, web_client=web)
            socket_mode_available = True
        except Exception as e:
            _append_log(f"[warn] slack socket mode unavailable: {e}; inbound disabled")
    else:
        _append_log("[info] inbound disabled (no SLACK_APP_TOKEN)")

    def _download_slack_file(file_obj: Dict[str, Any], channel_id: Optional[str] = None) -> Optional[Tuple[Path, Dict[str, Any]]]:
        """Robustly download a Slack file with token auth, preserving Authorization
        across Slack-domain redirects, validating content-type, and streaming to disk.
        """
        try:
            # Prefer fresh url_private_download; fall back to url_private; refetch via files.info when missing
            url = file_obj.get('url_private_download') or file_obj.get('url_private')
            fid = str(file_obj.get('id') or '')
            if not url and fid:
                try:
                    info = web.files_info(file=fid)
                    file2 = (info or {}).get('file') or {}
                    url = file2.get('url_private_download') or file2.get('url_private')
                    # merge enriched fields
                    for k in ('mimetype','name','size'):
                        if not file_obj.get(k) and file2.get(k):
                            file_obj[k] = file2.get(k)
                except Exception as e:
                    _append_log(f"[warn] files_info failed for {fid}: {e}")
            if not url:
                return None
            name = file_obj.get('name') or f"slack_{int(time.time())}"
            mime = file_obj.get('mimetype') or ''
            size = int(file_obj.get('size') or 0)
            cfg_files = (cfg.get('files') or {})
            max_mb = int(cfg_files.get('max_mb', 16))
            if max_mb > 0 and size > max_mb * 1024 * 1024:
                _append_log(f"[inbound] skip large file {name} {size} bytes > {max_mb} MB")
                return None
            inbound_root = Path(str(cfg_files.get('inbound_dir') or (HOME/"work"/"upload"/"inbound")))
            # Unified inbound layout: inbound/YYYYMMDD
            day = datetime.datetime.now().strftime('%Y%m%d')
            dest_dir = inbound_root/day
            dest_dir.mkdir(parents=True, exist_ok=True)
            safe = re.sub(r"[^A-Za-z0-9._-]", "_", name)
            mid = f"slack-{int(time.time())}"
            out = dest_dir/f"{mid}__{safe}"

            # Custom redirect handler that preserves Authorization for Slack-owned hosts only
            class _AuthRedirect(urllib.request.HTTPRedirectHandler):
                def redirect_request(self, req, fp, code, msg, headers, newurl):  # type: ignore[override]
                    new_req = super().redirect_request(req, fp, code, msg, headers, newurl)
                    try:
                        if new_req is None:
                            return None
                        auth = req.headers.get('Authorization')
                        if auth:
                            try:
                                o = urllib.parse.urlparse(req.full_url)
                                n = urllib.parse.urlparse(newurl)
                                # Only forward token to Slack-owned domains
                                def _is_slack_host(netloc: str) -> bool:
                                    h = (netloc or '').lower()
                                    return h.endswith('slack.com') or h.endswith('slack-edge.com') or h.endswith('files.slack.com')
                                if _is_slack_host(o.netloc) and _is_slack_host(n.netloc):
                                    new_req.add_unredirected_header('Authorization', auth)
                            except Exception:
                                pass
                        # Preserve Accept / UA for subsequent hops
                        if 'Accept' in req.headers:
                            new_req.add_unredirected_header('Accept', req.headers['Accept'])
                        if 'User-Agent' in req.headers:
                            new_req.add_unredirected_header('User-Agent', req.headers['User-Agent'])
                    except Exception:
                        pass
                    return new_req

            opener = urllib.request.build_opener(_AuthRedirect())
            headers = {
                "Authorization": f"Bearer {bot_token}",
                "Accept": "application/octet-stream, */*",
                "User-Agent": "cccc-slack-bridge/0.2.9"
            }

            def _fetch_to(out_path: Path, src_url: str) -> Tuple[bool, str, int, Optional[str]]:
                try:
                    req = urllib.request.Request(src_url, headers=headers)
                    with opener.open(req, timeout=120) as r:
                        ctype = (r.headers.get('Content-Type') or '').lower()
                        clen = r.headers.get('Content-Length')
                        exp = int(clen) if clen and clen.isdigit() else -1
                        # Reject obvious error bodies
                        if 'application/json' in ctype or 'text/html' in ctype:
                            blob = r.read(512)
                            try:
                                preview = blob.decode('utf-8','ignore')
                            except Exception:
                                preview = str(blob[:80])
                            return False, ctype, 0, preview
                        written = 0
                        with open(out_path, 'wb') as f:
                            while True:
                                chunk = r.read(1024*256)
                                if not chunk:
                                    break
                                f.write(chunk)
                                written += len(chunk)
                        if exp > 0 and written != exp:
                            return False, ctype, written, f"length_mismatch expected={exp} got={written}"
                        return True, ctype, written, None
                except HTTPError as e:
                    return False, f"http_error:{e.code}", 0, str(e)
                except URLError as e:
                    return False, "url_error", 0, str(e)
                except Exception as e:
                    return False, "exception", 0, str(e)

            ok, ctype, bytes_written, err = _fetch_to(out, url)
            if not ok and fid:
                # One retry via fresh files.info (URL may rotate)
                try:
                    info = web.files_info(file=fid)
                    file2 = (info or {}).get('file') or {}
                    retry_url = file2.get('url_private_download') or file2.get('url_private') or url
                    ok, ctype, bytes_written, err = _fetch_to(out, retry_url)
                except Exception as e:
                    _append_log(f"[warn] retry files_info failed for {fid}: {e}")

            if not ok:
                _append_log(f"[error] slack download failed name={name} url={url} ctype={ctype} err={err}")
                try:
                    if out.exists():
                        out.unlink()
                except Exception:
                    pass
                return None

            meta = {
                'platform': 'slack', 'name': name, 'bytes': bytes_written,
                'mime': mime or ctype, 'sha256': _sha256_file(out), 'ts': int(time.time()), 'url_src': url,
                'mid': mid,
            }
            try:
                with open(out.with_suffix(out.suffix+".meta.json"), 'w', encoding='utf-8') as mf:
                    json.dump(meta, mf, ensure_ascii=False, indent=2)
            except Exception:
                pass
            return out, meta
        except Exception as e:
            _append_log(f"[error] download file failed: {e}")
            return None

    def _append_inbound_index(p: Path, meta: Dict[str, Any], routes: Optional[list] = None):
        try:
            rec = {
                'ts': int(time.time()),
                'path': str(p),
                'platform': 'slack',
                **({} if not meta else meta)
            }
            if routes:
                rec['routes'] = routes
            idx = HOME/"state"/"inbound-index.jsonl"; idx.parent.mkdir(parents=True, exist_ok=True)
            with idx.open('a', encoding='utf-8') as f:
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
        except Exception:
            pass

    def handle(*args, **kwargs):
        try:
            # Accept both signatures: (req) or (client, req)
            _client = client
            if len(args) == 1:
                req = args[0]
            elif len(args) >= 2:
                _client = args[0]
                req = args[1]
            else:
                return
            # req: Slack SocketModeRequest
            typ = str(getattr(req, 'type', '') or '')
            if typ != 'events_api':
                return
            payload = getattr(req, 'payload', None) or {}
            event = (payload or {}).get('event') or {}
            # Ack early to avoid timeouts
            try:
                try:
                    from slack_sdk.socket_mode.response import SocketModeResponse  # type: ignore
                    _client.send_socket_mode_response(SocketModeResponse(envelope_id=getattr(req, 'envelope_id', '')))  # type: ignore
                except Exception:
                    pass
            except Exception:
                pass
            etype = str(event.get('type') or '')
            if etype == 'file_shared':
                # Enforce explicit routing: ignore bare file_shared without text
                _append_log("[inbound] drop file_shared without text (require a:/b:/both: with attachments)")
                return
            if etype != 'message':
                return
            sub = str(event.get('subtype') or '')
            # Allow subtype=file_share (channel message with files). Skip other subtypes (edits, bot_message, etc.)
            if sub and sub.lower() != 'file_share':
                return
            text = str(event.get('text') or '')
            ch = str(event.get('channel') or '')
            user = str(event.get('user') or '')
            # Strip leading self-mention once for routing parse
            stripped = text
            if BOT_USER_ID:
                stripped = re.sub(rf"^\s*<@{re.escape(BOT_USER_ID)}>\s+", "", stripped)
            command_text = stripped.strip()

            def _send_reply(msg: str):
                try:
                    web.chat_postMessage(channel=ch, text=msg)
                except Exception:
                    pass

            if command_text and re.match(r'^[abAB][!！]', command_text):
                m = re.match(r'^([abAB])[!！]\s*(.*)$', command_text)
                cmd_body = m.group(2).strip() if m else ""
                if not cmd_body:
                    _send_reply('Usage: a! <command> or b! <command>')
                else:
                    peer_key = 'a' if (m.group(1).lower() == 'a') else 'b'
                    result, req_id = _enqueue_im_command('passthrough', {'peer': peer_key, 'text': cmd_body}, source='slack', channel=ch)
                    if result and result.get('ok'):
                        reply = result.get('message') or f'Command sent to peer {peer_key.upper()}.'
                    elif result:
                        reply = f"Command error: {result.get('message')}"
                    else:
                        reply = f"Command queued (id={req_id})."
                    _send_reply(reply)
                    _append_log(f"[cmd] passthrough peer={peer_key} ch={ch} req={req_id}")
                return

            # Pause/Resume handoff delivery: !pause, !resume
            if re.match(r'^!pause\b', command_text, re.I):
                result, req_id = _enqueue_im_command('pause', {}, source='slack', channel=ch, wait_seconds=2.0)
                reply = (result.get('message') if result else 'Pause request queued.')
                _send_reply(f"⏸ {reply}")
                _append_log(f"[cmd] pause ch={ch} req={req_id}")
                return

            if re.match(r'^!resume\b', command_text, re.I):
                result, req_id = _enqueue_im_command('resume', {}, source='slack', channel=ch, wait_seconds=2.0)
                reply = (result.get('message') if result else 'Resume request queued.')
                _send_reply(f"▶️ {reply}")
                _append_log(f"[cmd] resume ch={ch} req={req_id}")
                return

            # Help command: !help
            if re.match(r'^!help\b', command_text, re.I):
                if format_help_for_im:
                    help_txt = format_help_for_im('!')
                else:
                    help_txt = (
                        "!a !b !both - send to peers\n"
                        "!pause !resume - delivery control\n"
                        "!status - system status\n"
                        "!subscribe !unsubscribe - opt in/out"
                    )
                _send_reply(help_txt)
                _append_log(f"[cmd] help ch={ch}")
                return

            # Status command: !status
            if re.match(r'^!status\b', command_text, re.I):
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
                _send_reply(status_text)
                _append_log(f"[cmd] status ch={ch}")
                return

            if re.match(r'^!aux\b', command_text, re.I):
                pieces = command_text.split(None, 1)
                prompt = pieces[1].strip() if len(pieces) > 1 else ''
                if not prompt:
                    _send_reply('Usage: !aux <prompt>')
                    return
                result, req_id = _enqueue_im_command('aux_cli', {'prompt': prompt}, source='slack', channel=ch)
                if result and result.get('ok'):
                    reply = result.get('message') or 'Aux CLI executed.'
                elif result:
                    reply = result.get('message') or 'Aux CLI error.'
                else:
                    reply = f"Aux CLI request queued (id={req_id})."
                _send_reply(reply[:3500])
                _append_log(f"[cmd] aux-cli ch={ch} req={req_id}")
                return

            if re.match(r'^!foreman\b', command_text, re.I):
                parts = command_text.split()
                action = parts[1].lower() if len(parts) > 1 else 'status'
                if action not in ('on','off','enable','disable','start','stop','status','now'):
                    _send_reply('Usage: !foreman on|off|now|status')
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
                        _send_reply(reply)
                        _append_log(f"[cmd] foreman status (fast) ch={ch}")
                        return
                    except Exception:
                        pass
                result, req_id = _enqueue_im_command('foreman', {'action': action}, source='slack', channel=ch)
                if result and result.get('ok'):
                    reply = result.get('message') or 'OK'
                elif result:
                    reply = result.get('message') or 'Foreman error'
                else:
                    reply = f"Foreman request queued (id={req_id})."
                _send_reply(reply)
                _append_log(f"[cmd] foreman ch={ch} req={req_id} action={action}")
                return

            if re.match(r'^!restart\b', command_text, re.I):
                parts = command_text.split()
                target = parts[1].lower() if len(parts) > 1 else 'both'
                if target not in ('peera', 'peerb', 'both', 'a', 'b'):
                    _send_reply('Usage: !restart peera|peerb|both')
                    return
                result, req_id = _enqueue_im_command('restart', {'target': target}, source='slack', channel=ch)
                if result and result.get('ok'):
                    reply = result.get('message') or 'OK'
                elif result:
                    reply = result.get('message') or 'Restart error'
                else:
                    reply = f"Restart request queued (id={req_id})."
                _send_reply(reply)
                _append_log(f"[cmd] restart ch={ch} req={req_id} target={target}")
                return

            # Routing prefixes only; ignore general chatter without explicit route (support fullwidth colon)
            has_prefix = bool(re.search(r"^\s*(a|b|both)[:：]", stripped, re.I))
            # Ignore self/bot messages to avoid echo loops
            if event.get('bot_id') or (BOT_USER_ID and user == BOT_USER_ID):
                return
            # Subscribe/Unsubscribe commands (with ! prefix for consistency)
            low = text.strip().lower()
            if low in ("!subscribe","!sub"):
                with SUBS_LOCK:
                    if ch not in SUBS:
                        SUBS.append(ch); save_subs(SUBS)
                try:
                    web.chat_postMessage(channel=ch, text="Subscribed this channel for to_user/to_peer_summary.")
                except Exception:
                    pass
                # Flush any pending to_user messages now that we have a channel
                _flush_pending()
                return
            # Runtime toggle: !verbose on|off
            msp = re.match(r"^\s*!verbose\s+(on|off)\b", low)
            if msp:
                val = (msp.group(1) == 'on')
                rt_path = HOME/"state"/"bridge-runtime.json"; rt_path.parent.mkdir(parents=True, exist_ok=True)
                try:
                    cur = {}
                    if rt_path.exists():
                        cur = json.loads(rt_path.read_text(encoding='utf-8'))
                    cur['show_peer_messages'] = bool(val)
                    rt_path.write_text(json.dumps(cur, ensure_ascii=False, indent=2), encoding='utf-8')
                except Exception:
                    pass
                # Also reflect Foreman cc_user to keep a single 'verbose' mental model
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
                    web.chat_postMessage(channel=ch, text=f"Verbose set to: {'ON' if val else 'OFF'} (peer summaries + Foreman CC)")
                except Exception:
                    pass
                return
            if low in ("!unsubscribe","!unsub"):
                with SUBS_LOCK:
                    SUBS2 = [x for x in SUBS if x != ch]
                    if len(SUBS2) != len(SUBS):
                        SUBS[:] = SUBS2; save_subs(SUBS)
                try:
                    web.chat_postMessage(channel=ch, text="Unsubscribed this channel.")
                except Exception:
                    pass
                return

            # Files (if any)
            try:
                flist = event.get('files') or []
                if flist and has_prefix:  # only accept files with explicit routing
                    saved: List[Tuple[Path, Dict[str, Any]]] = []
                    missed: List[str] = []
                    for fo in flist:
                        got = _download_slack_file(fo, ch)
                        if got:
                            saved.append(got)
                        else:
                            link = str((fo or {}).get('permalink') or (fo or {}).get('url_private') or '')
                            if link:
                                missed.append(link)
                    if saved:
                        refs = "\n".join([f"- {str(p)} ({m.get('mime','')},{m.get('bytes',0)} bytes)" for p,m in saved])
                        text = (text + "\n\nFiles:\n" + refs).strip()
                        # Index inbound files
                        try:
                            for pth, mt in saved:
                                _append_inbound_index(pth, mt, routes=['peerA','peerB'] if 'both:' in text.lower() else (['peerA'] if text.lower().strip().startswith('a:') else ['peerB']))
                        except Exception:
                            pass
                    if missed:
                        refs2 = "\n".join([f"- {u}" for u in missed])
                        text = (text + "\n\nFiles (undownloaded):\n" + refs2).strip()
            except Exception as e:
                _append_log(f"[error] files in message failed: {e}")
            # Route and write inbox
            if not has_prefix:
                # Allow only subscribe/unsubscribe without prefix; drop other chatter
                _append_log(f"[inbound] drop without prefix ch={ch}")
                return
            routes, body = _route_from_text(stripped, default_route)
            mid = f"slack-{int(time.time())}-{user[-4:]}"
            _write_inbox(routes, body, mid)
        except Exception:
            try:
                from slack_sdk.socket_mode.response import SocketModeResponse  # type: ignore
                _client.send_socket_mode_response(SocketModeResponse(envelope_id=getattr(req, 'envelope_id', '')))  # type: ignore
            except Exception: pass
    if socket_mode_available and client is not None:
        client.socket_mode_request_listeners.append(handle)
        _append_log("[info] slack socket mode starting …")
        # Connect in a background thread to avoid blocking outbound watchers
        threading.Thread(target=lambda: client.connect(), daemon=True).start()
    # Outbound files watcher
    def _send_file_to_channels(fp: Path, caption: str) -> bool:
        cap = _summarize(caption or '', 1200, 10)
        # Include dynamically subscribed channels as well
        with SUBS_LOCK:
            chs = list(dict.fromkeys((channels_to_user or []) + (channels_peer or []) + (SUBS or [])))
        try:
            from slack_sdk import WebClient  # type: ignore
            from slack_sdk.errors import SlackApiError  # type: ignore
            cli = WebClient(token=bot_token)
            ok_any = False
            for ch in chs:
                try:
                    # Prefer files_upload_v2: use 'channel' (singular) and a file-like object
                    try:
                        with open(fp, 'rb') as f:
                            cli.files_upload_v2(channel=ch, file=f, filename=fp.name, initial_comment=(cap or None))
                    except SlackApiError as e1:
                        # Retry v2 once without initial_comment (some workspaces/apps reject it)
                        try:
                            with open(fp, 'rb') as f0:
                                cli.files_upload_v2(channel=ch, file=f0, filename=fp.name)
                        except SlackApiError:
                            # Log and try fallback to legacy files_upload for older workspaces/apps
                            err = (e1.response or {}).get('error') if hasattr(e1, 'response') else str(e1)
                            _append_log(f"[warn] files_upload_v2 failed to {ch}: {err}")
                            try:
                                with open(fp, 'rb') as f2:
                                    cli.files_upload(channels=ch, file=f2, filename=fp.name, initial_comment=(cap or None))
                            except SlackApiError as e2:
                                err2 = (e2.response or {}).get('error') if hasattr(e2, 'response') else str(e2)
                                _append_log(f"[error] slack file upload failed to {ch}: {err2}")
                                continue
                            except Exception as e2:
                                _append_log(f"[error] slack file upload failed to {ch}: {e2}")
                                continue
                    except Exception as e1:
                        _append_log(f"[warn] files_upload_v2 unexpected error to {ch}: {e1}")
                        try:
                            with open(fp, 'rb') as f2:
                                cli.files_upload(channels=ch, file=f2, filename=fp.name, initial_comment=(cap or None))
                        except Exception as e2:
                            _append_log(f"[error] slack file upload failed to {ch}: {e2}")
                            continue
                    ok_any = True
                    time.sleep(0.5)
                except Exception as e:
                    _append_log(f"[error] slack file upload unexpected error to {ch}: {e}")
            if not chs:
                _append_log(f"[warn] no slack channels configured/subscribed for file send: {fp.name}")
            return ok_any
        except Exception as e:
            _append_log(f"[error] slack_sdk missing or upload failed: {e}")
            return False

    def watch_outbound_files():
        files_cfg = (cfg.get('files') or {})
        if not bool(files_cfg.get('enabled', True)):
            return
        out_root = Path(str(files_cfg.get('outbound_dir') or (HOME/"work"/"upload"/"outbound")))
        sent_files: Dict[str, float] = {}
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
            # Returns (route, caption_wo_prefix); route in {'peerA','peerB','both'}
            t = (cap or '').lstrip()
            m = re.match(r"^(a:|b:|both:)\s*", t, re.I)
            if m:
                tag = m.group(1).lower();
                body = t[m.end():]
                return ({'a:':'peerA','b:':'peerB','both:':'both'}[tag], body)
            return ('', cap)
        def _route_sidecar(fp: Path) -> str:
            try:
                for sc in (fp.with_suffix(fp.suffix + '.route'), fp.with_name(fp.name + '.route')):
                    if sc.exists():
                        val = (sc.read_text(encoding='utf-8').strip() or '').lower()
                        if val in ('a','peera','peerA'.lower()):
                            return 'peerA'
                        if val in ('b','peerb','peerB'.lower()):
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
                    # If a sent sidecar exists, skip (idempotency across restarts)
                    if f.with_name(f.name + '.sent.json').exists():
                        try:
                            # Best-effort: cleanup payload if sidecar exists
                            f.unlink()
                        except Exception:
                            pass
                        continue
                    key = str(f.resolve())
                    if key in sent_files and (time.time() - sent_files[key] < 3):
                        continue
                    cap0 = _read_caption(f)
                    route = _route_sidecar(f)
                    if not route:
                        route, cap0 = _detect_route_from_caption(cap0)
                    if not route:
                        route = 'both'
                    label = 'PeerA' if route=='peerA' else ('PeerB' if route=='peerB' else 'PeerA+PeerB')
                    ok = _send_file_to_channels(f, f"[{label}]\n" + cap0)
                    if ok:
                        meta = {'platform':'slack','ts': int(time.time()), 'file': str(f.name)}
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
                        sent_files[key] = time.time()
            except Exception:
                pass
            time.sleep(1.0)

    threading.Thread(target=watch_outbound_files, daemon=True).start()

    try:
        while True: time.sleep(1.0)
    finally:
        try: client.disconnect()
        except Exception: pass

if __name__ == '__main__':
    main()
