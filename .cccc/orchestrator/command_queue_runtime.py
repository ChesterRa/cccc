# -*- coding: utf-8 -*-
from __future__ import annotations
import os, json, time, shlex
from pathlib import Path
from typing import Any, Dict, Optional

from .command_queue import init_command_offsets, append_command_result

def make(ctx: Dict[str, Any]):
    home: Path = ctx['home']
    state: Path = ctx['state']
    session: str = ctx['session']

    # Aliases used by original body
    paneA = ctx['paneA']; paneB = ctx['paneB']
    profileA = ctx['profileA']; profileB = ctx['profileB']
    settings = ctx['settings']
    cli_profiles_path: Path = ctx['cli_profiles_path']
    PROCESSED_RETENTION: int = int(ctx.get('PROCESSED_RETENTION', 200))

    # External helpers/functions
    write_status = ctx['write_status']
    weave_system = ctx['weave_system']
    _send_handoff = ctx['send_handoff']
    _maybe_prepend_preamble = ctx['maybe_prepend_preamble']
    _process_im_commands = ctx['process_im_commands']
    _run_aux_cli = ctx['run_aux_cli']
    read_yaml = ctx['read_yaml']
    _write_yaml = ctx['write_yaml']
    load_profiles = ctx['load_profiles']
    tmux = ctx['tmux']
    tmux_start_interactive = ctx['tmux_start_interactive']
    _inbox_dir = ctx['inbox_dir']
    _processed_dir = ctx['processed_dir']
    outbox_write = ctx['outbox_write']

    commands_path: Path = ctx['commands_path']
    commands_paths = ctx['commands_paths']
    commands_last_pos_map: Dict[str,int] = ctx['commands_last_pos_map']
    processed_command_ids = ctx['processed_command_ids']
    resolved = ctx['resolved']

    def _inject_full_system():
        try:
            sysA = ctx['weave_system'](home, "peerA"); sysB = ctx['weave_system'](home, "peerB")
            _send_handoff("System", "PeerA", f"<FROM_SYSTEM>\n{sysA}\n</FROM_SYSTEM>\n")
            _send_handoff("System", "PeerB", f"<FROM_SYSTEM>\n{sysB}\n</FROM_SYSTEM>\n")
            return True, "SYSTEM injected to both peers"
        except Exception as e:
            return False, f"inject failed: {e}"

    def _write_tui_reply(cmd: str, ok: bool, message: str):
        """Write command reply to tui-replies.jsonl for TUI to display"""
        try:
            reply_file = state / "tui-replies.jsonl"
            reply = {
                'cmd': cmd,
                'ok': ok,
                'message': message,
                'ts': time.time()
            }
            with reply_file.open('a', encoding='utf-8') as f:
                f.write(json.dumps(reply, ensure_ascii=False) + '\n')
                f.flush()
        except Exception:
            pass  # Silent fail - don't break command processing

    def _is_debug() -> bool:
        try:
            import os, yaml  # type: ignore
            if str(os.environ.get('CCCC_LOG_LEVEL','')).lower() == 'debug':
                return True
            # Fallback: cli_profiles delivery.logging_level
            try:
                if cli_profiles_path and cli_profiles_path.exists():
                    data = yaml.safe_load(cli_profiles_path.read_text(encoding='utf-8')) or {}
                    lvl = str(((data.get('delivery') or {}) or {}).get('logging_level') or '').lower()
                    return lvl == 'debug'
            except Exception:
                pass
        except Exception:
            pass
        return False

    def consume(max_items: int = 50):
        nonlocal resolved, commands_last_pos_map
        # deliver_paused/shutdown_requested bridged via boxes in ctx
        deliver_paused = bool(ctx['deliver_paused_box']['v'])
        shutdown_requested = bool(ctx['shutdown_requested_box']['v'])
        cnt = 0
        scan = {"paths": []}
        any_error = False
        for cpath in commands_paths:
            if cnt >= max_items:
                break
            if not cpath.exists():
                try:
                    scan["paths"].append({"path": str(cpath), "exists": False})
                except Exception:
                    pass
                continue
            key = str(cpath)
            last = commands_last_pos_map.get(key, 0)
            try:
                with cpath.open('r', encoding='utf-8', errors='replace') as f:
                    try:
                        f.seek(0, 2)
                        endpos = f.tell()
                    except Exception:
                        endpos = None
                    if endpos is not None and last > endpos:
                        last = endpos
                        commands_last_pos_map[key] = last
                    try:
                        f.seek(last)
                    except Exception:
                        f.seek(0)
                        last = 0
                        commands_last_pos_map[key] = last
                    try:
                        scan["paths"].append({"path": key, "exists": True, "last_pos": last, "end_pos": endpos})
                    except Exception:
                        pass
                    while cnt < max_items:
                        line = f.readline()
                        if not line:
                            break
                        commands_last_pos_map[key] = f.tell()
                        cnt += 1
                        raw_line = line.strip()
                        if not raw_line:
                            continue
                        try:
                            obj = json.loads(raw_line)
                        except Exception:
                            any_error = True
                            # Record only parse errors (raw)
                            try:
                                (state/"last_command.json").write_text(json.dumps({"raw": raw_line}, ensure_ascii=False, indent=2), encoding='utf-8')
                            except Exception:
                                pass
                            continue
                        cmd_id = str(obj.get('id') or obj.get('request_id') or '')
                        if cmd_id and cmd_id in processed_command_ids:
                            continue
                        if isinstance(obj, dict) and 'result' in obj:
                            continue
                        ctype = str(obj.get('type') or obj.get('command') or '').strip().lower()
                        target = str(obj.get('target') or obj.get('route') or '').strip().lower()
                        args = obj.get('args') or {}
                        ok, msg = False, 'unsupported'
                        try:
                            if ctype in ('a','b','both','send'):
                                text = str(args.get('text') or obj.get('text') or '').strip()
                                if not text:
                                    ok, msg = False, 'empty text'
                                else:
                                    if ctype == 'a' or target in ('a','peera','peer_a'):
                                        _send_handoff('User','PeerA', _maybe_prepend_preamble('PeerA', f"<FROM_USER>\n{text}\n</FROM_USER>\n"))
                                    elif ctype == 'b' or target in ('b','peerb','peer_b'):
                                        _send_handoff('User','PeerB', _maybe_prepend_preamble('PeerB', f"<FROM_USER>\n{text}\n</FROM_USER>\n"))
                                    else:
                                        _send_handoff('User','PeerA', _maybe_prepend_preamble('PeerA', f"<FROM_USER>\n{text}\n</FROM_USER>\n"))
                                        _send_handoff('User','PeerB', _maybe_prepend_preamble('PeerB', f"<FROM_USER>\n{text}\n</FROM_USER>\n"))
                                    ok, msg = True, 'sent'
                            elif ctype in ('pause','resume'):
                                deliver_paused = (ctype == 'pause')
                                write_status(deliver_paused)
                                ok, msg = True, f"handoff {'paused' if deliver_paused else 'resumed'}"
                            elif ctype in ('sys-refresh','sys_refresh','sysrefresh'):
                                ok, msg = _inject_full_system()
                            elif ctype in ('clear','reset'):
                                ok, msg = True, 'clear acknowledged (noop)'
                            elif ctype in ('inbox','inbox_policy','startup_inbox_policy'):
                                try:
                                    policy = str((args.get('policy') or obj.get('policy') or 'resume')).strip().lower()
                                    def _apply_policy(label: str) -> int:
                                        try:
                                            inbox = _inbox_dir(home, label)
                                            proc = _processed_dir(home, label)
                                            files = sorted([f for f in inbox.iterdir() if f.is_file()], key=lambda p: p.name)
                                        except Exception:
                                            files = []
                                        if not files:
                                            return 0
                                        if policy in ('archive','discard'):
                                            moved = 0
                                            for f in files:
                                                try:
                                                    proc.mkdir(parents=True, exist_ok=True)
                                                    f.rename(proc/f.name); moved += 1
                                                except Exception:
                                                    pass
                                            try:
                                                allp = sorted(proc.iterdir(), key=lambda p: p.name)
                                                if len(allp) > PROCESSED_RETENTION:
                                                    for ff in allp[:len(allp)-PROCESSED_RETENTION]:
                                                        try:
                                                            ff.unlink()
                                                        except Exception:
                                                            pass
                                            except Exception:
                                                pass
                                            log_ledger(home, {"from":"system","kind":"startup-inbox-discard" if policy=='discard' else 'startup-inbox-archive',"peer":label,"moved":moved})
                                            return moved
                                        log_ledger(home, {"from":"system","kind":"startup-inbox-resume","peer":label})
                                        return len(files)
                                    a = _apply_policy('PeerA'); b = _apply_policy('PeerB')
                                    ok, msg = True, f"inbox policy {policy}: PeerA={a} PeerB={b}"
                                except Exception as e:
                                    ok, msg = False, f"inbox_policy failed: {e}"
                            elif ctype in ('launch','launch_peers'):
                                try:
                                    who = str((args.get('who') or 'both')).lower()
                                    try:
                                        resolved = load_profiles(home)
                                    except Exception:
                                        pass
                                    def _first_bin(cmd: str) -> str:
                                        try:
                                            import shlex
                                            return shlex.split(cmd or '')[0] if cmd else ''
                                        except Exception:
                                            return (cmd or '').split(' ')[0]
                                    def _bin_available(cmd: str) -> bool:
                                        prog = _first_bin(cmd)
                                        if not prog:
                                            return False
                                        import shutil
                                        return shutil.which(prog) is not None
                                    pa_cmd2 = (resolved.get('peerA') or {}).get('command') or ''
                                    pb_cmd2 = (resolved.get('peerB') or {}).get('command') or ''
                                    pa_eff2 = os.environ.get('CLAUDE_I_CMD') or pa_cmd2
                                    pb_eff2 = os.environ.get('CODEX_I_CMD') or pb_cmd2
                                    def _normalize_absbin(cmd: str) -> str:
                                        try:
                                            prog = _first_bin(cmd)
                                            if not prog:
                                                return cmd
                                            import shutil, shlex
                                            ab = shutil.which(prog)
                                            if not ab:
                                                return cmd
                                            parts = shlex.split(cmd)
                                            parts[0] = ab
                                            return " ".join(shlex.quote(x) for x in parts)
                                        except Exception:
                                            return cmd
                                    pa_eff2 = _normalize_absbin(pa_eff2)
                                    pb_eff2 = _normalize_absbin(pb_eff2)
                                    a_ok = _bin_available(pa_eff2) if who in ('a','both') else True
                                    b_ok = _bin_available(pb_eff2) if who in ('b','both') else True
                                    try:
                                        (state/"last_launch.json").write_text(json.dumps({
                                            "who": who,
                                            "peerA": {"cmd": pa_cmd2, "eff": pa_eff2, "ok": a_ok},
                                            "peerB": {"cmd": pb_cmd2, "eff": pb_eff2, "ok": b_ok},
                                            "paneA": paneA, "paneB": paneB,
                                        }, ensure_ascii=False, indent=2), encoding='utf-8')
                                    except Exception:
                                        pass
                                    if (who in ('a','both')) and (not a_ok):
                                        ok = False
                                        msg = f"PeerA CLI unavailable: {_first_bin(pa_eff2) or '(empty)'}"
                                        try:
                                            outbox_write(home, {"type":"to_user","peer":"System","text":msg})
                                        except Exception:
                                            pass
                                    if (who in ('b','both')) and (not b_ok):
                                        ok = False
                                        msg = f"PeerB CLI unavailable: {_first_bin(pb_eff2) or '(empty)'}"
                                        try:
                                            outbox_write(home, {"type":"to_user","peer":"System","text":msg})
                                        except Exception:
                                            pass
                                    pa_cwd2 = (resolved.get('peerA') or {}).get('cwd') or '.'
                                    pb_cwd2 = (resolved.get('peerB') or {}).get('cwd') or '.'
                                    def _wrap_cwd2(cmd: str, cwd: str | None) -> str:
                                        if cwd and cwd not in ('.',''):
                                            return f"cd {cwd} && {cmd}"
                                        return cmd
                                    launched = []
                                    if who in ('a','both') and pa_eff2 and _bin_available(pa_eff2):
                                        print(f"[LAUNCH] PeerA → {pa_eff2} (cwd={pa_cwd2}) pane={paneA}")
                                        tmux_start_interactive(paneA, _wrap_cwd2(pa_eff2, pa_cwd2))
                                        launched.append('PeerA')
                                    elif who in ('a','both'):
                                        print(f"[LAUNCH] PeerA not started (CLI unavailable): {_first_bin(pa_eff2) or '(empty)'}")
                                    if who in ('b','both') and pb_eff2 and _bin_available(pb_eff2):
                                        print(f"[LAUNCH] PeerB → {pb_eff2} (cwd={pb_cwd2}) pane={paneB}")
                                        tmux_start_interactive(paneB, _wrap_cwd2(pb_eff2, pb_cwd2))
                                        launched.append('PeerB')
                                    elif who in ('b','both'):
                                        print(f"[LAUNCH] PeerB not started (CLI unavailable): {_first_bin(pb_eff2) or '(empty)'}")
                                    ok = True
                                    msg = f"launched {' & '.join(launched) if launched else 'none'}"
                                    if not launched:
                                        try:
                                            outbox_write(home, {"type":"to_user","peer":"System","text":"Launch requested but no CLI available for selected actors (check agents.yaml or PATH)."})
                                        except Exception:
                                            pass
                                except Exception as e:
                                    ok, msg = False, f"launch failed: {e}"
                            elif ctype in ('quit','exit'):
                                # Clean up bridge processes BEFORE killing tmux session
                                try:
                                    cleanup_bridges_fn = ctx.get('cleanup_bridges')
                                    if cleanup_bridges_fn:
                                        cleanup_bridges_fn()
                                except Exception:
                                    pass
                                # Now kill tmux session
                                try:
                                    tmux("kill-session","-t",session)
                                    ok, msg = True, 'session terminated'
                                except Exception as e:
                                    ok, msg = False, f"kill-session failed: {e}"
                                shutdown_requested = True
                            elif ctype in ('foreman','fm'):
                                sub = str(args.get('action') or obj.get('action') or '').strip().lower() or 'status'
                                # Direct execution for TUI source, im_commands for IM source
                                if obj.get('source') == 'tui':
                                    # Execute directly and get real result
                                    try:
                                        import yaml as _yaml
                                        fc_p = settings/"foreman.yaml"
                                        fc = _yaml.safe_load(fc_p.read_text(encoding='utf-8')) if fc_p.exists() else {}
                                        
                                        if sub == 'on' or sub == 'enable' or sub == 'start':
                                            fc['enabled'] = True
                                            _write_yaml(fc_p, fc)
                                            ok, msg = True, "Foreman enabled"
                                        elif sub == 'off' or sub == 'disable' or sub == 'stop':
                                            fc['enabled'] = False
                                            _write_yaml(fc_p, fc)
                                            ok, msg = True, "Foreman disabled"
                                        elif sub == 'status':
                                            enabled = fc.get('enabled', False)
                                            status = "enabled" if enabled else "disabled"
                                            ok, msg = True, f"Foreman is {status}"
                                        elif sub == 'now':
                                            ok, msg = True, "Foreman immediate run requested"
                                        else:
                                            ok, msg = False, f"Unknown foreman action: {sub}"
                                        
                                        _write_tui_reply('foreman', ok, msg)
                                    except Exception as e:
                                        _write_tui_reply('foreman', False, f"Foreman {sub} failed: {str(e)[:100]}")
                                else:
                                    # For IM: use im_commands mechanism
                                    data = {'request_id': cmd_id or str(int(time.time()*1000)), 'command': 'foreman', 'source': obj.get('source', 'unknown'), 'args': {'action': sub}}
                                    tmp = state/"im_commands"/f"cmd-{int(time.time()*1000)}.json"
                                    try:
                                        tmp.write_text(json.dumps(data, ensure_ascii=False), encoding='utf-8')
                                        _process_im_commands()
                                        ok, msg = True, f"foreman {sub} requested"
                                    except Exception as e:
                                        ok, msg = False, f"foreman {sub} failed: {e}"
                            elif ctype in ('c','aux_cli','aux'):
                                prompt_text = str(args.get('prompt') or obj.get('prompt') or '').strip()
                                if not prompt_text:
                                    ok, msg = False, 'empty prompt'
                                else:
                                    rc, out, err, cmd_line = _run_aux_cli(prompt_text)
                                    ok = (rc == 0)
                                    summary = [f"[Aux CLI] exit={rc}", f"command: {cmd_line}"]
                                    if out: summary.append("stdout:\n" + out.strip())
                                    if err: summary.append("stderr:\n" + err.strip())
                                    msg = "\n".join(summary)
                            elif ctype in ('focus',):
                                try:
                                    hint = str((args.get('hint') or obj.get('hint') or '')).strip()
                                    ctx['request_por_refresh']('focus-tui', hint=hint or None, force=True)
                                    ok, msg = True, 'focus requested'
                                except Exception as e:
                                    ok, msg = False, f'focus failed: {e}'
                            elif ctype in ('roles-set-actor','roles_set_actor'):
                                try:
                                    role = str(args.get('role') or obj.get('role') or '').strip()
                                    actor = str(args.get('actor') or obj.get('actor') or '').strip()
                                    cp = read_yaml(cli_profiles_path)
                                    roles = dict(cp.get('roles') or {})
                                    roles.setdefault('peerA', {})
                                    roles.setdefault('peerB', {})
                                    roles.setdefault('aux', {})
                                    if role in roles:
                                        roles[role]['actor'] = actor
                                        cp['roles'] = roles
                                        _write_yaml(cli_profiles_path, cp)
                                        try:
                                            resolved = load_profiles(home)
                                        except Exception:
                                            pass
                                        write_status(deliver_paused)
                                        ok, msg = True, f"role {role} set to {actor}"
                                    else:
                                        ok, msg = False, f"unknown role: {role}"
                                except Exception as e:
                                    ok, msg = False, f'roles-set-actor failed: {e}'
                            elif ctype in ('im-config',):
                                try:
                                    provider = str(args.get('provider') or obj.get('provider') or '').strip().lower()
                                    if provider not in ('telegram', 'slack', 'discord'):
                                        ok, msg = False, f'unsupported IM provider: {provider}'
                                    else:
                                        cfgp = settings / f"{provider}.yaml"
                                        cfg = read_yaml(cfgp) or {}
                                        
                                        if provider == 'telegram':
                                            # Token field
                                            token = str(args.get('token') or obj.get('token') or '').strip()
                                            if token:
                                                cfg['token'] = token
                                                if 'token_env' not in cfg:
                                                    cfg['token_env'] = 'TELEGRAM_BOT_TOKEN'
                                                if 'autostart' not in cfg:
                                                    cfg['autostart'] = True
                                            # Chat ID field (allow_chats)
                                            chat_id = args.get('chat_id') or obj.get('chat_id') or ''
                                            if chat_id:
                                                try:
                                                    cfg['allow_chats'] = [int(chat_id)]
                                                except (ValueError, TypeError):
                                                    cfg['allow_chats'] = [str(chat_id)]
                                        
                                        elif provider == 'slack':
                                            # Bot token field
                                            bot_token = str(args.get('bot_token') or obj.get('bot_token') or '').strip()
                                            if bot_token:
                                                cfg['bot_token'] = bot_token
                                                if 'bot_token_env' not in cfg:
                                                    cfg['bot_token_env'] = 'SLACK_BOT_TOKEN'
                                                if 'autostart' not in cfg:
                                                    cfg['autostart'] = True
                                            # Channel ID field
                                            channel_id = str(args.get('channel_id') or obj.get('channel_id') or '').strip()
                                            if channel_id:
                                                if 'channels' not in cfg:
                                                    cfg['channels'] = {}
                                                cfg['channels']['to_user'] = [channel_id]
                                                cfg['channels']['to_peer_summary'] = [channel_id]
                                        
                                        elif provider == 'discord':
                                            # Bot token field
                                            bot_token = str(args.get('bot_token') or obj.get('bot_token') or '').strip()
                                            if bot_token:
                                                cfg['bot_token'] = bot_token
                                                if 'bot_token_env' not in cfg:
                                                    cfg['bot_token_env'] = 'DISCORD_BOT_TOKEN'
                                                if 'autostart' not in cfg:
                                                    cfg['autostart'] = True
                                            # Channel ID field
                                            channel_id = str(args.get('channel_id') or obj.get('channel_id') or '').strip()
                                            if channel_id:
                                                try:
                                                    ch_id = int(channel_id)
                                                except (ValueError, TypeError):
                                                    ch_id = channel_id
                                                if 'channels' not in cfg:
                                                    cfg['channels'] = {}
                                                cfg['channels']['to_user'] = [ch_id]
                                                cfg['channels']['to_peer_summary'] = [ch_id]
                                        
                                        _write_yaml(cfgp, cfg)
                                        ok, msg = True, f'{provider} IM configured'
                                except Exception as e:
                                    ok, msg = False, f'im-config failed: {e}'
                            elif ctype in ('token',):
                                # Legacy token command - deprecated, kept for backward compatibility
                                try:
                                    action = str(args.get('action') or obj.get('action') or '').strip().lower()
                                    provider = str(args.get('provider') or obj.get('provider') or 'telegram').strip().lower()
                                    if provider not in ('telegram', 'slack', 'discord'):
                                        ok, msg = False, f'unsupported provider: {provider}'
                                    else:
                                        cfgp = settings / f"{provider}.yaml"
                                        cfg = read_yaml(cfgp) or {}
                                        if action == 'set':
                                            val = str(args.get('value') or obj.get('value') or '').strip()
                                            if not val:
                                                ok, msg = False, 'token value required'
                                            else:
                                                if provider == 'telegram':
                                                    cfg['token'] = val
                                                else:
                                                    cfg['bot_token'] = val
                                                _write_yaml(cfgp, cfg)
                                                ok, msg = True, f'{provider} token set'
                                        elif action == 'unset':
                                            token_key = 'token' if provider == 'telegram' else 'bot_token'
                                            if token_key in cfg:
                                                cfg.pop(token_key, None)
                                                _write_yaml(cfgp, cfg)
                                            ok, msg = True, f'{provider} token unset'
                                        else:
                                            ok, msg = False, 'unsupported token action'
                                except Exception as e:
                                    ok, msg = False, f'token failed: {e}'
                            elif ctype in ('review',):
                                try:
                                    ctx['send_aux_reminder']('manual-review')
                                    ok, msg = True, 'Review requested'
                                except Exception as e:
                                    ok, msg = False, f'review failed: {e}'
                                # Send reply to TUI
                                if obj.get('source') == 'tui':
                                    _write_tui_reply('review', ok, msg)
                            elif ctype in ('verbose',):
                                try:
                                    vraw = str(args.get('value') or obj.get('value') or '').strip().lower()
                                    if vraw not in ('on','off',''):
                                        ok, msg = False, 'unsupported verbose value'
                                    else:
                                        desired = {'on': True, 'off': False}.get(vraw)
                                        import json as _json
                                        rt_path = state/"bridge-runtime.json"; rt_path.parent.mkdir(parents=True, exist_ok=True)
                                        cur = {}
                                        try:
                                            if rt_path.exists():
                                                cur = _json.loads(rt_path.read_text(encoding='utf-8'))
                                        except Exception:
                                            cur = {}
                                        if desired is None:
                                            desired = (not bool(cur.get('show_peer_messages', True)))
                                        cur['show_peer_messages'] = bool(desired)
                                        try:
                                            rt_path.write_text(_json.dumps(cur, ensure_ascii=False, indent=2), encoding='utf-8')
                                        except Exception:
                                            pass
                                        # Mirror to foreman cc_user for a single mental model
                                        try:
                                            import yaml as _yaml
                                            fc_p = home/"settings"/"foreman.yaml"
                                            if fc_p.exists():
                                                fc = _yaml.safe_load(fc_p.read_text(encoding='utf-8')) or {}
                                            else:
                                                fc = {}
                                            fc.setdefault('enabled', False)
                                            fc.setdefault('interval_seconds', 900)
                                            fc.setdefault('agent', 'reuse_aux')
                                            fc.setdefault('prompt_path', './FOREMAN_TASK.md')
                                            fc['cc_user'] = bool(desired)
                                            fc_p.write_text(_yaml.safe_dump(fc, allow_unicode=True, sort_keys=False), encoding='utf-8')
                                        except Exception:
                                            pass
                                        ok, msg = True, f"verbose={'on' if desired else 'off'}"
                                except Exception as e:
                                    ok, msg = False, f'verbose failed: {e}'
                            elif ctype in ('passthru','pass','raw'):
                                try:
                                    peer = str(args.get('peer') or obj.get('peer') or '').upper()
                                    cmdline = str(args.get('cmd') or obj.get('cmd') or '').strip()
                                    if not cmdline:
                                        ok, msg = False, 'empty passthru'
                                    else:
                                        ctx['_send_raw_to_cli'](home, 'PeerA' if peer=='A' else 'PeerB', cmdline, paneA, paneB)
                                        ok, msg = True, 'sent'
                                except Exception as e:
                                    ok, msg = False, f'passthru failed: {e}'
                            else:
                                ok, msg = False, 'unsupported'
                        except Exception as e:
                            ok, msg = False, f"error: {e}"
                        # Persist last_command.json only when not ok (error/unsupported)
                        if not ok:
                            any_error = True
                            try:
                                (state/"last_command.json").write_text(json.dumps({"obj": obj, "result": {"ok": ok, "message": msg}}, ensure_ascii=False, indent=2), encoding='utf-8')
                            except Exception:
                                pass
                        if cmd_id:
                            processed_command_ids.add(cmd_id)
                        append_command_result(commands_path, cmd_id or '-', ok, msg)
            except Exception:
                pass
        # Only write scan snapshot when DEBUG or this pass had errors
        if any_error or _is_debug():
            try:
                scan["last_pos_map"] = commands_last_pos_map
                (state/"commands.scan.json").write_text(json.dumps(scan, ensure_ascii=False, indent=2), encoding='utf-8')
            except Exception:
                pass
        # propagate updated values back to ctx
        ctx['deliver_paused_box']['v'] = deliver_paused
        ctx['shutdown_requested_box']['v'] = shutdown_requested
        ctx['resolved'] = resolved
        ctx['commands_last_pos_map'] = commands_last_pos_map
        return {
            'deliver_paused': deliver_paused,
            'shutdown_requested': shutdown_requested,
            'resolved': resolved,
            'commands_last_pos_map': commands_last_pos_map,
        }

    return type('CQAPI', (), {'consume': consume})
