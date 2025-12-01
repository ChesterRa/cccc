#!/usr/bin/env python3
# -*- coding: utf-8 -*-
from pathlib import Path
import os, sys, shutil, argparse, subprocess, json, atexit, signal, time, shlex
from typing import Optional

def _bootstrap(src_root: Path, target: Path, *, force: bool = False, include_guides: bool = False):
    # Preferred: packaged resources (importlib.resources)
    src_cccc = None
    try:
        from importlib import resources as _res
        base = _res.files("cccc_scaffold").joinpath("scaffold")  # type: ignore
        if base.is_dir():
            src_cccc = Path(str(base))
    except Exception:
        src_cccc = None
    # Fallback: repo/source layout during development
    if src_cccc is None:
        candidates = [src_root/".cccc", Path(sys.prefix)/".cccc", Path(sys.exec_prefix)/".cccc"]
        src_cccc = next((p for p in candidates if p.exists()), None)
    if src_cccc is None:
        print("[FATAL] Missing scaffold resources; reinstall the package or run from source repo with .cccc/")
        raise SystemExit(1)
    target_cccc = target/".cccc"
    target.mkdir(parents=True, exist_ok=True)

    def copy_one(src: Path, dst: Path):
        if dst.exists() and not force:
            print(f"[SKIP] Exists: {dst}")
            return
        if src.is_dir():
            if dst.exists() and force:
                shutil.rmtree(dst)
            shutil.copytree(src, dst)
        else:
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)
        print(f"[OK] Wrote: {dst}")

    # Copy .cccc scaffold (exclude state and dynamic files)
    for root, dirs, files in os.walk(src_cccc):
        rel = Path(root).relative_to(src_cccc)
        # Skip dynamic/state directories
        if any(str(rel).startswith(p) for p in ("state",)):
            continue
        for fn in files:
            # Skip caches and local state
            if fn.endswith(".pyc"):
                continue
            src = Path(root)/fn
            dst = target_cccc/rel/fn
            copy_one(src, dst)

    # Do not copy entry script; users run the installed `cccc` command from the package
    # Optional: copy reference guides into .cccc/guides/ (keep repo root clean)
    if include_guides:
        guides_dir = target_cccc/"guides"
        for top in ("CLAUDE.md", "AGENTS.md"):
            src = src_root/top
            if src.exists():
                copy_one(src, guides_dir/top)

    # Default Ephemeral: if target is not this product repo, append /.cccc/** to target .gitignore
    try:
        if (target/".git").exists() and target.resolve() != src_root.resolve():
            gi = target/".gitignore"
            line = "/.cccc/**"
            existing = gi.read_text(encoding="utf-8") if gi.exists() else ""
            if line not in existing.splitlines():
                with gi.open("a", encoding="utf-8") as f:
                    if existing and not existing.endswith("\n"):
                        f.write("\n")
                    f.write("# Ignore CCCC runtime domain (Ephemeral mode)\n")
                    f.write(line+"\n")
                print(f"[OK] Appended to .gitignore: {line}")
    except Exception:
        pass

    print(f"\n[BOOTSTRAP] CCCC scaffold written to: {target}")


def main():
    parser = argparse.ArgumentParser(description="CCCC Orchestrator & Bootstrap",
                                     epilog=(
        "Examples:\n"
        "  cccc init              # Create .cccc scaffold in current repo and append /.cccc/** to .gitignore\n"
        "  cccc doctor            # Check git/tmux/python and Telegram token presence\n"
        "  cccc token set         # Save Telegram token to .cccc/settings/telegram.yaml (gitignored)\n"
        "  cccc bridge start      # Start Telegram bridge (requires token)\n"
        "  cccc clean             # Purge .cccc/{mailbox,work,logs,state}/\n"
        "  cccc run               # Run orchestrator\n"
    ))
    sub = parser.add_subparsers(dest="cmd")
    p_init = sub.add_parser("init", help="Copy .cccc scaffold into target repo")
    p_init.add_argument("--to", default=".", help="Target repository path (default: current dir)")
    p_init.add_argument("--force", action="store_true", help="Overwrite existing files/directories")
    p_init.add_argument("--include-guides", action="store_true", help="Also copy CLAUDE.md/AGENTS.md into .cccc/guides/")

    p_up = sub.add_parser("upgrade", help="Upgrade existing .cccc (like init; by default non-overwriting)")
    p_up.add_argument("--to", default=".", help="Target repository path (default: current dir)")
    p_up.add_argument("--force", action="store_true", help="Overwrite existing files/directories")
    p_up.add_argument("--include-guides", action="store_true", help="Also copy CLAUDE.md/AGENTS.md into .cccc/guides/")

    # Utility subcommands (M2.1/M2.2)
    p_clean = sub.add_parser("clean", help="Purge .cccc/{mailbox,work,logs,state}/ runtime artifacts")

    p_reset = sub.add_parser("reset", help="Reset runtime state for new task (clear state/mailbox/logs/work)")
    p_reset.add_argument("--archive", action="store_true", help="Archive docs/por/ before reset")

    p_doctor = sub.add_parser("doctor", help="Environment check (git/tmux/python/telegram)")
    p_doctor.add_argument("what", nargs="?", default="all", choices=["all","roles"], help="Subset to check: all|roles (default: all)")
    p_roles = sub.add_parser("roles", help="Show roles/actors/commands and availability (same as: doctor roles)")

    p_token = sub.add_parser("token", help="Manage Telegram token (stored in .cccc/settings/telegram.yaml; gitignored)")
    p_token.add_argument("action", choices=["set","unset","show"], help="Action: set/unset/show")
    p_token.add_argument("value", nargs="?", help="When action=set, token value (empty = prompt)")

    p_bridge = sub.add_parser("bridge", help="Control chat bridges (telegram|slack|discord|all)")
    p_bridge.add_argument("name", choices=["telegram","slack","discord","all"], help="Bridge name or 'all'")
    p_bridge.add_argument("action", choices=["start","stop","status","restart","logs"], help="Start/stop/show status/restart/show logs")
    p_bridge.add_argument("-n","--lines", type=int, default=120, help="Tail this many lines for logs action")
    p_bridge.add_argument("-f","--follow", action="store_true", help="Follow logs (stream)")

    # Utility: show versions
    sub.add_parser("version", help="Show package and scaffold versions")

    # Runtime commands
    p_run = sub.add_parser("run", help="Start orchestrator tmux session and attach")
    p_run.add_argument("--session", help="Tmux session name (default: cccc-<repo>)")
    p_run.add_argument("--command", dest="run_command", help="Override command launched inside tmux (debug)")

    p_kill = sub.add_parser("kill", help="Force stop orchestrator tmux session")
    p_kill.add_argument("--session", help="Tmux session name (default: read from state/session.json)")

    args, rest = parser.parse_known_args()
    repo_root = Path(__file__).resolve().parent
    if args.cmd in {"init", "upgrade"}:
        _bootstrap(repo_root, Path(args.to).resolve(), force=bool(args.force), include_guides=bool(getattr(args, 'include_guides', False)))
        return

    # Resolve runtime home (allow utility subcommands to run even if missing)
    home = Path(os.environ.get("CCCC_HOME", ".cccc")).resolve()

    # Lightweight YAML read/write helpers (for subcommands)
    def _read_yaml(p: Path):
        if not p.exists():
            return {}
        try:
            import yaml  # type: ignore
            return yaml.safe_load(p.read_text(encoding='utf-8')) or {}
        except Exception:
            # naive fallback
            d = {}
            for line in p.read_text(encoding='utf-8').splitlines():
                line = line.strip()
                if not line or line.startswith('#') or ':' not in line:
                    continue
                k, v = line.split(':', 1)
                if '#' in v:
                    v = v.split('#', 1)[0]
                d[k.strip()] = v.strip().strip('"\'')
            return d

    def _write_yaml(p: Path, obj):
        try:
            import yaml  # type: ignore
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(yaml.safe_dump(obj, allow_unicode=True, sort_keys=False), encoding='utf-8')
            return
        except Exception:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(json.dumps(obj, ensure_ascii=False, indent=2), encoding='utf-8')

    # ---- Early dispatch: utility subcommands (must not trigger orchestrator/wizard) ----
    def _which(bin_name: str) -> bool:
        try:
            subprocess.run(["bash","-lc",f"command -v {bin_name}"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
            return True
        except Exception:
            return False

    def _cmd_clean():
        for d in (home/"mailbox", home/"work", home/"logs", home/"state"):
            try:
                if d.exists():
                    shutil.rmtree(d)
                    print(f"[CLEAN] Purged {d}")
            except Exception as e:
                print(f"[CLEAN] Failed to purge {d}: {e}")

    def _cmd_reset(archive: bool = False):
        """Reset runtime state for new task. Optionally archive POR/SUBPOR."""
        # Safety check: warn if orchestrator session is running
        session_file = home / "state" / "session.json"
        if session_file.exists():
            try:
                session_data = json.loads(session_file.read_text(encoding='utf-8'))
                session_name = str(session_data.get('session') or '').strip()
                if session_name:
                    # Check if tmux session exists (using session name as socket)
                    result = subprocess.run(
                        ["tmux", "-L", session_name, "has-session", "-t", session_name],
                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
                    )
                    if result.returncode == 0:
                        print(f"[WARN] Orchestrator session '{session_name}' is running.")
                        print("       Consider running `cccc kill` first to avoid conflicts.")
                        try:
                            confirm = input("Continue anyway? (y/N): ").strip().lower()
                            if confirm != 'y':
                                print("[RESET] Aborted.")
                                return
                        except (EOFError, KeyboardInterrupt):
                            print("\n[RESET] Aborted.")
                            return
            except Exception:
                pass  # Ignore errors reading session state

        # Handle POR/SUBPOR: archive (--archive) or delete (default)
        por_dir = Path.cwd() / "docs" / "por"
        if por_dir.exists():
            por_md = por_dir / "POR.md"
            subpor_dirs = [d for d in por_dir.iterdir() if d.is_dir() and d.name.startswith('T') and d.name != 'archive']

            if archive:
                # Archive mode: move to timestamped archive directory
                timestamp = time.strftime('%Y%m%d-%H%M%S')
                archive_dir = por_dir / "archive" / timestamp
                try:
                    archive_dir.mkdir(parents=True, exist_ok=True)
                    if por_md.exists():
                        shutil.move(str(por_md), str(archive_dir / "POR.md"))
                        print(f"[RESET] Archived POR.md")
                    for subdir in subpor_dirs:
                        try:
                            shutil.move(str(subdir), str(archive_dir / subdir.name))
                            print(f"[RESET] Archived {subdir.name}")
                        except Exception as e:
                            print(f"[RESET] Failed to archive {subdir.name}: {e}")
                except Exception as e:
                    print(f"[RESET] Failed to create archive: {e}")
            else:
                # Default mode: delete POR/SUBPOR directly
                if por_md.exists():
                    try:
                        por_md.unlink()
                        print(f"[RESET] Deleted POR.md")
                    except Exception as e:
                        print(f"[RESET] Failed to delete POR.md: {e}")
                for subdir in subpor_dirs:
                    try:
                        shutil.rmtree(subdir)
                        print(f"[RESET] Deleted {subdir.name}")
                    except Exception as e:
                        print(f"[RESET] Failed to delete {subdir.name}: {e}")

        # Clear runtime directories (same as clean, but with different messaging)
        cleared = []
        for d in (home/"mailbox", home/"work", home/"logs", home/"state"):
            try:
                if d.exists():
                    shutil.rmtree(d)
                    cleared.append(d.name)
            except Exception as e:
                print(f"[RESET] Failed to purge {d}: {e}")

        if cleared:
            print(f"[RESET] Cleared: {', '.join(cleared)}")
        else:
            print("[RESET] Nothing to clear (directories already empty).")

        print("[RESET] Done. Run `cccc run` to start fresh.")

    def _cmd_doctor(what: str = "all"):
        print("[DOCTOR] Starting checks…")
        ok_git = _which("git")
        ok_tmux = _which("tmux")
        ok_py = True
        try:
            subprocess.run([sys.executable, "-V"], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        except Exception:
            ok_py = False
        if what in ("all",):
            print(f"- git:  {'OK' if ok_git else 'MISSING'}")
            print(f"- tmux: {'OK' if ok_tmux else 'MISSING'}")
            print(f"- python: {'OK' if ok_py else 'MISSING'} ({sys.executable})")
            print(f"- CCCC_HOME: {home} ({'EXISTS' if home.exists() else 'MISSING'})")
        cfg = _read_yaml(home/"settings"/"telegram.yaml") if home.exists() else {}
        def _resolve_token(c):
            src = None
            val = None
            if c and c.get('token'):
                val = str(c.get('token'))
                src = 'config'
            else:
                tenv = (c or {}).get('token_env')
                if tenv:
                    v = os.environ.get(str(tenv), '')
                    if v:
                        val = v
                        src = f"env:{tenv}"
            return val, src
        tok, src = _resolve_token(cfg)
        print(f"- telegram config: {'FOUND' if cfg else 'NONE'}; token: {'SET' if tok else 'NOT SET'}" + (f" (source={src})" if src else ""))
        # Optional hint when env is present but config also has token (config wins by design)
        try:
            tenv = (cfg or {}).get('token_env')
            if tenv and os.environ.get(str(tenv)) and (cfg or {}).get('token'):
                print(f"  hint: env {tenv} is set but ignored (config token takes precedence)")
        except Exception:
            pass
        if not ok_tmux:
            print("Hint: install tmux (e.g., apt install tmux / brew install tmux).")
        if what in ("all",):
            # Slack quick check
            try:
                scfg = _read_yaml(home/"settings"/"slack.yaml")
                at_env = str((scfg or {}).get('app_token_env') or 'SLACK_APP_TOKEN')
                bt_env = str((scfg or {}).get('bot_token_env') or 'SLACK_BOT_TOKEN')
                at = (scfg or {}).get('app_token') or os.environ.get(at_env)
                bt = (scfg or {}).get('bot_token') or os.environ.get(bt_env)
                print(f"- slack config: {'FOUND' if scfg else 'NONE'}; bot_token: {'SET' if bt else 'NOT SET'}; app_token: {'SET' if at else 'NOT SET'}")
                # SDK presence
                try:
                    import slack_sdk  # type: ignore
                    print("  - slack_sdk: OK")
                except Exception:
                    print("  - slack_sdk: MISSING (install with: pip install slack_sdk)")
            except Exception:
                pass
            # Discord quick check
            try:
                dcfg = _read_yaml(home/"settings"/"discord.yaml")
                be = str((dcfg or {}).get('bot_token_env') or 'DISCORD_BOT_TOKEN')
                bt = (dcfg or {}).get('bot_token') or os.environ.get(be)
                print(f"- discord config: {'FOUND' if dcfg else 'NONE'}; bot_token: {'SET' if bt else 'NOT SET'}")
                try:
                    import discord  # type: ignore
                    print("  - discord.py: OK")
                except Exception:
                    print("  - discord.py: MISSING (install with: pip install discord.py)")
            except Exception:
                pass
        # Roles/actors/commands summary (always available via 'doctor roles')
        try:
            sys.path.insert(0, str(home))
            from common.config import load_profiles  # type: ignore
            resolved = load_profiles(home)
            def _first_bin(cmd: str) -> str:
                if not cmd:
                    return ''
                return cmd.strip().split()[0]
            pa = resolved.get('peerA') or {}
            pb = resolved.get('peerB') or {}
            ax = resolved.get('aux') or {}
            # read aux.mode for display
            cp = _read_yaml(home/"settings"/"cli_profiles.yaml") if home.exists() else {}
            # aux is on iff roles.aux.actor is set
            roles = (cp.get('roles') or {}) if isinstance(cp.get('roles'), dict) else {}
            aux_role = (roles.get('aux') or {}) if isinstance(roles.get('aux'), dict) else {}
            aux_mode = 'on' if str((aux_role.get('actor') or '')).strip() else 'off'
            pa_actor = str(pa.get('actor') or '')
            pb_actor = str(pb.get('actor') or '')
            aux_actor = str(ax.get('actor') or '')
            pa_cwd = str(pa.get('cwd') or '.')
            pb_cwd = str(pb.get('cwd') or '.')
            aux_cwd = str(ax.get('cwd') or '.')
            pa_cmd = str(pa.get('command') or '')
            pb_cmd = str(pb.get('command') or '')
            aux_cmd = str(ax.get('invoke_command') or '')
            pa_bin, pb_bin, aux_bin = _first_bin(pa_cmd), _first_bin(pb_cmd), _first_bin(aux_cmd)
            print("- roles (effective):")
            def _ok(b):
                return 'OK' if _which(b) else 'MISSING'
            print(f"  peerA: actor={pa_actor} cwd={pa_cwd} cmd=`{pa_cmd}` (bin={pa_bin}:{_ok(pa_bin)})")
            print(f"  peerB: actor={pb_actor} cwd={pb_cwd} cmd=`{pb_cmd}` (bin={pb_bin}:{_ok(pb_bin)})")
            print(f"  aux:   actor={aux_actor or 'none'} cwd={aux_cwd} mode={aux_mode} cmd=`{aux_cmd}` (bin={aux_bin}:{_ok(aux_bin)})")
        except Exception as e:
            print(f"- roles: failed to resolve: {e}")

    def _cmd_token(action: str, value: str|None):
        cfg_path = home/"settings"/"telegram.yaml"
        cfg = _read_yaml(cfg_path)
        if action == 'set':
            tok = value
            if not tok:
                try:
                    tok = input("Enter Telegram Bot Token: ").strip()
                except Exception:
                    tok = None
            if not tok:
                print("[TOKEN] No token set."); return
            cfg['token'] = tok
            _write_yaml(cfg_path, cfg)
            print(f"[TOKEN] Saved to {cfg_path} (file is gitignored)")
        elif action == 'unset':
            if 'token' in cfg:
                cfg.pop('token', None)
                _write_yaml(cfg_path, cfg)
                print("[TOKEN] Token removed.")
            else:
                print("[TOKEN] No saved token.")
        else:  # show
            tok = cfg.get('token') if cfg else None
            if tok:
                print("[TOKEN] Saved: " + (tok[:4] + "…" + tok[-4:]) )
            else:
                print("[TOKEN] Not saved. Use `cccc token set`.")

    def _cmd_bridge(name: str, action: str, *, lines: int = 120, follow: bool = False):
        """Manage a single bridge by name."""
        state = home/"state"; state.mkdir(parents=True, exist_ok=True)
        script = {
            'telegram': home/"adapters"/"bridge_telegram.py",
            'slack':    home/"adapters"/"bridge_slack.py",
            'discord':  home/"adapters"/"bridge_discord.py",
        }.get(name)
        pid_path = state/f"bridge-{name}.pid"
        log_path = state/f"bridge-{name}.log"
        def _find_pids_for(script_path: Path) -> list[int]:
            pids: list[int] = []
            sp = str(script_path.resolve()) if script_path else ''
            if not sp:
                return pids
            proc = Path('/proc')
            try:
                for d in proc.iterdir():
                    if not d.is_dir():
                        continue
                    if not d.name.isdigit():
                        continue
                    pid = int(d.name)
                    try:
                        cmd = (d/"cmdline").read_bytes().decode('utf-8','ignore')
                        if sp in cmd:
                            pids.append(pid)
                    except Exception:
                        continue
            except Exception:
                pass
            return pids
        def _alive(pid: int) -> bool:
            try:
                os.kill(pid, 0)
                return True
            except Exception:
                return False
        def _start():
            if not script or not script.exists():
                print(f"[BRIDGE] Script not found for {name}: {script}"); return
            env = os.environ.copy(); src = None
            # Resolve tokens per adapter
            if name == 'telegram':
                cfg = _read_yaml(home/"settings"/"telegram.yaml")
                token_env = str((cfg or {}).get('token_env') or 'TELEGRAM_BOT_TOKEN')
                tok = None
                if (cfg or {}).get('token'):
                    tok = str(cfg.get('token')); src = 'config'
                else:
                    tenv = (cfg or {}).get('token_env')
                    if tenv:
                        v = os.environ.get(str(tenv), '')
                        if v:
                            tok = v; src = f"env:{tenv}"
                if not tok:
                    print("[BRIDGE] Telegram token not found. Run `cccc token set` or set env."); return
                env[token_env] = tok
            elif name == 'slack':
                cfg = _read_yaml(home/"settings"/"slack.yaml")
                at_env = str((cfg or {}).get('app_token_env') or 'SLACK_APP_TOKEN')
                bt_env = str((cfg or {}).get('bot_token_env') or 'SLACK_BOT_TOKEN')
                if (cfg or {}).get('app_token'): env[at_env] = str(cfg.get('app_token')); src = (src or '') + ' app_token=config'
                if (cfg or {}).get('bot_token'): env[bt_env] = str(cfg.get('bot_token')); src = (src or '') + ' bot_token=config'
                if not env.get(at_env):
                    v = os.environ.get(at_env, '')
                    if v: env[at_env] = v; src = (src or '') + f" app_token=env:{at_env}"
                if not env.get(bt_env):
                    v = os.environ.get(bt_env, '')
                    if v: env[bt_env] = v; src = (src or '') + f" bot_token=env:{bt_env}"
                # Allow outbound-only when only a bot token is present; inbound requires an app token
                if not env.get(bt_env):
                    print("[BRIDGE] Slack bot token missing; set slack.yaml bot_token or env SLACK_BOT_TOKEN."); return
            elif name == 'discord':
                cfg = _read_yaml(home/"settings"/"discord.yaml")
                be = str((cfg or {}).get('bot_token_env') or 'DISCORD_BOT_TOKEN')
                if (cfg or {}).get('bot_token'): env[be] = str(cfg.get('bot_token')); src = 'config'
                if not env.get(be):
                    v = os.environ.get(be, '')
                    if v: env[be] = v; src = f"env:{be}"
                # Discord requires a valid Bot Token; otherwise exit

            # Run from project root
            p = subprocess.Popen([sys.executable, str(script)], env=env, cwd=str(Path.cwd()), start_new_session=True)
            pid_path.write_text(str(p.pid), encoding='utf-8')
            print(f"[BRIDGE] {name} started, pid={p.pid} ({src or 'no-tokens'})")
        def _stop():
            try:
                # Kill by pid file (if present)
                if pid_path.exists():
                    try:
                        pid = int(pid_path.read_text(encoding='utf-8').strip())
                    except Exception:
                        pid = 0
                    if pid:
                        try:
                            os.killpg(os.getpgid(pid), signal.SIGTERM)
                        except Exception:
                            try:
                                os.kill(pid, signal.SIGTERM)
                            except Exception:
                                pass
                # Also scan /proc for any matching script processes and stop them all
                pids = _find_pids_for(script)
                for pid in pids:
                    try:
                        os.killpg(os.getpgid(pid), signal.SIGTERM)
                    except Exception:
                        try:
                            os.kill(pid, signal.SIGTERM)
                        except Exception:
                            pass
                print(f"[BRIDGE] {name} stop signal sent (targets={len(pids) + (1 if pid_path.exists() else 0)}).")
            except Exception as e:
                print(f"[BRIDGE] {name} stop failed:", e)
        def _status():
            pids = _find_pids_for(script)
            pid_file_pid = None
            if pid_path.exists():
                try:
                    pid_file_pid = int(pid_path.read_text(encoding='utf-8').strip())
                except Exception:
                    pid_file_pid = None
            if pids:
                print(f"[BRIDGE] {name} running (instances={len(pids)}): {', '.join(str(p) for p in pids)}")
            else:
                print(f"[BRIDGE] {name} not running.")
            if pid_file_pid and pid_file_pid not in pids:
                print(f"  note: pid file exists but process not found (stale pid {pid_file_pid}).")
        def _logs(lines: int = 120, follow: bool = False):
            lg = log_path
            if not lg.exists():
                print("[BRIDGE] Log file not found:", lg)
                return
            try:
                if follow:
                    # Simple follow implementation
                    print(f"[BRIDGE] Tailing {lg} (Ctrl-C to stop)…")
                    with open(lg, 'r', encoding='utf-8') as f:
                        # jump to last lines
                        try:
                            from collections import deque
                            dq = deque(f, maxlen=lines)
                            for ln in dq:
                                print(ln.rstrip())
                        except Exception:
                            pass
                        while True:
                            ln = f.readline()
                            if not ln:
                                time.sleep(0.5); continue
                            print(ln.rstrip())
                else:
                    # print last N lines
                    try:
                        from collections import deque
                        with open(lg, 'r', encoding='utf-8') as f:
                            dq = deque(f, maxlen=lines)
                            for ln in dq:
                                print(ln.rstrip())
                    except Exception as e:
                        print(f"[BRIDGE] Failed to read logs: {e}")
            except KeyboardInterrupt:
                pass
        if action == 'start': _start(); return
        if action == 'stop': _stop(); return
        if action == 'status': _status(); return
        if action == 'restart': _stop(); time.sleep(0.5); _start(); return
        if action == 'logs': _logs(lines=lines, follow=follow); return

    if args.cmd == 'clean':
        _cmd_clean(); return
    if args.cmd == 'reset':
        _cmd_reset(archive=bool(getattr(args, 'archive', False))); return
    if args.cmd == 'doctor':
        _cmd_doctor(getattr(args,'what','all')); return
    if args.cmd == 'roles':
        _cmd_doctor('roles'); return
    if args.cmd == 'token':
        _cmd_token(args.action, getattr(args, 'value', None)); return
    if args.cmd == 'bridge':
        name = getattr(args, 'name')
        act = getattr(args, 'action')
        if name == 'all':
            for nm in ('telegram','slack','discord'):
                try:
                    print(f"[BRIDGE] {nm} → {act}")
                    _cmd_bridge(nm, act, lines=int(getattr(args,'lines',120) or 120), follow=bool(getattr(args,'follow', False)))
                except Exception as e:
                    print(f"[BRIDGE] {nm} {act} failed: {e}")
            return
        _cmd_bridge(name, act, lines=int(getattr(args,'lines',120) or 120), follow=bool(getattr(args,'follow', False))); return
    if args.cmd == 'version':
        # Package version
        pkg_ver = "unknown"
        try:
            try:
                import importlib.metadata as md  # py3.8+
            except Exception:
                import importlib_metadata as md  # type: ignore
            for name in ("cccc-pair","cccc_pair","cccc"):
                try:
                    pkg_ver = md.version(name); break
                except Exception:
                    continue
        except Exception:
            pass
        # Scaffold info
        scaffold_path = home
        exists = scaffold_path.exists()
        file_count = 0
        if exists:
            try:
                for root, dirs, files in os.walk(scaffold_path):
                    if any(x in root for x in ("state","logs","work","mailbox","__pycache__")):
                        continue
                    file_count += len(files)
            except Exception:
                pass
        print(f"cccc package: {pkg_ver}")
        print(f"scaffold path: {scaffold_path} (exists={exists}, files~{file_count})")
        return
    # Defer 'kill' until functions are defined to avoid NameError/UnboundLocalError
    pending_kill_session = getattr(args, 'session', None) if args.cmd == 'kill' else None

    run_mode = args.cmd in (None, 'run', 'kill')

    if not run_mode:
        print(f"[ERROR] Unknown command: {args.cmd}")
        return

    # Run orchestrator (tmux session)
    if not home.exists():
        print(f"[FATAL] CCCC directory not found: {home}\nRun `cccc init` in your target repository to copy the scaffold, or set CCCC_HOME.")
        raise SystemExit(1)
    sys.path.insert(0, str(home))

    # Lightweight wizard: ask once about Telegram connection (optional; local by default).
    # - Token can come from env or a one-time prompt (we save to YAML, gitignored)
    # - Config writes only non-sensitive fields
    def _isatty() -> bool:
        try:
            return sys.stdin.isatty()
        except Exception:
            return False

    # _read_yaml/_write_yaml defined above; reuse here

    BRIDGE_PROC = {"p": None}

    def _kill_stale_bridge():
        """Best-effort: terminate a previously spawned bridge (by pid file), and
        remove stale lock/pid files to avoid duplicate or wedged instances.
        """
        try:
            pid_path = home/"state"/"telegram-bridge.pid"
            if pid_path.exists():
                try:
                    pid = int(pid_path.read_text(encoding='utf-8').strip())
                except Exception:
                    pid = 0
                if pid > 0:
                    # Verify target process belongs to this repo (by cmdline path)
                    belongs=False
                    try:
                        cmdline_path=f'/proc/{pid}/cmdline'
                        from pathlib import Path as _P
                        if _P(cmdline_path).exists():
                            cmd=_P(cmdline_path).read_bytes().decode('utf-8','ignore')
                            # Must contain this repo's bridge_telegram.py path
                            newp = str((home/'adapters'/'bridge_telegram.py').resolve())
                            if newp in cmd:
                                belongs=True
                    except Exception:
                        pass
                    if not belongs:
                        # Do not kill unknown process; treat as stale pid
                        pid=0
                    if pid>0:
                        try:
                            os.killpg(os.getpgid(pid), signal.SIGTERM)
                        except Exception:
                            try:
                                os.kill(pid, signal.SIGTERM)
                            except Exception:
                                pass
                        for _ in range(10):
                            try:
                                os.kill(pid, 0)
                                time.sleep(0.3)
                            except Exception:
                                break
                        else:
                            try:
                                os.killpg(os.getpgid(pid), signal.SIGKILL)
                            except Exception:
                                try:
                                    os.kill(pid, signal.SIGKILL)
                                except Exception:
                                    pass
            try:
                (home/"state"/"telegram-bridge.lock").unlink(missing_ok=True)
            except Exception:
                pass
            try:
                (home/"state"/"telegram-bridge.pid").unlink(missing_ok=True)
            except Exception:
                pass
        except Exception:
            pass

    def _spawn_telegram_bridge(env_extra: dict):
        bridge = home/"adapters"/"bridge_telegram.py"
        if not bridge.exists():
            print("[WARN] Telegram bridge script not found; skipping.")
            return None
        # Read token from YAML config (if not provided in env_extra)
        cfg = _read_yaml(home/"settings"/"telegram.yaml")
        token_env = str((cfg or {}).get('token_env') or 'TELEGRAM_BOT_TOKEN')
        env = os.environ.copy(); env.update(env_extra or {})
        src = None
        if env.get(token_env):
            src = f"env:{token_env}"
        else:
            if (cfg or {}).get('token'):
                env[token_env] = str(cfg.get('token'))
                src = 'config'
            else:
                tenv = (cfg or {}).get('token_env')
                if tenv:
                    v = os.environ.get(str(tenv), '')
                    if v:
                        env[token_env] = v
                        src = f"env:{tenv}"
        print(f"[TELEGRAM] Starting bridge (long-polling)… (token_source={src or 'none'})")
        # Kill stale instance before spawning
        _kill_stale_bridge()
        # Start new session so we can kill the whole process group on exit; set cwd to project root
        project_root = home.parent
        p = subprocess.Popen([sys.executable, str(bridge)], env=env, cwd=str(project_root), start_new_session=True)
        BRIDGE_PROC["p"] = p
        try:
            # Persist PID for diagnostics
            pid_path = home/"state"/"telegram-bridge.pid"
            pid_path.parent.mkdir(parents=True, exist_ok=True)
            pid_path.write_text(str(p.pid), encoding='utf-8')
        except Exception:
            pass
        return p

    def _cleanup_bridge():
        p = BRIDGE_PROC.get("p")
        if not p:
            return
        try:
            # Send SIGTERM to the process group (Linux/macOS)
            try:
                os.killpg(os.getpgid(p.pid), signal.SIGTERM)
            except Exception:
                p.terminate()
            # Wait up to 5s, then SIGKILL the group
            for _ in range(10):
                if p.poll() is not None:
                    break
                time.sleep(0.5)
            if p.poll() is None:
                try:
                    os.killpg(os.getpgid(p.pid), signal.SIGKILL)
                except Exception:
                    try:
                        p.kill()
                    except Exception:
                        pass
        finally:
            BRIDGE_PROC["p"] = None
            # best-effort: remove stale pid file
            try:
                (home/"state"/"telegram-bridge.pid").unlink(missing_ok=True)  # type: ignore
            except Exception:
                pass

    # Ensure child cleanup on exit and signals
    def _install_shutdown_hooks():
        atexit.register(_cleanup_bridge)
        # Also best-effort stop Slack/Discord bridges started outside this process
        def _kill_known_bridges():
            try:
                for nm in ('slack','discord'):
                    try:
                        _cmd_bridge(nm, 'stop')
                    except Exception:
                        pass
            except Exception:
                pass
        atexit.register(_kill_known_bridges)
        def _sig_handler(signum, frame):
            _cleanup_bridge()
            # Re-raise default behavior
            raise SystemExit(0)
        for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            try:
                signal.signal(sig, _sig_handler)
            except Exception:
                pass

    def _session_state_path() -> Path:
        return home/"state"/"session.json"

    def _write_session_state(session: str):
        try:
            state_dir = home/"state"
            state_dir.mkdir(parents=True, exist_ok=True)
            payload = {
                "session": session,
                "created_at": time.time(),
                "cwd": str(Path.cwd()),
            }
            _session_state_path().write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")
        except Exception:
            pass

    def _read_session_state() -> Optional[str]:
        p = _session_state_path()
        if not p.exists():
            return None
        try:
            data = json.loads(p.read_text(encoding="utf-8"))
            value = str(data.get("session") or "").strip()
            return value or None
        except Exception:
            return None

    def _remove_session_state():
        try:
            _session_state_path().unlink()
        except FileNotFoundError:
            pass
        except Exception:
            pass

    def _default_session_name() -> str:
        return f"cccc-{Path.cwd().name}"

    def _tmux_has_session(session: str) -> bool:
        """Check if tmux session exists (using session name as socket for isolation)."""
        try:
            result = subprocess.run(["tmux", "-L", session, "has-session", "-t", session], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            return result.returncode == 0
        except FileNotFoundError:
            return False

    def _tmux_kill_session(session: str) -> bool:
        """Kill tmux session (using session name as socket for isolation)."""
        try:
            result = subprocess.run(["tmux", "-L", session, "kill-session", "-t", session], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            return result.returncode == 0
        except FileNotFoundError:
            return False

    def _cmd_kill(session_arg: Optional[str] = None):
        session = session_arg or _read_session_state()
        if not session:
            print("[KILL] No tmux session recorded. Use --session to specify explicitly.")
            return
        if not _tmux_has_session(session):
            print(f"[KILL] Session '{session}' not found.")
            _remove_session_state()
            for leftover in (home/"state"/"tui.ready", home/"state"/"tui.log"):
                try:
                    leftover.unlink()
                except FileNotFoundError:
                    pass
                except Exception:
                    pass
            return
        if _tmux_kill_session(session):
            print(f"[KILL] Session '{session}' terminated.")
        else:
            print(f"[KILL] Failed to terminate session '{session}'.")
        _remove_session_state()
        for leftover in (home/"state"/"tui.ready", home/"state"/"tui.log"):
            try:
                leftover.unlink()
            except FileNotFoundError:
                pass
            except Exception:
                pass

    def _cmd_run(session_arg: Optional[str] = None, override_cmd: Optional[str] = None):
        if not _which("tmux"):
            print("[ERROR] tmux is required. Install tmux and try again.")
            return
        session = session_arg or _default_session_name()
        command_str = override_cmd
        if not command_str:
            parts = [sys.executable, "-u", str(home/"orchestrator_tmux.py"), "--home", str(home), "--session", session]
            command_str = " ".join(shlex.quote(p) for p in parts)
        if _tmux_has_session(session):
            print(f"[RUN] Reusing existing tmux session '{session}'.")
            _write_session_state(session)
        else:
            # Use session name as socket (-L) for environment isolation between instances
            tmux_args = [
                "tmux", "-L", session, "new-session", "-d",
                "-s", session,
                "-n", "cccc",
                "-c", str(Path.cwd()),
                command_str,
            ]
            result = subprocess.run(tmux_args)
            if result.returncode != 0:
                print(f"[ERROR] tmux new-session failed (exit {result.returncode}).")
                return
            _write_session_state(session)
            print(f"[RUN] Started tmux session '{session}' (socket: {session}).")
        print(f"[RUN] Attaching to session '{session}' (supervised; will cleanup on exit).")
        try:
            # Run tmux client as a child process so our atexit/supervision can run after it exits
            # Use -L to connect to the same socket as the session
            subprocess.run(["tmux", "-L", session, "attach", "-t", session], check=False)
        finally:
            # Best-effort cleanup bridges on detach/exit
            try:
                _cleanup_bridge()
            except Exception:
                pass
            try:
                # Also stop other bridges we might have started
                for nm in ('slack','discord'):
                    try:
                        _cmd_bridge(nm, 'stop')
                    except Exception:
                        pass
            except Exception:
                pass
            # Remove session state markers
            try:
                _remove_session_state()
            except Exception:
                pass
            print("[RUN] tmux client exited; performed cleanup.")
            return

    # Ensure `cccc kill` never triggers the interactive wizard.
    # Always execute kill (even when --session is not provided, _cmd_kill will resolve from state).
    if args.cmd == 'kill':
        _cmd_kill(getattr(args, 'session', None))
        return

    # Startup wizard (interactive) — disabled for `run` to ensure tmux+TUI launch immediately.
    # Users can configure inside the TUI (/setup, /token) or via `cccc doctor/roles`.
    if (args.cmd not in ('run', 'kill') ) and _isatty() and not os.environ.get('CCCC_NO_WIZARD'):
        try:
            print("\n[SETUP] Choose run mode:\n  1) Local CLI only (default)\n  2) Local + connect Telegram\n  3) Local + connect Slack (Socket Mode + Web API)\n  4) Local + connect Discord")
            choice = input("> Enter 1-4 (Enter=1): ").strip() or "1"
        except Exception:
            choice = "1"
        if choice == "2":
            cfg_path = home/"settings"/"telegram.yaml"
            cfg = _read_yaml(cfg_path)
            if not cfg:
                cfg = {
                    "token_env": "TELEGRAM_BOT_TOKEN",
                    "allow_chats": [],
                    "discover_allowlist": True,
                    "autoregister": "open",
                    "max_auto_subs": 3,
                }
            else:
                if not cfg.get("allow_chats"):
                    cfg["discover_allowlist"] = True
            if not cfg.get("autoregister"):
                cfg["autoregister"] = "open"
            if not cfg.get("max_auto_subs"):
                cfg["max_auto_subs"] = 3
            # Normalize allow_chats (handle strings like "[]" or "[123]")
            def _coerce_allowlist(val):
                def to_int(x):
                    try:
                        return int(str(x).strip())
                    except Exception:
                        return None
                if isinstance(val, (list, tuple, set)):
                    out = []
                    for x in val:
                        v = to_int(x)
                        if v is not None:
                            out.append(v)
                    return out
                if isinstance(val, str):
                    s = val.strip().strip('"\'')
                    if not s:
                        return []
                    if s.startswith('[') and s.endswith(']'):
                        try:
                            arr = json.loads(s)
                            return _coerce_allowlist(arr)
                        except Exception:
                            pass
                    s2 = s.strip('[]')
                    parts = [p for p in s2.replace(',', ' ').split() if p]
                    out = []
                    for p in parts:
                        v = to_int(p)
                        if v is not None:
                            out.append(v)
                    return out
                return []
            coerced = _coerce_allowlist(cfg.get("allow_chats"))
            if coerced:
                cfg["allow_chats"] = coerced
            else:
                cfg["allow_chats"] = []
                cfg["discover_allowlist"] = True
            # Prompt for token (saved to .cccc/settings/telegram.yaml; gitignored)
            token_env = str(cfg.get("token_env") or "TELEGRAM_BOT_TOKEN")
            token_val = os.environ.get(token_env, "")
            if not token_val:
                saved = str(cfg.get('token') or '')
                if saved:
                    # Offer a minimal choice: use saved or paste new (no extra skip here to avoid duplicate mode selection)
                    masked = (saved[:4] + "…" + saved[-4:]) if len(saved) >= 8 else "(saved)"
                    print(f"[SETUP] Saved Bot Token detected (masked: {masked}).")
                    print("  1) Use saved token (default)\n  2) Paste a new token and save")
                    try:
                        ch = input("> Choose 1/2 (Enter=1): ").strip() or "1"
                    except Exception:
                        ch = "1"
                    if ch == "2":
                        try:
                            token_val = input("Paste new Telegram Bot Token: ").strip()
                        except Exception:
                            token_val = ""
                        if token_val:
                            cfg['token'] = token_val
                            _write_yaml(cfg_path, cfg)
                            print("[SETUP] New token saved.")
                        else:
                            token_val = saved
                            print("[SETUP] Empty input; continue with saved token.")
                    else:
                        token_val = saved
                        print("[SETUP] Using saved token.")
                else:
                    print(f"[SETUP] Env var {token_env} not found. Paste your Bot Token to save it to .cccc/settings/telegram.yaml (local).")
                    try:
                        token_val = input("Paste Telegram Bot Token: ").strip()
                    except Exception:
                        token_val = ""
                    if token_val:
                        cfg['token'] = token_val
                        _write_yaml(cfg_path, cfg)
                        print("[SETUP] Token saved.")
            # Persist early if env provided a token but config lacks one (optional convenience)
            if token_val and not cfg.get('token'):
                cfg['token'] = token_val
                _write_yaml(cfg_path, cfg)
            # Allow user to input chat_id(s) (optional; Enter to skip)
            if not cfg.get("allow_chats"):
                try:
                    raw = input("Optional: enter chat_id (comma/space separated; groups are negative -100...; Enter to skip): ").strip()
                except Exception:
                    raw = ""
                ids = []
                if raw:
                    import re as _re
                    parts = [p for p in _re.split(r"[\s,]+", raw) if p]
                    for p in parts:
                        try:
                            ids.append(int(p))
                        except Exception:
                            pass
                    if ids:
                        cfg["allow_chats"] = ids
                        cfg["discover_allowlist"] = False
                        print(f"[SETUP] allow_chats set to {ids}")
            # Write config (including any changes) to .cccc/settings/telegram.yaml (should not be committed)
            _write_yaml(cfg_path, cfg)
            # Optional: onboarding for chat_id
            if not cfg.get("allow_chats"):
                print("[SETUP] Discovery mode enabled (discover_allowlist=true). You can either:\n  • Send /subscribe in chat (if autoregister=open), or\n  • Send /whoami and find chat_id in .cccc/state/bridge-telegram.log, then add it to allow_chats and restart.")
            # Start bridge (prefer env token; else read token from YAML)
            if token_val:
                _spawn_telegram_bridge({token_env: token_val})
            else:
                # If YAML has a token, _spawn_telegram_bridge will inject it automatically
                if cfg.get('token'):
                    _spawn_telegram_bridge({})
                else:
                    print("[WARN] No token provided; continue in local mode without Telegram.")
        if choice == "3":
            # Interactive Slack setup (prompt for tokens if missing), then start bridge
            cfg_path = home/"settings"/"slack.yaml"
            scfg = _read_yaml(cfg_path) or {
                "app_token_env": "SLACK_APP_TOKEN",
                "bot_token_env": "SLACK_BOT_TOKEN",
                "autostart": False,
                "channels": {"to_user": [], "to_peer_summary": []},
                "outbound": {"cursor": {"start_mode": "tail", "replay_last": 0}},
            }
            at_env = str(scfg.get('app_token_env') or 'SLACK_APP_TOKEN')
            bt_env = str(scfg.get('bot_token_env') or 'SLACK_BOT_TOKEN')
            # Read existing/masked tokens
            saved_app = str(scfg.get('app_token') or '')
            saved_bot = str(scfg.get('bot_token') or '')
            try:
                print("[SETUP][Slack] Provide tokens (Enter to keep saved or use env).")
                if saved_bot:
                    masked = (saved_bot[:6] + "…" + saved_bot[-4:]) if len(saved_bot) >= 10 else "(saved)"
                    print(f"  Bot token (xoxb-): {masked}")
                bot = input("  Paste Bot User OAuth Token (xoxb-, Enter to keep): ").strip() or saved_bot
            except Exception:
                bot = saved_bot
            try:
                if saved_app:
                    masked = (saved_app[:6] + "…" + saved_app[-4:]) if len(saved_app) >= 10 else "(saved)"
                    print(f"  App-level token (xapp-): {masked}")
                app = input("  Paste App-level Token (xapp-, Enter to keep/skip for outbound-only): ").strip() or saved_app
            except Exception:
                app = saved_app
            # Optional: ask for one channel id for to_user
            try:
                cur = (scfg.get('channels') or {}).get('to_user') or []
                cur_id = cur[0] if cur else ''
                if cur_id:
                    print(f"  Current to_user channel id: {cur_id}")
                ch = input("  Optional: channel ID for to_user (e.g., C0123456789, Enter to skip): ").strip()
            except Exception:
                ch = ""
            if bot:
                scfg['bot_token'] = bot
            if app:
                scfg['app_token'] = app
            if ch:
                scfg.setdefault('channels', {}).setdefault('to_user', [])
                if ch not in scfg['channels']['to_user']:
                    scfg['channels']['to_user'].append(ch)
                scfg['channels'].setdefault('to_peer_summary', [])
                if ch not in scfg['channels']['to_peer_summary']:
                    scfg['channels']['to_peer_summary'].append(ch)
            _write_yaml(cfg_path, scfg)
            try:
                _cmd_bridge('slack', 'start')
            except Exception:
                pass
        if choice == "4":
            # Interactive Discord setup (prompt for bot token if missing), then start bridge
            cfg_path = home/"settings"/"discord.yaml"
            dcfg = _read_yaml(cfg_path) or {
                "bot_token_env": "DISCORD_BOT_TOKEN",
                "autostart": False,
                "channels": {"to_user": [], "to_peer_summary": []},
                "outbound": {"cursor": {"start_mode": "tail", "replay_last": 0}},
            }
            be = str(dcfg.get('bot_token_env') or 'DISCORD_BOT_TOKEN')
            saved = str(dcfg.get('bot_token') or '')
            try:
                if saved:
                    masked = (saved[:6] + "…" + saved[-4:]) if len(saved) >= 10 else "(saved)"
                    print(f"[SETUP][Discord] Bot token: {masked}")
                bot = input("  Paste Discord Bot Token (Enter to keep): ").strip() or saved
            except Exception:
                bot = saved
            # Optional channel id for to_user
            try:
                cur = (dcfg.get('channels') or {}).get('to_user') or []
                cur_id = cur[0] if cur else ''
                if cur_id:
                    print(f"  Current to_user channel id: {cur_id}")
                ch = input("  Optional: channel ID for to_user (numeric, Enter to skip): ").strip()
            except Exception:
                ch = ""
            if bot:
                dcfg['bot_token'] = bot
            if ch:
                dcfg.setdefault('channels', {}).setdefault('to_user', [])
                if ch not in dcfg['channels']['to_user']:
                    dcfg['channels']['to_user'].append(ch)
                dcfg['channels'].setdefault('to_peer_summary', [])
                if ch not in dcfg['channels']['to_peer_summary']:
                    dcfg['channels']['to_peer_summary'].append(ch)
            _write_yaml(cfg_path, dcfg)
            try:
                _cmd_bridge('discord', 'start')
            except Exception:
                pass
    # Install shutdown hooks after potential spawn
    _install_shutdown_hooks()

    # Autostart bridges in non-interactive environments when configured
    try:
        # Telegram
        tcfg = _read_yaml(home/"settings"/"telegram.yaml")
        t_auto = True if not tcfg else bool((tcfg or {}).get('autostart', True))
        if t_auto and (BRIDGE_PROC.get('p') is None):
            token_env = str((tcfg or {}).get('token_env') or 'TELEGRAM_BOT_TOKEN')
            token_val = None
            if (tcfg or {}).get('token'):
                token_val = str(tcfg.get('token'))
            else:
                tenv = (tcfg or {}).get('token_env')
                if tenv:
                    v = os.environ.get(str(tenv), '')
                    if v:
                        token_val = v
            if token_val:
                _spawn_telegram_bridge({token_env: token_val})
        # Slack
        scfg = _read_yaml(home/"settings"/"slack.yaml")
        if scfg and bool((scfg or {}).get('autostart', False)):
            try:
                _cmd_bridge('slack', 'start')
            except Exception:
                pass
        # Discord
        dcfg = _read_yaml(home/"settings"/"discord.yaml")
        if dcfg and bool((dcfg or {}).get('autostart', False)):
            try:
                _cmd_bridge('discord', 'start')
            except Exception:
                pass
    except Exception:
        pass

    _cmd_run(getattr(args, 'session', None), getattr(args, 'run_command', None))
    return

if __name__ == "__main__":
    main()
