# -*- coding: utf-8 -*-
"""
CCCC Orchestrator (tmux + long‑lived CLI sessions)
- Left/right panes run PeerA and PeerB interactive sessions (actors are bound at startup).
- Uses tmux to paste messages and capture output, parses <TO_USER>/<TO_PEER>, and runs optional lint/tests before committing.
- Injects a minimal SYSTEM prompt at startup (from prompt_weaver); runtime hot‑reload is removed for simplicity and control.
"""
import os, re, sys, json, time, shlex, fnmatch, subprocess, hashlib, io, shutil, threading, signal, atexit
from datetime import datetime, timedelta
# POSIX file locking for cross-process sequencing; gracefully degrade if unavailable
try:
    import fcntl  # type: ignore
except Exception:  # pragma: no cover
    fcntl = None  # type: ignore
from glob import glob
from pathlib import Path
from typing import Dict, Any, Optional, Tuple, List
from delivery import deliver_or_queue, PaneIdleJudge, new_mid, wrap_with_mid, send_text, find_acks_from_output
try:  # package import (python -m .cccc.orchestrator_tmux)
    from .orchestrator.tmux_layout import (
        tmux, tmux_session_exists, tmux_new_session, tmux_respawn_pane,
        tmux_build_tui_layout, tmux_build_single_peer_layout, tmux_paste, tmux_type, tmux_capture,
        tmux_start_interactive, wait_for_ready,
    )
    from .orchestrator.logging_util import log_ledger, outbox_write
    from .orchestrator.handoff_helpers import (
        _plain_text_without_tags_and_mid,
        _peer_folder_name,
        _inbox_dir,
        _processed_dir,
        _format_local_ts,
        _compose_nudge,
        _compose_detailed_nudge,
        _safe_headline,
        _write_inbox_message,
        _append_suffix_inside,
    )
    from .orchestrator import handoff_helpers as HH
    from .orchestrator import json_util as JU
    from .orchestrator.nudge import make as make_nudge
    from .orchestrator.mailbox_pipeline import make as make_mailbox_pipeline
    from .orchestrator.foreman_scheduler import make as make_foreman_scheduler
    from .orchestrator.foreman import make as make_foreman
    from .orchestrator.launcher import make as make_launcher
    from .orchestrator.keepalive import make as make_keepalive
    from .orchestrator.api_error_recovery import make as make_api_error_recovery
except ImportError:  # script import (python .cccc/orchestrator_tmux.py)
    from orchestrator.tmux_layout import (
        tmux, tmux_session_exists, tmux_new_session, tmux_respawn_pane,
        tmux_build_tui_layout, tmux_build_single_peer_layout, tmux_paste, tmux_type, tmux_capture,
        tmux_start_interactive, wait_for_ready,
    )
    from orchestrator.logging_util import log_ledger, outbox_write
    from orchestrator.handoff_helpers import (
        _plain_text_without_tags_and_mid,
        _peer_folder_name,
        _inbox_dir,
        _processed_dir,
        _format_local_ts,
        _compose_nudge,
        _compose_detailed_nudge,
        _safe_headline,
        _write_inbox_message,
        _append_suffix_inside,
    )
    import orchestrator.handoff_helpers as HH
    import orchestrator.json_util as JU
    from orchestrator.nudge import make as make_nudge
    from orchestrator.mailbox_pipeline import make as make_mailbox_pipeline
    from orchestrator.foreman_scheduler import make as make_foreman_scheduler
    from orchestrator.foreman import make as make_foreman
    from orchestrator.launcher import make as make_launcher
    from orchestrator.keepalive import make as make_keepalive
    from orchestrator.api_error_recovery import make as make_api_error_recovery
from common.config import load_profiles, ensure_env_vars, is_single_peer_mode
from mailbox import ensure_mailbox, MailboxIndex, scan_mailboxes, reset_mailbox, compose_sentinel, sha256_text, is_sentinel_text


# Rebind to external helpers to prefer module implementations
_plain_text_without_tags_and_mid = HH._plain_text_without_tags_and_mid
_peer_folder_name = HH._peer_folder_name
_inbox_dir = HH._inbox_dir
_processed_dir = HH._processed_dir
_format_local_ts = HH._format_local_ts
_compose_nudge = HH._compose_nudge
_compose_detailed_nudge = HH._compose_detailed_nudge
_safe_headline = HH._safe_headline
_write_inbox_message = HH._write_inbox_message

_read_json_safe = JU._read_json_safe
_write_json_safe = JU._write_json_safe

ANSI_RE = re.compile(r"\x1b\[.*?m|\x1b\[?[\d;]*[A-Za-z]")  # strip ANSI color/control sequences
try:
    from .orchestrator.policy_filter import is_high_signal as _pf_is_high_signal, is_low_signal as _pf_is_low_signal, should_forward as _pf_should_forward
except ImportError:
    from orchestrator.policy_filter import is_high_signal as _pf_is_high_signal, is_low_signal as _pf_is_low_signal, should_forward as _pf_should_forward
CONSOLE_ECHO = False

def _is_debug() -> bool:
    try:
        return str(os.environ.get('CCCC_LOG_LEVEL','')).lower() == 'debug'
    except Exception:
        return False

def _dbg(msg: str) -> None:
    if _is_debug():
        try:
            print(msg)
        except Exception:
            pass

SECTION_RE_TPL = r"<\s*{tag}\s*>([\s\S]*?)</\s*{tag}\s*>"
INPUT_END_MARK = "[CCCC_INPUT_END]"

# Aux helper state
# Aux on/off is derived from presence of roles.aux.actor; no explicit mode set
AUX_WORK_ROOT_NAME = "aux_sessions"
AUX_BINDING_BOX = {"template": "", "cwd": "."}

# ---------- REV state helpers (lightweight) ----------
INSIGHT_BLOCK_RE = re.compile(r"```\s*insight\s*([\s\S]*?)```", re.I)

class _TeeStream(io.TextIOBase):
    """Mirror writes to the original stream and a shadow file (line-buffered)."""
    def __init__(self, primary: io.TextIOBase, shadow: io.TextIOBase):
        super().__init__()
        self.primary = primary
        self.shadow = shadow

    @property
    def encoding(self):
        return getattr(self.primary, "encoding", "utf-8")

    @property
    def errors(self):
        return getattr(self.primary, "errors", "replace")

    def readable(self):
        return False

    def writable(self):
        return True

    def write(self, data: str):
        if not data:
            return 0
        written = self.primary.write(data)
        self.primary.flush()
        try:
            self.shadow.write(data)
            self.shadow.flush()
        except Exception:
            pass
        return written

    def flush(self):
        try:
            self.primary.flush()
        except Exception:
            pass
        try:
            self.shadow.flush()
        except Exception:
            pass

    def isatty(self):
        try:
            return self.primary.isatty()
        except Exception:
            return False

    def fileno(self):
        try:
            return self.primary.fileno()
        except Exception:
            raise io.UnsupportedOperation("fileno unsupported for TeeStream")

def _attach_orchestrator_logger(state_dir: Path) -> Optional[Path]:
    """Create/attach orchestrator.log under state/ and tee stdout/stderr into it."""
    log_path = state_dir/"orchestrator.log"
    try:
        log_path.parent.mkdir(parents=True, exist_ok=True)
        log_file = log_path.open("w", encoding="utf-8")
    except Exception:
        return None
    original_stdout = sys.stdout
    original_stderr = sys.stderr
    sys.stdout = _TeeStream(original_stdout, log_file)
    sys.stderr = _TeeStream(original_stderr, log_file)
    def _cleanup():
        try:
            log_file.flush()
            log_file.close()
        except Exception:
            pass
    atexit.register(_cleanup)
    print(f"[INFO] orchestrator log at {log_path}")
    return log_path

# --- PID file management (Unix standard for process liveness detection) ---
def _write_pid_file(state_dir: Path) -> None:
    """
    Write orchestrator PID to state/orchestrator.pid for liveness detection.

    This is the standard Unix approach for daemon/service management:
    - Write PID once at startup (minimal disk I/O)
    - TUI/monitors check process liveness via os.kill(pid, 0) (zero disk I/O)
    - Remove PID file on clean exit

    Handles stale PID files from crashes:
    - If PID file exists, check if process is still alive
    - If dead, remove stale PID and continue
    - If alive, abort (orchestrator already running)
    """
    pid_file = state_dir / "orchestrator.pid"
    current_pid = os.getpid()

    # Check for existing PID file (potential stale from crash)
    if pid_file.exists():
        try:
            old_pid_text = pid_file.read_text(encoding='utf-8', errors='replace').strip()
            old_pid = int(old_pid_text)

            # Try to send signal 0 to check if process exists
            # Signal 0 doesn't actually send a signal, just checks process existence
            try:
                os.kill(old_pid, 0)
                # Process still alive - orchestrator already running!
                print(f"[ERROR] Orchestrator already running with PID {old_pid}")
                print(f"[ERROR] PID file: {pid_file}")
                print(f"[ERROR] If this is incorrect, remove the PID file manually")
                sys.exit(1)
            except ProcessLookupError:
                # Process doesn't exist - stale PID file from crash
                print(f"[INFO] Removing stale PID file (old PID {old_pid} no longer exists)")
                pid_file.unlink()
        except (ValueError, PermissionError) as e:
            # Invalid PID file or permission issue - log and remove
            print(f"[WARN] Invalid PID file, removing: {e}")
            try:
                pid_file.unlink()
            except Exception:
                pass

    # Write current PID
    try:
        pid_file.write_text(str(current_pid), encoding='utf-8')
        print(f"[INFO] Orchestrator PID {current_pid} written to {pid_file}")
    except Exception as e:
        print(f"[WARN] Failed to write PID file: {e}")

def _remove_pid_file(state_dir: Path) -> None:
    """Remove orchestrator PID file on clean exit"""
    pid_file = state_dir / "orchestrator.pid"
    try:
        if pid_file.exists():
            pid_file.unlink()
            print(f"[INFO] Removed PID file {pid_file}")
    except Exception as e:
        print(f"[WARN] Failed to remove PID file: {e}")

# --- inbox/nudge settings (read at startup from cli_profiles.delivery) ---
MB_PULL_ENABLED = True
INBOX_DIRNAME = "inbox"
PROCESSED_RETENTION = 200
SOFT_ACK_ON_MAILBOX_ACTIVITY = False
INBOX_STARTUP_POLICY = "resume"  # resume | discard
INBOX_STARTUP_PROMPT = False
AUX_BINDING_BOX = {"template": "", "cwd": "."}


def _inbox_dir(home: Path, receiver_label: str) -> Path:
    return home/"mailbox"/_peer_folder_name(receiver_label)/INBOX_DIRNAME


def _processed_dir(home: Path, receiver_label: str) -> Path:
    return home/"mailbox"/_peer_folder_name(receiver_label)/"processed"
# Debug: reduce ledger noise for outbox enqueue diagnostics
OUTBOX_DEBUG = False
# Debug: keepalive skip reasons are high-frequency; gate behind this flag
KEEPALIVE_DEBUG = False

def _send_raw_to_cli(home: Path, receiver_label: str, text: str,
                     left_pane: str, right_pane: str):
    """Direct passthrough: send raw text to CLI without any wrappers/MID (tmux send-keys + Enter)."""
    ts = time.strftime('%Y-%m-%d %H:%M:%S')
    # Determine pane and send command with Enter
    pane = left_pane if receiver_label == 'PeerA' else right_pane
    # Send text using tmux send-keys with -l (literal) flag, then send Enter
    tmux("send-keys", "-t", pane, "-l", text)
    tmux("send-keys", "-t", pane, "Enter")
    print(f"[RAW] → {receiver_label} @ {ts}: {text[:80]}")

def run(cmd: str, *, cwd: Optional[Path]=None, timeout: int=600) -> Tuple[int,str,str]:
    p = subprocess.Popen(cmd, shell=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, cwd=str(cwd) if cwd else None)
    try:
        out, err = p.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        p.kill(); return 124, "", "Timeout"
    return p.returncode, out, err

def _escape_for_double_quotes(s: str) -> str:
    """Escape a string for safe inclusion inside a double-quoted POSIX shell string.
    Escapes backslash, double-quote, dollar, and backtick which remain special in "...".
    """
    try:
        s = s.replace('\\', r'\\')
        s = s.replace('"', r'\"')
        s = s.replace('`', r'\`')
        s = s.replace('$', r'\$')
        return s
    except Exception:
        return s

def _build_exec_args(template: str, prompt: str) -> List[str]:
    """Build argv for CLI execution without shell quoting issues.
    Replace {prompt} with a placeholder token, shlex-split the template,
    then inject the prompt as a single argv element. If {prompt} is not
    present, append it as the last argument.
    """
    PLACE = "__PROMPT_PLACEHOLDER__"
    s = template.replace("{prompt}", PLACE) if "{prompt}" in template else (template + " " + PLACE)
    argv = shlex.split(s)
    return [prompt if a == PLACE else a for a in argv]

def ensure_bin(name: str):
    code,_,_ = run(f"command -v {shlex.quote(name)}")
    if code != 0:
        print(f"[FATAL] Executable required: {name}")
        raise SystemExit(1)
def has_bin(name: str) -> bool:
    code,_,_ = run(f"command -v {shlex.quote(name)}"); return code==0

def ensure_git_repo():
    code, out, _ = run("git rev-parse --is-inside-work-tree")
    if code != 0 or "true" not in out:
        print("[INFO] Not a git repository; initializing …")
        run("git init")
        # Ensure identity to avoid commit failures on fresh repos
        code_email, out_email, _ = run("git config --get user.email")
        code_name,  out_name,  _ = run("git config --get user.name")
        if code_email != 0 or not out_email.strip():
            run("git config user.email cccc-bot@local")
        if code_name != 0 or not out_name.strip():
            run("git config user.name CCCC Bot")
        run("git add -A")
        run("git commit -m 'init' || true")
    else:
        # Ensure identity to avoid commit failures on existing repos
        code_email, out_email, _ = run("git config --get user.email")
        code_name,  out_name,  _ = run("git config --get user.name")
        if code_email != 0 or not out_email.strip():
            run("git config user.email cccc-bot@local")
        if code_name != 0 or not out_name.strip():
            run("git config user.name CCCC Bot")

def strip_ansi(s: str) -> str: return ANSI_RE.sub("", s)
def parse_section(text: str, tag: str) -> str:
    m = re.search(SECTION_RE_TPL.format(tag=tag), text, re.I)
    return (m.group(1).strip() if m else "")

# ---------- handoff anti-loop ----------
is_high_signal = _pf_is_high_signal
is_low_signal = _pf_is_low_signal
should_forward = _pf_should_forward

# ---------- tmux ----------
def paste_when_ready(pane: str, profile: Dict[str,Any], text: str, *, timeout: float = 10.0, poke: bool = True):
    # send_text handles delivery reliably using bracketed paste or char-by-char typing
    # timeout and poke parameters kept for compatibility but unused
    send_text(pane, text, profile)

nudge_api = make_nudge({'paste_when_ready': paste_when_ready})

# ---------- YAML & prompts ----------
def read_yaml(p: Path) -> Dict[str,Any]:
    if not p.exists(): return {}
    try:
        import yaml; return yaml.safe_load(p.read_text(encoding="utf-8")) or {}
    except ImportError:
        d: Dict[str, Any] = {}
        for line in p.read_text(encoding="utf-8").splitlines():
            # strip inline comments
            line = line.split('#', 1)[0].rstrip()
            if not line or ":" not in line:
                continue
            k, v = line.split(":", 1)
            if not v.strip():
                # container keys like "peerA:" — ignore in fallback
                continue
            d[k.strip()] = v.strip().strip('"\'')
        return d

# ---------- ledger & policies ----------

def try_lint():
    LINT_CMD=os.environ.get("LINT_CMD","").strip()
    cmd = None
    if LINT_CMD:
        cmd = LINT_CMD
    else:
        # Auto-detect a lightweight linter if available; otherwise skip quietly
        if has_bin("ruff"):
            cmd = "ruff check"
        elif has_bin("eslint"):
            # Only run eslint if a config exists
            cfg_files = [
                ".eslintrc", ".eslintrc.json", ".eslintrc.js", ".eslintrc.cjs",
                ".eslintrc.yaml", ".eslintrc.yml"
            ]
            has_cfg = any(Path(p).exists() for p in cfg_files)
            if not has_cfg and Path("package.json").exists():
                try:
                    pj = json.loads(Path("package.json").read_text(encoding="utf-8"))
                    has_cfg = bool(pj.get("eslintConfig"))
                except Exception:
                    has_cfg = False
            if not has_cfg:
                print("[LINT] Skipped (eslint detected but no config)"); return
            cmd = "eslint . --max-warnings=0"
        else:
            print("[LINT] Skipped (no LINT_CMD and no ruff/eslint)"); return
    code,out,err=run(cmd)
    print("[LINT]", "OK" if code==0 else "FAIL")
    if out.strip(): print(out.strip())
    if err.strip(): print(err.strip())

def try_tests() -> bool:
    TEST_CMD=os.environ.get("TEST_CMD","").strip()
    cmd=None
    if TEST_CMD:
        cmd=TEST_CMD
    else:
        if has_bin("pytest"):
            # Only run pytest if tests exist
            py_patterns = ["tests/**/*.py", "test_*.py", "*_test.py"]
            has_tests = any(glob(p, recursive=True) for p in py_patterns)
            if has_tests:
                cmd="pytest -q"
            else:
                print("[TEST] Skipped (no pytest tests found)"); return True
        elif has_bin("npm") and Path("package.json").exists():
            try:
                pj = json.loads(Path("package.json").read_text(encoding="utf-8"))
                test_script = (pj.get("scripts") or {}).get("test")
                if not test_script:
                    print("[TEST] Skipped (package.json has no test script)"); return True
                # Skip the default placeholder script
                if "no test specified" in test_script:
                    print("[TEST] Skipped (default placeholder npm test script)"); return True
                cmd="npm test --silent"
            except Exception:
                print("[TEST] Skipped (failed to parse package.json)"); return True
        else:
            print("[TEST] Skipped (no TEST_CMD and no pytest/npm)"); return True
    code,out,err=run(cmd)
    ok=(code==0)
    print("[TEST]", "OK" if ok else "FAIL")
    if out.strip(): print(out.strip())
    if err.strip(): print(err.strip())
    return ok

def git_commit(msg: str):
    run("git add -A"); run(f"git commit -m {shlex.quote(msg)}")

# ---------- prompt weaving ----------
def weave_system(home: Path, peer: str) -> str:
    from prompt_weaver import weave_minimal_system_prompt, ensure_rules_docs
    try:
        ensure_rules_docs(home)
    except Exception:
        pass
    return weave_minimal_system_prompt(home, peer)

def weave_preamble_text(home: Path, peer: str) -> str:
    """Preamble for the very first user message (full SYSTEM)."""
    try:
        from prompt_weaver import weave_system_prompt
        return weave_system_prompt(home, peer)
    except Exception:
        # Fallback to minimal system if full generation fails
        return weave_system(home, peer)

DEFAULT_CONTEXT_EXCLUDES = [
    ".venv/**", "node_modules/**", "**/__pycache__/**", "**/*.pyc",
    ".tox/**", "dist/**", "build/**", ".mypy_cache/**"
]

def _matches_any(path: str, patterns: List[str]) -> bool:
    return any(fnmatch.fnmatch(path, pat) for pat in patterns)

def list_repo_files(policies: Dict[str,Any], limit:int=200)->str:
    code,out,_ = run("git ls-files")
    files = out.splitlines()
    context_conf = policies.get("context", {}) if isinstance(policies.get("context", {}), dict) else {}
    excludes = context_conf.get("exclude", DEFAULT_CONTEXT_EXCLUDES)
    max_items = int(context_conf.get("files_limit", limit))
    # Drop excluded patterns only; no diff/patch-based allowlist
    filtered = [p for p in files if not _matches_any(p, excludes)]
    return "\n".join(filtered[:max_items])

def context_blob(policies: Dict[str,Any], phase: str) -> str:
    # Present a compact policy snapshot (without any diff/patch settings)
    pol_view = {k: v for k, v in policies.items() if k not in ("patch_queue",)}
    return (f"# PHASE: {phase}\n# REPO FILES (partial):\n{list_repo_files(policies)}\n\n"
            f"# POLICIES:\n{json.dumps(pol_view, ensure_ascii=False)}\n")


# ---------- watcher ----------
# Note: runtime hot-reload of settings/prompts/personas removed for simplicity.

# ---------- EXCHANGE ----------
def print_block(title: str, body: str):
    """Console-less mode: do not echo blocks to stdout."""
    return


# ---------- MAIN ----------
def main(home: Path, session_name: Optional[str] = None):
    ensure_bin("tmux"); ensure_git_repo()
    # Soft dependency: prompt_toolkit is recommended for the left-pane TUI, but orchestrator core should still run.
    try:
        import prompt_toolkit  # type: ignore
    except Exception:
        print("[WARN] prompt_toolkit not importable in this Python. Left-pane full TUI may not start, but orchestrator will continue.")
        print(f"[WARN] Python: {sys.executable}")
    # Directories
    settings = home/"settings"; state = home/"state"
    state.mkdir(exist_ok=True)
    _attach_orchestrator_logger(state)

    # Write PID file for liveness detection (Unix standard approach)
    _write_pid_file(state)

    settings_confirmed_path = state/"settings.confirmed"
    settings_confirm_ready_after = time.time()
    # Always require fresh setup confirmation each run: remove previous marker and wait for a fresh mtime
    try:
        if settings_confirmed_path.exists():
            settings_confirmed_path.unlink()
            print("[SETUP] Cleared previous settings.confirmed; waiting for fresh confirmation.")
    except Exception as e:
        print(f"[WARN] Unable to clear settings.confirmed: {e}")
    settings_confirm_ready_after = time.time()
    def _settings_confirmed_ready() -> bool:
        try:
            return settings_confirmed_path.stat().st_mtime >= settings_confirm_ready_after
        except FileNotFoundError:
            return False
    # Mark orchestrator startup for troubleshooting
    try:
        (state/"orchestrator.ready").write_text(str(int(time.time())), encoding='utf-8')
    except Exception:
        pass
    # Note: rules are rebuilt after the roles wizard (post-binding) to reflect
    # the current Aux/IM state and avoid stale "Aux disabled" banners.
    # Reset preamble sent flags on each orchestrator start to ensure the first
    # user message per peer carries the preamble in this session.
    try:
        (state/"preamble_sent.json").write_text(json.dumps({"PeerA": False, "PeerB": False}, ensure_ascii=False, indent=2), encoding="utf-8")
    except Exception:
        pass

    policies = read_yaml(settings/"policies.yaml")

    # Load actors configuration for auto-compact
    try:
        actors_doc = read_yaml(settings/"agents.yaml")
        actors = actors_doc.get('actors') if isinstance(actors_doc.get('actors'), dict) else {}
    except Exception:
        actors = {}

    session  = session_name or os.environ.get("CCCC_SESSION") or f"cccc-{Path.cwd().name}"

    # Set tmux socket for this instance to enable environment variable isolation
    # Each cccc instance uses its own tmux server, inheriting the terminal's environment
    from orchestrator.tmux_layout import set_socket
    set_socket(session)

    context_update_last_request = 0.0

    def _perform_reset(mode: str, *, trigger: str, reason: str) -> str:
        mode_norm = (mode or "").lower()
        if mode_norm not in ("compact", "clear"):
            raise ValueError("reset mode must be compact or clear")
        ts = time.strftime('%Y-%m-%d %H:%M')
        if mode_norm == "compact":
            try:
                _send_raw_to_cli(home, 'PeerA', '/compact', paneA, paneB)
                _send_raw_to_cli(home, 'PeerB', '/compact', paneA, paneB)
            except Exception:
                pass
            try:
                sysA = weave_system(home, "peerA"); sysB = weave_system(home, "peerB")
                _send_handoff("System", "PeerA", f"<FROM_SYSTEM>\nManual compact at {ts}.\n{sysA}\n</FROM_SYSTEM>\n")
                _send_handoff("System", "PeerB", f"<FROM_SYSTEM>\nManual compact at {ts}.\n{sysB}\n</FROM_SYSTEM>\n")
            except Exception:
                pass
            log_ledger(home, {"from": "system", "kind": "reset", "mode": "compact", "trigger": trigger})
            write_status(deliver_paused)
            _request_context_refresh(f"reset-{mode_norm}", force=True)
            return "Manual compact executed"
        clear_msg = (
            "<FROM_SYSTEM>\nReset requested: treat this as a fresh exchange. Discard interim scratch context and rely on context/context.yaml for milestones and context/tasks/ for active work.\n"
            "</FROM_SYSTEM>\n"
        )
        _send_handoff("System", "PeerA", clear_msg)
        _send_handoff("System", "PeerB", clear_msg)
        log_ledger(home, {"from": "system", "kind": "reset", "mode": "clear", "trigger": trigger})
        write_status(deliver_paused)
        _request_context_refresh(f"reset-{mode_norm}", force=True)
        return "Manual clear notice issued"

    aux_mode = "off"

    aux_last_reason = ""
    aux_last_reminder: Dict[str, float] = {"PeerA": 0.0, "PeerB": 0.0}
    aux_work_root = home/"work"/AUX_WORK_ROOT_NAME
    aux_work_root.mkdir(parents=True, exist_ok=True)

    def _aux_snapshot() -> Dict[str, Any]:
        return {
            "mode": aux_mode,
            "command": aux_command,
            "last_reason": aux_last_reason,
        }

    def _prepare_aux_bundle(reason: str, stage: str, peer_label: Optional[str], payload: Optional[str]) -> Optional[Path]:
        try:
            session_id = time.strftime("%Y%m%d-%H%M%S")
            if peer_label:
                session_id += f"-{peer_label.lower()}"
            session_path = aux_work_root/session_id
            session_path.mkdir(parents=True, exist_ok=True)
            details: List[str] = []
            details.append("# Aux Helper Context")
            details.append(f"Reason: {reason}")
            details.append(f"Stage: {stage}")
            if aux_command:
                details.append(f"Suggested command: {aux_command}")
            details.append("")
            details.append("## What you can do")
            details.append("- You may inspect repository files and `.cccc/work` artifacts as needed.")
            details.append("- Feel free to create additional notes or scratch files under `.cccc/work/` (e.g., run experiments, capture logs).")
            # No special change format required; peers validate via minimal checks/tests/logs.
            details.append("- Summarize findings, highlight risks, and propose concrete next steps for the peers.")
            details.append("")
            # Aux CLI examples - reflect current binding when available
            details.append(f"## Aux CLI examples (actor={(_resolve_bindings(home).get('aux_actor') or 'none')})")
            details.append("```bash")
            if aux_command:
                details.append("# Prompt with inline text")
                details.append(f"{aux_command.replace('{prompt}', 'Review the context and suggest improvements')}")
                details.append("# Point to specific files or directories")
                details.append(f"{aux_command.replace('{prompt}', '@context/ @.cccc/work/aux_sessions/{session_id} Provide a review summary')}")
            else:
                details.append("# Aux not configured; select an Aux actor at startup to enable one-line invokes.")
            details.append("```")
            details.append("")
            details.append("## Data in this bundle")
            details.append("- `context/context.yaml`: milestones and execution status (read from repo root)")
            details.append("- `context/tasks/`: active tasks (read from repo root)")
            details.append("- `peer_message.txt`: the triggering message or artifact from the peer.")
            details.append("- `notes.txt`: this instruction file.")
            (session_path/"notes.txt").write_text("\n".join(details), encoding="utf-8")
            if payload:
                (session_path/"peer_message.txt").write_text(payload, encoding="utf-8")
            return session_path
        except Exception:
            return None

    def _run_aux_cli(prompt: str, timeout: Optional[int] = None) -> Tuple[int, str, str, str]:
        safe_prompt = _escape_for_double_quotes(prompt)
        template = aux_command_template  # no hard fallback to a specific actor/CLI
        if not template:
            # Aux not configured — explicit error instead of silently falling back
            return 1, "", "Aux is not configured (no actor bound or invoke_command missing).", ""
        if "{prompt}" in template:
            command = template.replace("{prompt}", safe_prompt)
        else:
            command = f"{template} {safe_prompt}"
        try:
            run_cwd = Path(aux_cwd) if aux_cwd else Path.cwd()
            if not run_cwd.is_absolute():
                run_cwd = Path.cwd()/run_cwd
            if timeout is not None and timeout > 0:
                proc = subprocess.run(command, shell=True, capture_output=True, text=True, cwd=str(run_cwd), timeout=timeout)
            else:
                proc = subprocess.run(command, shell=True, capture_output=True, text=True, cwd=str(run_cwd))
            return proc.returncode, proc.stdout, proc.stderr, command
        except Exception as exc:
            return 1, "", str(exc), command

    def _send_aux_reminder(reason: str, peers: Optional[List[str]] = None, *, stage: str = "manual", payload: Optional[str] = None, source_peer: Optional[str] = None):
        nonlocal aux_last_reason
        bundle_path = _prepare_aux_bundle(reason, stage, source_peer, payload)
        targets = peers or ["PeerA", "PeerB"]
        lines = ["Aux helper reminder.", f"Reason: {reason}."]
        if aux_command:
            lines.append(f"Run helper command: {aux_command}")
        else:
            lines.append("Aux not configured (no actor bound). Bind an Aux actor at next start to enable one-line invokes.")
        lines.append("You may inspect `.cccc/work` resources created for this session and perform extended analysis.")
        lines.append("Share verdict/actions/checks in your next response.")
        if bundle_path:
            lines.append(f"Context bundle: {bundle_path}")
        message = "\n".join(lines)
        for label in targets:
            payload = f"<FROM_SYSTEM>\n{message}\n</FROM_SYSTEM>\n"
            _send_handoff("System", label, payload)
            aux_last_reminder[label] = time.time()
        aux_last_reason = reason
        log_ledger(home, {"from": "system", "kind": "aux_reminder", "peers": targets, "reason": reason})
        write_status(deliver_paused)

    # Note: auto Aux trigger based on YAML payload has been removed. Use manual /review or one-off /aux (Aux CLI).


    cli_profiles_path = settings/"cli_profiles.yaml"
    cli_profiles = read_yaml(cli_profiles_path)

    # Read startup wait time from config (used by launch and restart)
    try:
        startup_wait_seconds = float(cli_profiles.get("startup_wait_seconds", 10))
        if startup_wait_seconds <= 0:
            startup_wait_seconds = 10.0
    except Exception:
        startup_wait_seconds = 10.0

    # Foreman helper context (module-based)
    aux_binding_box = AUX_BINDING_BOX
    foreman_ctx = {
        'home': home,
        'settings': settings,
        'state': state,
        'compose_sentinel': compose_sentinel,
        'is_sentinel_text': is_sentinel_text,
        'new_mid': new_mid,
        'read_yaml': read_yaml,
        'write_yaml': None,
        'build_exec_args': _build_exec_args,
        'load_profiles': load_profiles,
        'aux_binding_box': aux_binding_box,
        'wrap_with_mid': wrap_with_mid,
        'write_inbox_message': _write_inbox_message,
        'sha256_text': sha256_text,
        'outbox_write': outbox_write,
        'log_ledger': log_ledger,
    }
    # provide write_yaml if available
    def _write_yaml(p: Path, obj: Dict[str, Any]):
        try:
            import yaml  # type: ignore
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(yaml.safe_dump(obj, allow_unicode=True, sort_keys=False), encoding='utf-8')
        except Exception:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(json.dumps(obj, ensure_ascii=False, indent=2), encoding='utf-8')
    foreman_ctx['write_yaml'] = _write_yaml
    foreman_api = make_foreman(foreman_ctx)
    _load_foreman_conf = foreman_api.load_conf
    _save_foreman_conf = foreman_api.save_conf
    _foreman_state_path = foreman_api.state_path
    _foreman_load_state = foreman_api.load_state
    _foreman_save_state = foreman_api.save_state
    _ensure_foreman_task = foreman_api.ensure_task
    _compose_foreman_prompt = foreman_api.compose_prompt
    _foreman_write_user_message = foreman_api.write_user_message
    _foreman_run_once = foreman_api.run_once
    _foreman_stop_running = foreman_api.stop_running

    def _actors_available() -> List[str]:
        try:
            actors_doc = read_yaml(settings/"agents.yaml")
            acts = actors_doc.get('actors') if isinstance(actors_doc.get('actors'), dict) else {}
            return sorted(list(acts.keys()))
        except Exception:
            return []

    def _current_roles(cp: Dict[str, Any]) -> Tuple[str, str, str, str]:
        roles = cp.get('roles') if isinstance(cp.get('roles'), dict) else {}
        # Do not fall back to specific actors; reflect config as-is.
        pa = str(((roles.get('peerA') or {}).get('actor')) or '').strip()
        pb = str(((roles.get('peerB') or {}).get('actor')) or '').strip()
        ax = str(((roles.get('aux') or {}).get('actor')) or '').strip()
        aux_mode = 'on' if ax else 'off'
        return pa, pb, ax, aux_mode

    def _current_foreman_summary() -> str:
        try:
            fc = _load_foreman_conf()
        except Exception:
            fc = {"enabled": False, "allowed": False}
        agent = str(fc.get('agent') or 'reuse_aux') if bool(fc.get('allowed', False)) else 'none'
        enabled = 'ON' if bool(fc.get('enabled', False)) and bool(fc.get('allowed', False)) else 'OFF'
        if not bool(fc.get('allowed', False)):
            return "none"
        try:
            iv = int(fc.get('interval_seconds') or 900)
        except Exception:
            iv = 900
        return f"agent={agent} ({enabled}), interval={iv}s"

    def _persist_roles(cp: Dict[str, Any], peerA_actor: str, peerB_actor: str, aux_actor: str, aux_mode: str):
        cp = dict(cp or {})
        roles = dict(cp.get('roles') or {})
        roles['peerA'] = dict(roles.get('peerA') or {})
        roles['peerB'] = dict(roles.get('peerB') or {})
        roles['aux']   = dict(roles.get('aux') or {})
        roles['peerA']['actor'] = peerA_actor
        roles['peerB']['actor'] = peerB_actor
        roles['aux']['actor']   = aux_actor
        roles['peerA'].setdefault('cwd','.')
        roles['peerB'].setdefault('cwd','.')
        roles['aux'].setdefault('cwd','.')
        cp['roles'] = roles
        _write_yaml(cli_profiles_path, cp)

    # Roles wizard is disabled during `run` to avoid any pre-TUI blocking prompts.
    # Configuration can be adjusted inside the TUI and is persisted to cli_profiles.yaml.
    # Non-interactive: foreman allowed only if configured in config at start
    try:
        _fc0 = _load_foreman_conf()
        if bool(_fc0.get('enabled', False)):
            # Ensure first run is scheduled after full interval on process start
            try:
                st = _foreman_load_state() or {}
                now_ts = time.time()
                try:
                    iv = float(_fc0.get('interval_seconds',900) or 900)
                except Exception:
                    iv = 900.0
                st.update({'running': False, 'next_due_ts': now_ts + iv, 'last_heartbeat_ts': now_ts})
                _foreman_save_state(st)
                lk = state/"foreman.lock"
                if lk.exists():
                    try: lk.unlink()
                    except Exception: pass
            except Exception:
                pass
    except Exception:
        pass
    # NOTE: Rule files are generated on-demand at launch time (after TUI confirms settings).
    # Early generation was removed because TUI may change config after orchestrator starts.
    # Load roles + actors; ensure required env vars in memory (never persist)
    config_deferred = False
    try:
        resolved = load_profiles(home)
        missing_env = ensure_env_vars(resolved.get('env_require') or [], prompt=True)
        if missing_env:
            log_ledger(home, {"kind":"missing-env", "keys": missing_env})
    except Exception as exc:
        # Do not hard-fail: wait for TUI to provide roles before starting
        print(f"[WARN] config load failed; waiting for roles via TUI: {exc}")
        resolved = { 'peerA': {}, 'peerB': {}, 'aux': {}, 'bindings': {}, 'actors': {}, 'env_require': [] }
        config_deferred = True

    # Always start with dual-peer mode (4-pane layout)
    # Single-peer mode detection and layout adjustment happens in launch command handler
    # This ensures users always see the TUI setup screen first
    single_peer_mode = False  # Layout starts as 4-pane; adjusted after TUI confirms settings

    # Role profiles merged with actor IO settings
    profileA = dict((resolved.get('peerA') or {}).get('profile', {}) or {})
    profileB = dict((resolved.get('peerB') or {}).get('profile', {}) or {})
    delivery_conf = cli_profiles.get("delivery", {})
    try:
        SYSTEM_REFRESH_EVERY = int(delivery_conf.get("system_refresh_every_self_checks") or 3)
        if SYSTEM_REFRESH_EVERY <= 0:
            SYSTEM_REFRESH_EVERY = 3
    except Exception:
        SYSTEM_REFRESH_EVERY = 3

    # Conversation reset configuration
    reset_cfg = delivery_conf.get("conversation_reset") if isinstance(delivery_conf.get("conversation_reset"), dict) else {}
    conversation_reset_policy = str(reset_cfg.get("policy") or "compact").strip().lower()
    if conversation_reset_policy not in ("compact", "clear"):
        conversation_reset_policy = "compact"
    try:
        conversation_reset_interval = int(reset_cfg.get("interval_handoffs") or 0)
    except Exception:
        conversation_reset_interval = 0
    default_reset_mode = conversation_reset_policy if conversation_reset_policy in ("compact", "clear") else "compact"

    # Source AUX template from bound actor (agents.yaml); role may override rate
    aux_resolved = resolved.get('aux') or {}
    aux_command_template = str(aux_resolved.get('invoke_command') or '').strip()
    aux_command = aux_command_template
    aux_actor = str(aux_resolved.get('actor') or '').strip()
    aux_cwd = str(aux_resolved.get('cwd') or '.')
    aux_binding_box['cwd'] = aux_cwd
    rate_limit_per_minute = int(aux_resolved.get("rate_limit_per_minute") or 2)
    if rate_limit_per_minute <= 0:
        rate_limit_per_minute = 1
    aux_min_interval = 60.0 / rate_limit_per_minute
    # Aux on/off is derived from presence of roles.aux.actor (no separate mode flag)
    aux_mode = "on" if aux_actor else "off"

    # Merge input_mode per peer if provided
    imodes = cli_profiles.get("input_mode", {}) if isinstance(cli_profiles.get("input_mode", {}), dict) else {}
    if imodes.get("peerA"):
        profileA["input_mode"] = imodes.get("peerA")
    if imodes.get("peerB"):
        profileB["input_mode"] = imodes.get("peerB")

    # Console echo configuration removed (no stdin console).

    # Read inbox+NUDGE parameters (effective at startup)
    try:
        global MB_PULL_ENABLED, INBOX_DIRNAME, PROCESSED_RETENTION, SOFT_ACK_ON_MAILBOX_ACTIVITY
        MB_PULL_ENABLED = bool(delivery_conf.get("mailbox_pull_enabled", True))
        INBOX_DIRNAME = str(delivery_conf.get("inbox_dirname", "inbox"))
        PROCESSED_RETENTION = int(delivery_conf.get("processed_retention", 200))
        SOFT_ACK_ON_MAILBOX_ACTIVITY = bool(delivery_conf.get("soft_ack_on_mailbox_activity", False))
        INBOX_STARTUP_POLICY = str(delivery_conf.get("inbox_startup_policy", "resume") or "resume").strip().lower()
        INBOX_STARTUP_PROMPT = bool(delivery_conf.get("inbox_startup_prompt", False))
        nudge_api.configure({
            'NUDGE_RESEND_SECONDS': float(delivery_conf.get("nudge_resend_seconds", 90)),
            'NUDGE_JITTER_PCT': float(delivery_conf.get("nudge_jitter_pct", 0.0) or 0.0),
            'NUDGE_DEBOUNCE_MS': float(delivery_conf.get("nudge_debounce_ms", 1500.0)),
            'NUDGE_PROGRESS_TIMEOUT_S': float(delivery_conf.get("nudge_progress_timeout_s", 45.0)),
            'NUDGE_KEEPALIVE': bool(delivery_conf.get("nudge_keepalive", True)),
            'NUDGE_BACKOFF_BASE_MS': float(delivery_conf.get("nudge_backoff_base_ms", 1000.0)),
            'NUDGE_BACKOFF_MAX_MS': float(delivery_conf.get("nudge_backoff_max_ms", 60000.0)),
            'NUDGE_MAX_RETRIES': float(delivery_conf.get("nudge_max_retries", 1.0)),
            'PROCESSED_RETENTION': PROCESSED_RETENTION,
        })
    except Exception:
        pass

    # Lazy preamble (applies to both console input and mailbox-driven inbound)
    LAZY = (delivery_conf.get("lazy_preamble") or {}) if isinstance(delivery_conf.get("lazy_preamble"), dict) else {}
    LAZY_ENABLED = bool(LAZY.get("enabled", True))

    def _preamble_state_path() -> Path:
        return state/"preamble_sent.json"
    def _load_preamble_sent() -> Dict[str,bool]:
        try:
            return json.loads(_preamble_state_path().read_text(encoding="utf-8"))
        except Exception:
            return {"PeerA": False, "PeerB": False}
    def _save_preamble_sent(st: Dict[str,bool]):
        try:
            _preamble_state_path().write_text(json.dumps(st, ensure_ascii=False, indent=2), encoding="utf-8")
        except Exception:
            pass
    def _maybe_prepend_preamble_inbox(receiver_label: str):
        """If first user message for this peer arrived via mailbox inbox, prepend preamble in-file."""
        if not LAZY_ENABLED:
            return
        try:
            st = _load_preamble_sent()
            if bool(st.get(receiver_label)):
                return
            ib = _inbox_dir(home, receiver_label)
            files = sorted([f for f in ib.iterdir() if f.is_file()], key=lambda p: p.name)
            if not files:
                return
            target = files[0]
            try:
                body = target.read_text(encoding='utf-8')
            except Exception:
                return
            # Only modify <FROM_USER> payloads; otherwise keep as-is
            m = re.search(r"<\s*FROM_USER\s*>\s*([\s\S]*?)<\s*/FROM_USER\s*>", body, re.I)
            if not m:
                return
            peer_key = "peerA" if receiver_label == "PeerA" else "peerB"
            pre = weave_preamble_text(home, peer_key)
            # If preamble text already present (e.g., injected by adapter), skip to avoid duplication
            try:
                if pre and pre.strip() and (pre.strip() in body):
                    st[receiver_label] = True
                    _save_preamble_sent(st)
                    return
            except Exception:
                pass
            inner = m.group(1)
            combined = f"<FROM_USER>\n{pre}\n\n{inner.strip()}\n</FROM_USER>\n"
            target.write_text(combined, encoding='utf-8')
            st[receiver_label] = True
            _save_preamble_sent(st)
            log_ledger(home, {"from":"system","kind":"lazy-preamble-sent","peer":receiver_label, "route":"mailbox"})
        except Exception:
            pass

    def _maybe_dispatch_foreman_message():
        """If foreman/to_peer.md contains a new message, route to Peer inbox(es) per To header and write sentinel.
        Also increments self-check counter for meaningful Foreman deliveries (per peer)."""
        try:
            base = home/"mailbox"/"foreman"
            f = base/"to_peer.md"
            if not f.exists():
                return
            raw = f.read_text(encoding='utf-8').strip()
            if not raw:
                return
            if is_sentinel_text(raw):
                return
            # parse routing header: To: Both|PeerA|PeerB (default Both)
            to_label = 'Both'
            lines0 = raw.splitlines()
            hdr_used = 0
            for i,l in enumerate(lines0[:4]):
                m = re.match(r"\s*To\s*:\s*(Both|PeerA|PeerB)\s*$", l, re.I)
                if m:
                    val = m.group(1).lower()
                    to_label = 'PeerA' if val=='peera' else ('PeerB' if val=='peerb' else 'Both')
                    hdr_used = max(hdr_used, i+1)
            body = parse_section(raw, "TO_PEER")
            if not body:
                body = "\n".join(lines0[hdr_used:]).strip()
            # ensure wrapper for inbox payload
            if not re.search(r"<\s*TO_PEER\s*>", body, re.I):
                body = f"<TO_PEER>\n{body}\n</TO_PEER>\n"
            # dispatch
            delivered_labels = []
            header_line = "To: Both" if to_label=='Both' else (f"To: {to_label}")
            # build unified FROM_USER payload preserving To header
            def _deliver(lbl: str):
                mid = new_mid("foreman")
                user_payload = f"<FROM_USER>\n{header_line}\n{body}\n</FROM_USER>\n"
                text_with_mid = wrap_with_mid(user_payload, mid)
                _write_inbox_message(home, lbl, text_with_mid, mid)
                delivered_labels.append(lbl)
            if to_label == 'Both':
                _deliver('PeerA'); _deliver('PeerB')
            elif to_label == 'PeerA':
                _deliver('PeerA')
            else:
                _deliver('PeerB')
            # sentinel
            try:
                eid = hashlib.sha1(body.encode('utf-8','ignore')).hexdigest()[:12]
            except Exception:
                eid = str(int(time.time()))
            tsz = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
            sha8 = sha256_text(body)[:8]
            route_lbl = 'Both' if to_label=='Both' else to_label
            sentinel = compose_sentinel(ts=tsz, eid=eid, sha8=sha8, route=f"Foreman→{route_lbl}")
            f.write_text(sentinel, encoding='utf-8')
            # optional user CC
            try:
                fc = _load_foreman_conf(); cc_u = bool(fc.get('cc_user', True))
                if cc_u:
                    if route_lbl == 'Both':
                        outbox_write(home, {"type":"to_user","peer": 'both', "from":"Foreman", "owner":"both", "text": body, "eid": eid})
                    else:
                        peer_key = 'peerA' if route_lbl=='PeerA' else 'peerB'
                        outbox_write(home, {"type":"to_user","peer": peer_key, "from":"Foreman", "owner": peer_key, "text": body, "eid": eid})
            except Exception:
                pass
            # Increment cadence and maybe inject full system via unified helper
            try:
                pl = (header_line + "\n" + body) if body else header_line
                handoff_api.maybe_selfcheck(delivered_labels, pl, True)
            except Exception:
                pass
        except Exception:
            pass
    def _maybe_prepend_preamble(receiver_label: str, user_payload: str) -> str:
        """If this is the first FROM_USER payload for a peer, prepend its preamble."""
        if not LAZY_ENABLED:
            return user_payload
        st = _load_preamble_sent()
        if bool(st.get(receiver_label)):
            return user_payload
        try:
            peer_key = "peerA" if receiver_label == "PeerA" else "peerB"
            pre = weave_preamble_text(home, peer_key)
            # Merge preamble into the first user message as one instruction block
            m = re.search(r"<\s*FROM_USER\s*>\s*([\s\S]*?)<\s*/FROM_USER\s*>", user_payload, re.I)
            inner = m.group(1) if m else user_payload
            combined = f"<FROM_USER>\n{pre}\n\n{inner.strip()}\n</FROM_USER>\n"
            st[receiver_label] = True
            _save_preamble_sent(st)
            log_ledger(home, {"from":"system","kind":"lazy-preamble-sent","peer":receiver_label})
            return combined
        except Exception:
            return user_payload

    # --- Foreman external cleanup helpers (terminate on orchestrator exit) ---
    def _foreman_stop_running(grace_seconds: float = 5.0) -> None:
        """
        Best-effort: terminate Foreman subprocess group (if any), wait a short grace,
        then SIGKILL. Also clears state.running and removes lock.
        """
        try:
            st = _foreman_load_state() or {}
            pid = int(st.get('pid') or 0)
            pgid = st.get('pgid')
        except Exception:
            pid, pgid = 0, None
        # Signal process group first (POSIX)
        try:
            if pgid:
                try:
                    os.killpg(int(pgid), signal.SIGTERM)
                except Exception:
                    pass
            elif pid:
                try:
                    os.kill(int(pid), signal.SIGTERM)
                except Exception:
                    pass
        except Exception:
            pass
        t_end = time.time() + max(0.0, float(grace_seconds))
        while time.time() < t_end:
            alive = False
            try:
                if pgid:
                    # Check any process in group by sending signal 0 to -pgid
                    os.killpg(int(pgid), 0)
                    alive = True
                elif pid:
                    os.kill(int(pid), 0)
                    alive = True
            except Exception:
                alive = False
            if not alive:
                break
            time.sleep(0.2)
        # Force kill if still alive
        try:
            if pgid:
                try:
                    os.killpg(int(pgid), signal.SIGKILL)
                except Exception:
                    pass
            elif pid:
                try:
                    os.kill(int(pid), signal.SIGKILL)
                except Exception:
                    pass
        except Exception:
            pass
        # Clear running + lock
        try:
            st = _foreman_load_state() or {}
            st['running'] = False
            _foreman_save_state(st)
        except Exception:
            pass
        try:
            lk = state/"foreman.lock"
            if lk.exists():
                lk.unlink()
        except Exception:
            pass

    # Prepare tmux session/panes
    if not tmux_session_exists(session):
        _,_ = tmux_new_session(session)
        # Ensure the detached session window uses our current terminal size (avoid 80x24 default)
        try:
            tsz = shutil.get_terminal_size(fallback=(160, 48))
            tmux("resize-window","-t",session,"-x",str(tsz.columns),"-y",str(tsz.lines))
        except Exception:
            pass
        # Create a dedicated UI window (index 1) and build layout there
        tmux("new-window","-t",session,"-n","ui")
        # Select UI window
        tmux("select-window","-t",f"{session}:1")
        # ALWAYS build 4-pane layout first (TUI/Log/PeerA/PeerB)
        # Single-peer layout adjustment happens later in launch command after TUI confirms settings
        pos = tmux_build_tui_layout(session, '1')
        # Fallback guard: ensure we actually have 4 panes
        try:
            code,_out,_ = tmux("list-panes","-t",session,"-F","#P")
            panes = (_out.strip().splitlines() if code==0 else [])
            if len(panes) < 4:
                # Create bottom-right pane explicitly then re-map
                tmux("split-window","-v","-t",pos.get('rt', f"{session}:1.1"))
                pos = tmux_build_tui_layout(session, '1')
                print("[TMUX] ensured 4 panes (right split)")
        except Exception:
            pass
        print("[TMUX] Built default 4-pane layout (TUI/Log/PeerA/PeerB)")
        left_top = pos['lt']
        left_bot = pos.get('lb', pos['lt'])
        right = pos['rt']
        paneA = pos['rt']
        paneB = pos.get('rb', right)
        # Note: single_peer layout adjustment will happen in launch command handler
        session_data = {"session":session,"ui_window":"1","left":left_top,"left_bottom":left_bot,"right":right,"single_peer":False,**pos}
        (state/"session.json").write_text(json.dumps(session_data, ensure_ascii=False, indent=2), encoding="utf-8")
    else:
        # Resize to current terminal as well to avoid stale small size from background server
        try:
            tsz = shutil.get_terminal_size(fallback=(160, 48))
            tmux("resize-window","-t",session,"-x",str(tsz.columns),"-y",str(tsz.lines))
        except Exception:
            pass
        # Ensure UI window exists; if not, create it
        code_w, out_w, _ = tmux("list-windows","-t",session,"-F","#{window_index} #{window_name}")
        have_ui = False
        if code_w == 0:
            for ln in out_w.strip().splitlines():
                try:
                    idx, name = ln.split(" ",1)
                    if name.strip() == 'ui': have_ui = True
                except Exception:
                    pass
        if not have_ui:
            tmux("new-window","-t",session,"-n","ui")
        tmux("select-window","-t",f"{session}:1")
        # ALWAYS build 4-pane layout (existing session case - shouldn't normally happen with fresh start)
        pos = tmux_build_tui_layout(session, '1')
        # Fallback guard: ensure 4 panes
        try:
            code,_out,_ = tmux("list-panes","-t",session,"-F","#P")
            panes = (_out.strip().splitlines() if code==0 else [])
            if len(panes) < 4:
                tmux("split-window","-v","-t",pos.get('rt', f"{session}:1.1"))
                pos = tmux_build_tui_layout(session, '1')
                print("[TMUX] ensured 4 panes (right split, existing)")
        except Exception:
            pass
        print("[TMUX] Built default 4-pane layout (existing session)")
        left_top = pos['lt']
        left_bot = pos.get('lb', pos['lt'])
        right = pos['rt']
        paneA = pos['rt']
        paneB = pos.get('rb', right)
        session_data = {"session":session,"ui_window":"1","left":left_top,"left_bottom":left_bot,"right":right,"single_peer":False,**pos}
        (state/"session.json").write_text(json.dumps(session_data, ensure_ascii=False, indent=2), encoding="utf-8")

    # Improve usability: larger history for all panes; keep mouse on but avoid binding wheel to copy-mode
    tmux("set-option","-g","mouse","on")
    # Let windows follow the size of the attached client aggressively
    tmux("set-window-option","-g","aggressive-resize","on")
    tmux("set-option","-g","history-limit","100000")
    # Optional: disable alternate-screen to keep scrollback (some CLIs toggle full-screen modes)
    try:
        tmux_cfg = cli_profiles.get("tmux", {}) if isinstance(cli_profiles.get("tmux", {}), dict) else {}
        if bool(tmux_cfg.get("alternate_screen_off", False)):
            tmux("set-option","-g","alternate-screen","off")
        else:
            tmux("set-option","-g","alternate-screen","on")
    except Exception:
        pass
    # Enable mouse wheel scroll: pass to app when in alternate screen (TUI), else use tmux copy-mode
    # This allows prompt_toolkit TUI to receive scroll events while preserving tmux scrollback for other panes
    tmux("bind-key", "-n", "WheelUpPane", "if-shell", "-Ft=", "#{alternate_on}", "send-keys -M", "copy-mode -e")
    tmux("bind-key", "-n", "WheelDownPane", "send-keys", "-M")
    
    # Cross-platform clipboard integration: sync tmux selection to system clipboard
    # Detect available clipboard tool and configure tmux copy-pipe accordingly
    clipboard_cmd = None
    if sys.platform == "darwin":
        clipboard_cmd = 'pbcopy'
    elif sys.platform == "linux":
        # WSL2: use clip.exe
        if shutil.which('clip.exe'):
            clipboard_cmd = 'clip.exe'
        # Native Linux: try wl-copy (Wayland), xclip (X11), or xsel
        elif shutil.which('wl-copy'):
            clipboard_cmd = 'wl-copy'
        elif shutil.which('xclip'):
            clipboard_cmd = 'xclip -selection clipboard'
        elif shutil.which('xsel'):
            clipboard_cmd = 'xsel --clipboard --input'
    elif sys.platform == "win32":
        clipboard_cmd = 'clip.exe'
    
    if clipboard_cmd:
        # Enable OSC 52 clipboard (modern tmux 3.2+)
        tmux("set-option", "-g", "set-clipboard", "on")
        # Mouse drag selection: copy to clipboard and cancel copy-mode
        tmux("bind-key", "-T", "copy-mode", "MouseDragEnd1Pane",
             f"send-keys -X copy-pipe-and-cancel '{clipboard_cmd}'")
        tmux("bind-key", "-T", "copy-mode-vi", "MouseDragEnd1Pane",
             f"send-keys -X copy-pipe-and-cancel '{clipboard_cmd}'")
        # vi-copy mode: 'y' to copy selection to clipboard
        tmux("bind-key", "-T", "copy-mode-vi", "y",
             f"send-keys -X copy-pipe-and-cancel '{clipboard_cmd}'")
    # Always start with 4-pane layout; single-peer adjustment happens after TUI confirms settings
    print(f"[INFO] Using tmux session: {session} (left-top=TUI / left-bottom=orchestrator log / right=PeerA+PeerB)")
    print(f"[INFO] pane map: left_top={left_top} left_bot={left_bot} PeerA(top)={paneA} PeerB(bottom)={paneB}")
    try:
        panes_data = {"left": left_top, "left_bottom": left_bot, "peerA": paneA, "peerB": paneB, "single_peer": False}
        (state/"panes.json").write_text(json.dumps(panes_data, ensure_ascii=False, indent=2), encoding='utf-8')
    except Exception:
        pass
    print(f"[TIP] tmux UI session ready: {session}. Use /quit or tmux commands to exit.")
    # Start TUI (no fallback)
    py = shlex.quote(sys.executable or 'python3')
    tui_py = shlex.quote(str(home/"cccc_tui.py"))
    ready_file = state/"tui.ready"
    try:
        if ready_file.exists():
            ready_file.unlink()
    except Exception:
        pass
    # Launch TUI (minimal, robust quoting) using the same interpreter; env inherits from tmux
    cmd_tui = f"{py} -u {tui_py} --home {shlex.quote(str(home))}"
    tmux_respawn_pane(left_top, cmd_tui)
    # Launch orchestrator log tail in left-bottom pane
    try:
        logp = shlex.quote(str(state/"orchestrator.log"))
        cmd_log = f"bash -lc 'printf \"[Orchestrator Log]\\n\"; tail -F {logp} 2>/dev/null || tail -f {logp}'"
        tmux_respawn_pane(left_bot, cmd_log)
    except Exception:
        pass
    # Ensure left pane (TUI) is focused for initial attach
    try:
        tmux("select-pane","-t",left_top)
    except Exception:
        pass
    # Wait for TUI ready: block indefinitely until .cccc/state/tui.ready appears.
    try:
        ready_file = state/"tui.ready"
        t0 = time.time()
        next_notice = 30.0
        while True:
            if ready_file.exists():
                break
            elapsed = time.time() - t0
            if elapsed >= next_notice:
                print(f"[INFO] Waiting for TUI readiness... {int(elapsed)}s elapsed.")
                next_notice += 30.0
            time.sleep(0.5)
    except Exception as exc:
        print(f"[ERROR] Failed while waiting for TUI readiness: {exc}")
        try:
            tmux("kill-session", "-t", session)
        except Exception:
            pass
        raise SystemExit(1)

    # IM command queue (bridge initiated)
    im_command_dir = state/"im_commands"
    im_command_processed = im_command_dir/"processed"
    try:
        im_command_dir.mkdir(parents=True, exist_ok=True)
        im_command_processed.mkdir(parents=True, exist_ok=True)
    except Exception:
        pass

    def _record_im_command_result(src_path: Path, request_id: str, result: Dict[str, Any]):
        try:
            im_command_processed.mkdir(parents=True, exist_ok=True)
        except Exception:
            pass
        res_path = im_command_processed/f"{request_id}.result.json"
        tmp = res_path.with_suffix('.tmp')
        try:
            tmp.write_text(json.dumps(result, ensure_ascii=False, indent=2), encoding='utf-8')
            tmp.replace(res_path)
        except Exception:
            pass
        archive_path = im_command_processed/f"{request_id}.req.json"
        try:
            src_path.replace(archive_path)
        except Exception:
            try:
                src_path.unlink()
            except Exception:
                pass

    def _process_im_commands():
        nonlocal aux_mode, deliver_paused
        try:
            files = sorted(im_command_dir.glob("*.json"))
        except Exception:
            return
        for fp in files:
            try:
                data = json.loads(fp.read_text(encoding='utf-8'))
            except Exception:
                data = {}
            request_id = str(data.get("request_id") or fp.stem)
            command = str(data.get("command") or "").lower().strip()
            source = str(data.get("source") or "im")
            args = data.get("args") or {}
            result: Dict[str, Any] = {"ok": False, "message": "unsupported"}
            try:
                if command == "focus":
                    raw = str(args.get("raw") or "")
                    _request_context_refresh(f"focus-{source}", hint=raw or None, force=True)
                    result = {"ok": True, "message": "Context refresh requested."}
                elif command == "reset":
                    mode = str(args.get("mode") or default_reset_mode)
                    message = _perform_reset(mode, trigger=source, reason=f"{source}-{mode}")
                    result = {"ok": True, "message": message}
                elif command == "aux":
                    action = str(args.get("action") or "").lower()
                    if action in ("", "status"):
                        last = aux_last_reason or "-"
                        cmd_display = aux_command or "-"
                        result = {"ok": True, "message": f"Aux status: mode={aux_mode}, command={cmd_display}, last_reason={last}"}
                    else:
                        result = {"ok": False, "message": "unsupported (only status)"}
                elif command == "aux_cli":
                    prompt_text = str(args.get("prompt") or "").strip()
                    if not prompt_text:
                        result = {"ok": False, "message": "Aux CLI prompt is empty."}
                    else:
                        rc, out, err, cmd_line = _run_aux_cli(prompt_text)
                        summary_lines = [f"[Aux CLI] exit={rc}", f"command: {cmd_line}"]
                        if out:
                            summary_lines.append("stdout:\n" + out.strip())
                        if err:
                            summary_lines.append("stderr:\n" + err.strip())
                        summary = "\n".join(summary_lines)
                        # Limit message length to avoid IM overflow
                        if len(summary) > 3500:
                            summary = summary[:3490] + "..."
                        result = {"ok": rc == 0, "message": summary, "returncode": rc}
                elif command == "review":
                    _send_aux_reminder("manual-review")
                    result = {"ok": True, "message": "Aux review reminder triggered"}
                elif command == "passthrough":
                    peer = str(args.get("peer") or "").lower()
                    text = str(args.get("text") or "").strip()
                    if not text:
                        raise ValueError("empty command text")
                    if peer in ("a", "peera", "peer_a"):
                        labels = ["PeerA"]
                    elif peer in ("b", "peerb", "peer_b"):
                        labels = ["PeerB"]
                    elif peer in ("both", "ab", "ba"):
                        labels = ["PeerA", "PeerB"]
                    else:
                        raise ValueError("unknown peer")
                    for label in labels:
                        _send_raw_to_cli(home, label, text, paneA, paneB)
                        try:
                            log_ledger(home, {"from": source, "kind": "im-passthrough", "to": label, "chars": len(text)})
                        except Exception:
                            pass
                    msg = f"Command sent to {' & '.join(labels)}"
                    result = {"ok": True, "message": msg}
                elif command == "foreman":
                    sub = str(args.get("action") or "").strip().lower()
                    outcome = foreman_scheduler.command(sub, origin=source)
                    if not isinstance(outcome, dict):
                        raise ValueError("foreman command returned invalid result")
                    result = dict(outcome)
                elif command == "restart":
                    target = str(args.get("target") or "both").strip().lower()
                    results = []
                    if target in ('peera', 'a', 'both'):
                        success = restart_peer('PeerA', reason='manual-im')
                        results.append(f"PeerA: {'✓' if success else '✗'}")
                    if target in ('peerb', 'b', 'both'):
                        success = restart_peer('PeerB', reason='manual-im')
                        results.append(f"PeerB: {'✓' if success else '✗'}")
                    msg = f"Restart {target}: {', '.join(results)}"
                    result = {"ok": True, "message": msg}
                elif command == "pause":
                    deliver_paused = True
                    deliver_paused_box['v'] = True
                    write_status(deliver_paused)
                    result = {"ok": True, "message": "Handoff paused. Messages will be saved to inbox but not delivered."}
                elif command == "resume":
                    deliver_paused = False
                    deliver_paused_box['v'] = False
                    write_status(deliver_paused)
                    # Check inbox and send NUDGE if there are pending messages
                    try:
                        for label in ('PeerA', 'PeerB'):
                            inbox = _inbox_dir(home, label)
                            if inbox.exists() and any(inbox.iterdir()):
                                pane = paneA if label == 'PeerA' else paneB
                                prof = profileA if label == 'PeerA' else profileB
                                nudge_api.maybe_send_nudge(home, label, pane, prof, force=True)
                    except Exception:
                        pass
                    result = {"ok": True, "message": "Handoff resumed."}
                else:
                    raise ValueError("unknown command")
            except Exception as exc:
                result = {"ok": False, "message": str(exc)}
            result.update({"command": command or "unknown", "request_id": request_id, "source": source, "ts": time.strftime('%Y-%m-%d %H:%M:%S')})
            _record_im_command_result(fp, request_id, result)


    # --- TUI command inbox (fast path, polled ~200ms within main loop) ---
    try:
        from .orchestrator.command_queue import init_command_offsets, append_command_result
    except ImportError:
        from orchestrator.command_queue import init_command_offsets, append_command_result
    commands_path = state/"commands.jsonl"
    commands_path.parent.mkdir(parents=True, exist_ok=True)
    commands_scan_path = state/"commands.scan.json"
    # Only track the canonical commands.jsonl; remove legacy typo with space
    commands_paths = [state/"commands.jsonl"]
    commands_last_pos_map: Dict[str, int] = init_command_offsets(commands_paths, commands_scan_path)
    if any(commands_last_pos_map.values()):
        print("[COMMANDS] Initialized command offsets.")
    shutdown_requested = False
    processed_command_ids: set[str] = set()

    def _inject_full_system():
        try:
            sysA = weave_system(home, "peerA"); sysB = weave_system(home, "peerB")
            _send_handoff("System", "PeerA", f"<FROM_SYSTEM>\n{sysA}\n</FROM_SYSTEM>\n")
            _send_handoff("System", "PeerB", f"<FROM_SYSTEM>\n{sysB}\n</FROM_SYSTEM>\n")
            return True, "SYSTEM injected to both peers"
        except Exception as e:
            return False, f"inject failed: {e}"

    # moved into .orchestrator.command_queue_runtime
    # PROJECT.md bootstrap: avoid blocking TTY prompts in tmux/TUI runs
    project_md_path = Path.cwd()/"PROJECT.md"
    project_md_exists = project_md_path.exists()
    # Non-interactive default: do not prompt; continue with current files
    start_mode = "has_doc" if project_md_exists else "has_doc"

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

    launcher_api = make_launcher({
        'home': home,
        'state': state,
        'paneA': paneA,
        'paneB': paneB,
        'tmux': tmux,
        'tmux_start_interactive': tmux_start_interactive,
        'profileA': profileA,
        'profileB': profileB,
        'outbox_write': outbox_write,
        'inbox_dir': _inbox_dir,
        'processed_dir': _processed_dir,
        'ensure_mailbox': ensure_mailbox,
        'log_ledger': log_ledger,
        'processed_retention': PROCESSED_RETENTION,
        'inbox_policy': INBOX_STARTUP_POLICY,
        # Console input disabled; do not pass read_console_line_timeout
        'cli_profiles': cli_profiles,
        'mb_pull_enabled': MB_PULL_ENABLED,
        'wait_for_ready': wait_for_ready,
        'commands_path': commands_path,
        'settings_confirmed_ready': _settings_confirmed_ready,
        'load_profiles': load_profiles,
    })
    auto_launch_pending, resolved = launcher_api.initial_setup(resolved, config_deferred, start_mode)

    # After initial injection, record capture lengths as the parsing baseline
    left_snap  = tmux_capture(paneA,  lines=800)
    right_snap = tmux_capture(paneB, lines=800)
    last_windows = {"PeerA": len(left_snap), "PeerB": len(right_snap)}
    dedup_user = {}
    dedup_peer = {}

    # Simplify: no hot-reload; changes to governance/policies/personas require restart

    # Initialize mailbox (do not clear inbox; honor startup policy)
    ensure_mailbox(home)
    mbox_idx = MailboxIndex(state)
    mbox_counts = {"peerA": {"to_user":0, "to_peer":0},
                   "peerB": {"to_user":0, "to_peer":0}}
    mbox_last = {"peerA": {"to_user": "-", "to_peer": "-"},
                 "peerB": {"to_user": "-", "to_peer": "-"}}
    # Track last mailbox activity per peer (used for timeout-based soft ACK)
    last_event_ts = {"PeerA": 0.0, "PeerB": 0.0}
    # No runtime handoff filter override (policy-only)
    # Minimalism: no session serialization; broadcast immediately; order governed by prompts

    # Handoff backpressure: maintain in-flight and waiting queues per receiver
    inflight: Dict[str, Optional[Dict[str,Any]]] = {"PeerA": None, "PeerB": None}
    queued: Dict[str, List[Dict[str,Any]]] = {"PeerA": [], "PeerB": []}
    # Simple resend de-bounce (hash payload; drop duplicates within a short window)
    recent_sends: Dict[str, List[Dict[str,Any]]] = {"PeerA": [], "PeerB": []}
    delivery_cfg = (cli_profiles.get("delivery", {}) or {})
    ack_timeout = float(delivery_cfg.get("ack_timeout_seconds", 30))
    resend_attempts = int(delivery_cfg.get("resend_attempts", 2))
    ack_require_mid = bool(delivery_cfg.get("ack_require_mid", False))
    duplicate_window = float(delivery_cfg.get("duplicate_window_seconds", 90))
    ack_mode = str(delivery_cfg.get("ack_mode", "ack_text")).strip().lower()
    # Main loop tick (poll interval)
    try:
        main_loop_tick_seconds = float(delivery_cfg.get("main_loop_tick_seconds", 2.0))
        if main_loop_tick_seconds < 0.2:
            main_loop_tick_seconds = 0.2
    except Exception:
        main_loop_tick_seconds = 2.0
    # Next keepalive (lightweight): delayed system echo back to sender to keep CLI alive
    keepalive_enabled = bool(delivery_cfg.get("keepalive_enabled", True))
    try:
        keepalive_delay_s = float(delivery_cfg.get("keepalive_delay_seconds", 60))
        if keepalive_delay_s < 5:
            keepalive_delay_s = 5.0
    except Exception:
        keepalive_delay_s = 60.0
    # Single-peer mode keepalive parameters (longer delay, limited nudges)
    single_peer_cfg = delivery_cfg.get("single_peer", {}) if isinstance(delivery_cfg.get("single_peer"), dict) else {}
    try:
        single_peer_delay_s = float(single_peer_cfg.get("keepalive_delay_seconds", 240))  # 4 minutes
        if single_peer_delay_s < 30:
            single_peer_delay_s = 30.0
    except Exception:
        single_peer_delay_s = 240.0
    try:
        single_peer_max_nudges = int(single_peer_cfg.get("keepalive_max_nudges", 3))
        if single_peer_max_nudges < 1:
            single_peer_max_nudges = 1
    except Exception:
        single_peer_max_nudges = 3
    pending_keepalive: Dict[str, Optional[Dict[str, Any]]] = {"PeerA": None, "PeerB": None}

    # Periodic self-check configuration
    _sc_every = int(delivery_cfg.get("self_check_every_handoffs", 0) or 0)
    self_check_enabled = _sc_every > 0
    self_check_every = max(1, _sc_every) if self_check_enabled else 0
    instr_counter = 0  # global sum for backward status
    in_self_check = False
    handoffs_peer = {"PeerA": 0, "PeerB": 0}
    self_checks_done = {"PeerA": 0, "PeerB": 0}
    # Self-check text: read from config only (no fallback in code)
    # Purpose: Strategic-level thinking calibration to prevent tunnel vision
    # Config: settings/cli_profiles.yaml -> delivery.self_check_text
    self_check_text = str(delivery_cfg.get("self_check_text") or "").strip()
    if not self_check_text:
        # Minimal inline fallback only if config is completely empty
        self_check_text = (
            "[Self-check] Off-course or blocked? Are milestone/task/step updates accurate? "
            "PeerA: check missing/obsolete milestones/tasks; PeerB: flag gaps. "
            "Keep sketch static (no TODO/progress). Answer briefly."
        )

    auto_reset_interval_cfg = conversation_reset_interval
    reset_interval_effective = auto_reset_interval_cfg if auto_reset_interval_cfg > 0 else 0
    # Aux review prompt for self-checks (only when aux mode is on)
    aux_review_prompt = ""
    try:
        if aux_mode == "on":
            aux_review_prompt = (
                "Aux available: invoke for heavy tasks (tests/reviews/transforms)."
            )
    except Exception:
        pass

    def _receiver_map(name: str) -> Tuple[str, Dict[str,Any]]:
        if name == "PeerA":
            return paneA, profileA
        return paneB, profileB

    # Pane idle judges for optional soft-ACK
    judges: Dict[str, PaneIdleJudge] = {"PeerA": PaneIdleJudge(profileA), "PeerB": PaneIdleJudge(profileB)}

    # Track inbox filenames to detect file-move ACKs (file_move mode)
    def _list_inbox_files(label: str) -> List[str]:
        try:
            ib = _inbox_dir(home, label)
            return sorted([f.name for f in ib.iterdir() if f.is_file()])
        except Exception:
            return []
    prev_inbox: Dict[str, List[str]] = {"PeerA": _list_inbox_files("PeerA"), "PeerB": _list_inbox_files("PeerB")}

    keepalive_ctx = {
        'home': home,
        'pending': pending_keepalive,
        'enabled': keepalive_enabled,
        'delay_s': keepalive_delay_s,
        'inflight': inflight,
        'queued': queued,
        'list_inbox_files': _list_inbox_files,
        'profileA': profileA,
        'profileB': profileB,
        'log_ledger': log_ledger,
        'keepalive_debug': KEEPALIVE_DEBUG,
        # Single-peer mode parameters
        'single_peer_mode': single_peer_mode,
        'single_peer_delay_s': single_peer_delay_s,
        'single_peer_max_nudges': single_peer_max_nudges,
        # Direct pane access for keepalive (no inbox write)
        'paste_when_ready': paste_when_ready,
        'paneA': paneA,
        'paneB': paneB,
    }
    keepalive_api = make_keepalive(keepalive_ctx)

    def _mailbox_peer_name(peer_label: str) -> str:
        return "peerA" if peer_label == "PeerA" else "peerB"

    # (Defined earlier before startup)

    def _send_handoff(sender_label: str, receiver_label: str, payload: str, require_mid: Optional[bool]=None, *, nudge_text: Optional[str]=None):
        return handoff_api.send_handoff(sender_label, receiver_label, payload, require_mid, nudge_text=nudge_text)

    def _request_context_refresh(trigger: str, hint: Optional[str] = None, *, force: bool = False):
        nonlocal context_update_last_request
        now = time.time()
        if (not force) and (now - context_update_last_request) < 60.0:
            return
        lines = [
            f"Context update requested (trigger: {trigger}).",
            "Review execution status:",
            "- context/context.yaml: milestones (project phases), notes (lessons), references",
            "- context/tasks/T###.yaml: active tasks with steps and status",
            "For each active task: confirm goal, steps, and status. Update files directly or via MCP tools.",
            "Check for gaps: missing tasks, unowned work, new risks; create T###.yaml when planning new work.",
            "If everything is current, reply in to_peer.md with 1-2 verified points."
        ]
        if hint:
            lines.append(f"Hint: {hint}")
        body = "\n".join(lines)
        payload = f"<FROM_SYSTEM>\n{body}\n</FROM_SYSTEM>\n"
        _send_handoff("System", "PeerA", payload)
        context_update_last_request = now
        log_ledger(home, {"from": "system", "kind": "context-refresh", "trigger": trigger, "hint": hint or ""})

    def _ack_receiver(label: str, event_text: Optional[str] = None):
        # ACK policy:
        # - If ack_require_mid=True: confirm only when event text contains [MID: *]
        # - If ack_require_mid=False: treat any event as ACK (compat with CLIs that don’t echo MID strictly)
        infl = inflight.get(label)
        if not infl:
            return
        if event_text:
            # Per-message MID enforcement: confirm only when require_mid=False or when event contains MID
            need_mid = bool(infl.get('require_mid', False))
            if (not need_mid) or (str(infl.get("mid","")) in event_text):
                cur_mid = infl.get("mid")
                inflight[label] = None
                # Clean up entries in queue with the same mid (e.g., requeued after timeout)
                if queued[label]:
                    queued[label] = [q for q in queued[label] if q.get("mid") != cur_mid]
                if queued[label]:
                    nxt = queued[label].pop(0)
                    _send_handoff(nxt.get("sender","System"), label, nxt.get("payload",""))

    def _resend_timeouts():
        now = time.time()
        for label, infl in list(inflight.items()):
            if not infl:
                continue
            eff_timeout = ack_timeout
            eff_resend = resend_attempts
            # Soft-ACK: if receiver pane is idle, consider delivery successful
            pane, prof = _receiver_map(label)
            idle, _r = judges[label].refresh(pane)
            # Do not treat "pane idle" as ACK anymore to avoid false positives
            # Still allow strong ACK via [MID]
            if now - infl.get("ts", 0) >= eff_timeout:
                if int(infl.get("attempts", 0)) < eff_resend:
                    mid = infl.get("mid"); payload = infl.get("payload")
                    status, out_mid = deliver_or_queue(home, pane, _mailbox_peer_name(label), payload, prof, delivery_conf, mid=mid)
                    infl["attempts"] = int(infl.get("attempts", 0)) + 1
                    infl["ts"] = now
                    log_ledger(home, {"from": infl.get("sender"), "kind": "handoff-resend", "to": label, "status": status, "mid": out_mid})
                    print(f"[RESEND] {infl.get('sender')} → {label} (mid={out_mid}, attempt={infl['attempts']})")
                else:
                    # Exceeded retries: drop to avoid duplicate injection
                    kind = "handoff-timeout-drop"
                    log_ledger(home, {"from": infl.get("sender"), "kind": kind, "to": label, "mid": infl.get("mid")})
                    print(f"[TIMEOUT] handoff to {label} mid={infl.get('mid')} — {kind}")
                    inflight[label] = None
        # Also check delayed keepalives (coalesced per sender)
        keepalive_api.tick()

    def _try_send_from_queue(label: str):
        if inflight.get(label) is not None:
            return
        if not queued.get(label):
            return
        pane, prof = _receiver_map(label)
        idle, _r = judges[label].refresh(pane)
        if not idle:
            return
        nxt = queued[label].pop(0)
        _send_handoff(nxt.get("sender","System"), label, nxt.get("payload",""))

    # Wait for user input mode (no hard initial requirement)
    phase = "discovery"
    ctx = context_blob(policies, phase)
    # Simplify: do not pause handoff by default; let user /pause when needed
    deliver_paused = False
    deliver_paused_box = {'v': deliver_paused}
    # Foreman thread handle (non-overlapping)
    foreman_thread: Optional[threading.Thread] = None

    # Register graceful cleanup on exit/signals to avoid orphan Foreman and Bridge processes
    _cleanup_called = {"v": False}
    
    def _cleanup_bridges():
        """Terminate all bridge processes (telegram, slack, discord) on orchestrator exit."""
        cleanup_log = home / "state" / "cleanup.log"
        def _log(msg):
            # Don't print() - stdout may be closed when tmux session is destroyed
            try:
                with cleanup_log.open('a', encoding='utf-8') as f:
                    import datetime
                    ts = datetime.datetime.now().strftime('%Y-%m-%d %H:%M:%S')
                    f.write(f"[{ts}] {msg}\n")
            except Exception:
                pass
        
        _log("[CLEANUP] _cleanup_bridges() called")
        state = home / "state"
        def _find_pids_for(adapter_name: str) -> list[int]:
            pids: list[int] = []
            try:
                script_path = (home/"adapters"/f"bridge_{adapter_name}.py").resolve()
                sp = str(script_path)
                proc = Path('/proc')
                if not proc.exists():
                    return pids
                for d in proc.iterdir():
                    if not d.is_dir() or not d.name.isdigit():
                        continue
                    pid = int(d.name)
                    try:
                        cmdline = (d/"cmdline").read_bytes().decode('utf-8','ignore')
                        if sp in cmdline:
                            pids.append(pid)
                    except Exception:
                        continue
            except Exception:
                pass
            return pids
        for adapter in ['telegram', 'slack', 'discord']:
            pidf_name = f"bridge-{adapter}.pid" if adapter != 'telegram' else "telegram-bridge.pid"
            pidf = state / pidf_name
            _log(f"[CLEANUP] Checking {adapter}: {pidf}")
            # Resolve PID targets: pid from file + /proc matches
            targets: list[int] = []
            try:
                if pidf.exists():
                    try:
                        pid_file_pid = int(pidf.read_text(encoding='utf-8').strip() or '0')
                        if pid_file_pid > 0:
                            targets.append(pid_file_pid)
                    except Exception:
                        pass
                else:
                    _log(f"[CLEANUP] {adapter} PID file not found (will scan /proc)")
                for p in _find_pids_for(adapter):
                    if p not in targets:
                        targets.append(p)
                if not targets:
                    _log(f"[CLEANUP] {adapter} no targets found")
                    # remove stale pid/lock if present
                    try: pidf.unlink()
                    except Exception: pass
                    try: (state/f"{adapter}-bridge.lock").unlink()
                    except Exception: pass
                    continue
                _log(f"[CLEANUP] {adapter} targets: {targets}")
                for pid in targets:
                    if pid <= 0:
                        _log(f"[CLEANUP] {adapter} invalid PID {pid}")
                        continue
                    # Check alive
                    alive = True
                    try:
                        os.kill(pid, 0)
                    except (ProcessLookupError, OSError):
                        alive = False
                    if not alive:
                        _log(f"[CLEANUP] {adapter} process {pid} already dead")
                        continue
                    # Send SIGTERM to process group when possible
                    try:
                        _log(f"[CLEANUP] Sending SIGTERM to {adapter} process/group {pid}")
                        try:
                            pgid = os.getpgid(pid)
                            os.killpg(pgid, signal.SIGTERM)
                        except Exception:
                            os.kill(pid, signal.SIGTERM)
                        log_ledger(home, {"from":"system","kind":"bridge-stop","adapter":adapter,"pid":pid,"signal":"SIGTERM"})
                    except Exception as e:
                        _log(f"[CLEANUP] Failed to send SIGTERM to {adapter}: {e}")
                    # Wait briefly for graceful shutdown
                    import time
                    terminated = False
                    for i in range(30):
                        time.sleep(0.1)
                        try:
                            os.kill(pid, 0)
                        except (ProcessLookupError, OSError):
                            _log(f"[CLEANUP] {adapter} process {pid} terminated after {i*0.1:.1f}s")
                            terminated = True
                            break
                    if not terminated:
                        try:
                            _log(f"[CLEANUP] {adapter} process {pid} still alive, sending SIGKILL")
                            try:
                                pgid = os.getpgid(pid)
                                os.killpg(pgid, signal.SIGKILL)
                            except Exception:
                                os.kill(pid, signal.SIGKILL)
                            log_ledger(home, {"from":"system","kind":"bridge-stop","adapter":adapter,"pid":pid,"signal":"SIGKILL"})
                            _log(f"[CLEANUP] SIGKILL sent to {adapter} process {pid}")
                        except Exception as e:
                            _log(f"[CLEANUP] Failed to send SIGKILL to {adapter}: {e}")
                # After attempts, clean up files
                try:
                    pidf.unlink()
                except Exception:
                    pass
                try:
                    (state/f"{adapter}-bridge.lock").unlink()
                except Exception:
                    pass
            except Exception as e:
                try:
                    log_ledger(home, {"from":"system","kind":"bridge-cleanup-error","adapter":adapter,"error":str(e)[:200]})
                except Exception:
                    pass
    
    def _cleanup_on_exit(signum=None, frame=None):
        # Exit cleanup handler: stop foreman and bridges, record shutdown, then exit.
        if _cleanup_called["v"]:
            return
        _cleanup_called["v"] = True

        # Best-effort ledger entry (do not raise)
        try:
            log_ledger(home, {"from": "system", "kind": "shutdown", "note": "cleanup foreman and bridges"})
        except Exception:
            pass

        # Stop foreman gracefully
        try:
            _foreman_stop_running(grace_seconds=5.0)
        except Exception:
            pass

        # Stop IM bridges and remove stale PID files
        try:
            _cleanup_bridges()
        except Exception:
            pass

        # Join foreman thread if still alive
        try:
            if foreman_thread is not None and foreman_thread.is_alive():
                foreman_thread.join(timeout=3.0)
        except Exception:
            pass

        # Remove orchestrator PID file
        try:
            _remove_pid_file(state)
        except Exception:
            pass

        # Exit the process after cleanup to prevent restarts in main loop
        import sys
        sys.exit(0)

    # Tmux session health check
    def _check_tmux_alive() -> bool:
        """Check if the tmux session we're in still exists"""
        tmux_env = os.environ.get('TMUX')
        if not tmux_env:
            return True  # Not in tmux, always alive
        
        # TMUX format: /tmp/tmux-1000/default,12345,0
        parts = tmux_env.split(',')
        if len(parts) < 1:
            return True
        
        socket_path = parts[0]
        # Check if socket still exists - if not, tmux server is gone
        return os.path.exists(socket_path)

    import atexit
    try:
        atexit.register(_cleanup_on_exit)
        signal.signal(signal.SIGTERM, _cleanup_on_exit)
        signal.signal(signal.SIGINT, _cleanup_on_exit)
        signal.signal(signal.SIGHUP, _cleanup_on_exit)  # Critical: tmux sends SIGHUP on destroy-unattached
    except Exception:
        pass

    # Write initial status snapshot for panel
    # status writer/queue snapshot moved to .orchestrator.status
    try:
        from .orchestrator.status import make as make_status
    except ImportError:
        from orchestrator.status import make as make_status
    stapi = make_status({
        'home': home, 'state': state, 'session': session, 'policies': policies,
        'conversation_reset_policy': conversation_reset_policy,
        'default_reset_mode': default_reset_mode,
        'auto_reset_interval_cfg': auto_reset_interval_cfg,
        'reset_interval_effective': reset_interval_effective,
        'self_check_enabled': self_check_enabled,
        'self_check_every': self_check_every,
        'instr_counter_box': {'v': instr_counter},
        'handoffs_peer': handoffs_peer,
        '_aux_snapshot': _aux_snapshot,
        'cli_profiles': cli_profiles,
        'settings': settings,
        'resolved_box': {'v': resolved},
        '_bin_available': _bin_available,
        '_actors_available': _actors_available,
        '_inbox_dir': _inbox_dir,
        '_processed_dir': _processed_dir,
        '_load_foreman_conf': _load_foreman_conf,
        '_foreman_load_state': _foreman_load_state,
        'mbox_counts': mbox_counts, 'mbox_last': mbox_last,
        'phase': phase,
        'read_yaml': read_yaml,
        'queued': queued, 'inflight': inflight,
    })
    write_status = stapi.write_status
    write_queue_and_locks = stapi.write_queue_and_locks
    write_status(deliver_paused); write_queue_and_locks()

    # Initialize NUDGE de-dup and ACK de-dup state before first injection
    last_nudge_ts: Dict[str,float] = {"PeerA": 0.0, "PeerB": 0.0}
    seen_acks: Dict[str,set] = {"PeerA": set(), "PeerB": set()}

    # Console input disabled; orchestrator is driven by IM/TUI queues only.
    console_input_enabled = False

    ready_banner_printed = False

    def _announce_ready() -> None:
        nonlocal ready_banner_printed
        if ready_banner_printed:
            return
        print("\n[READY] Common: /a|/b|/both send; /pause|/resume handoff; /sys-refresh inject SYSTEM; /restart peera|peerb|both.")
        print("[TIP] Use IM for CLI passthrough (/pa,/pb). TUI has adjacent panes for direct typing.")
        try:
            sys.stdout.write("[READY] Type h or /help for command hints.\n> ")
            sys.stdout.flush()
        except Exception:
            pass
        if start_mode == "ai_bootstrap":
            print("[PROJECT] Selected AI bootstrap for PROJECT.md.")
        if start_mode == "has_doc":
            print("[PROJECT] Found PROJECT.md.")
        ready_banner_printed = True

    initial_settings_done = _settings_confirmed_ready()
    if not initial_settings_done:
        print("[SETUP] Awaiting initial settings confirmation from TUI...")
    else:
        print("[SETUP] Initial settings already confirmed prior to orchestrator start.")
        _announce_ready()

    # last_windows/dedup_* initialized after handshake

    # Bind external handoff API now that dependencies are ready
    try:
        from .orchestrator.handoff import make as make_handoff
        from .orchestrator.events import make as make_events
        from .orchestrator.bridge_runtime import make as make_bridge_runtime
        from .orchestrator.auto_compact import make as make_auto_compact
    except ImportError:
        from orchestrator.handoff import make as make_handoff
        from orchestrator.events import make as make_events
        from orchestrator.bridge_runtime import make as make_bridge_runtime
        from orchestrator.auto_compact import make as make_auto_compact
    handoff_ctx = {
        'home': home,
        'paneA': paneA, 'paneB': paneB,
        'profileA': profileA, 'profileB': profileB,
        'policies': policies,
        'inflight': inflight, 'queued': queued,
        'recent_sends': recent_sends, 'duplicate_window': duplicate_window,
        'aux_mode': aux_mode, 'last_nudge_ts': last_nudge_ts,
        'schedule_keepalive': keepalive_api.schedule_from_payload,
        'is_low_signal': is_low_signal,
        'request_context_refresh': _request_context_refresh,
        'new_mid': new_mid, 'wrap_with_mid': wrap_with_mid,
        'maybe_send_nudge': nudge_api.maybe_send_nudge, 'send_nudge': nudge_api.send_nudge,
        'self_check_enabled': self_check_enabled,
        'self_check_every': self_check_every,
        'self_check_text': self_check_text,
        'aux_review_prompt': aux_review_prompt,
        'in_self_check': {'v': in_self_check},
        'handoffs_peer': handoffs_peer,
        'self_checks_done': self_checks_done,
        'send_handoff': None,
        'system_refresh_every': SYSTEM_REFRESH_EVERY,
        'processed_dir': _processed_dir,
        'processed_retention': PROCESSED_RETENTION,
        'deliver_paused_box': deliver_paused_box,
    }
    # Helper to get actor name for a peer
    def _get_peer_actor(peer: str) -> str:
        try:
            if peer == 'PeerA':
                return str((resolved.get('peerA') or {}).get('actor') or '')
            elif peer == 'PeerB':
                return str((resolved.get('peerB') or {}).get('actor') or '')
            return ''
        except Exception:
            return ''

    # Auto-compact API (idle-detection based context compression)
    auto_compact_ctx = {
        'home': home,
        'actors': actors,
        'delivery_conf': delivery_conf,
        'get_peer_actor': _get_peer_actor,
        'inflight': inflight,
        'queued': queued,
        'send_raw_to_cli': _send_raw_to_cli,
        'paneA': paneA,
        'paneB': paneB,
        'log_ledger': log_ledger,
    }
    auto_compact_api = make_auto_compact(auto_compact_ctx)

    # Add auto-compact callback to handoff context
    handoff_ctx['auto_compact_on_handoff'] = auto_compact_api.on_handoff_delivered

    handoff_api = make_handoff(handoff_ctx)
    handoff_ctx['send_handoff'] = handoff_api.send_handoff
    _send_handoff = handoff_api.send_handoff
    events_api = make_events({'home': home, 'send_handoff': _send_handoff})
    bridge_rt = make_bridge_runtime({'home': home, 'log_ledger': log_ledger, 'read_yaml': read_yaml})

    def _refresh_role_profiles(new_resolved: Dict[str, Any]):
        nonlocal resolved, profileA, profileB, aux_resolved, aux_command_template, aux_command, aux_actor, aux_cwd, aux_mode, rate_limit_per_minute, aux_min_interval
        resolved = new_resolved
        new_profileA = dict((resolved.get('peerA') or {}).get('profile', {}) or {})
        new_profileB = dict((resolved.get('peerB') or {}).get('profile', {}) or {})
        profileA.clear(); profileA.update(new_profileA)
        profileB.clear(); profileB.update(new_profileB)
        if imodes.get("peerA"):
            profileA["input_mode"] = imodes.get("peerA")
        if imodes.get("peerB"):
            profileB["input_mode"] = imodes.get("peerB")
        aux_resolved = resolved.get('aux') or {}
        aux_command_template = str(aux_resolved.get('invoke_command') or '').strip()
        aux_command = aux_command_template
        aux_actor = str(aux_resolved.get('actor') or '').strip()
        aux_cwd = str(aux_resolved.get('cwd') or '.')
        aux_binding_box['cwd'] = aux_cwd
        rate_limit_per_minute = int(aux_resolved.get("rate_limit_per_minute") or 2)
        if rate_limit_per_minute <= 0:
            rate_limit_per_minute = 1
        aux_min_interval = 60.0 / rate_limit_per_minute
        aux_mode = "on" if aux_actor else "off"
        judges['PeerA'] = PaneIdleJudge(profileA)
        judges['PeerB'] = PaneIdleJudge(profileB)
        keepalive_ctx['profileA'] = profileA
        keepalive_ctx['profileB'] = profileB
        keepalive_ctx['aux_mode'] = aux_mode
        keepalive_ctx['aux_actor'] = aux_actor
        handoff_ctx['profileA'] = profileA
        handoff_ctx['profileB'] = profileB
        handoff_ctx['aux_mode'] = aux_mode

    console_state = {
        'deliver_paused': deliver_paused,
        'foreman_thread': foreman_thread,
    }
    foreman_scheduler = make_foreman_scheduler({
        'home': home,
        'state': state,
        'log_ledger': log_ledger,
        'load_conf': _load_foreman_conf,
        'save_conf': _save_foreman_conf,
        'load_state': _foreman_load_state,
        'save_state': _foreman_save_state,
        'stop_running': _foreman_stop_running,
        'run_once': _foreman_run_once,
        'console_state': console_state,
    })
    # Console command API removed; all commands come via TUI/IM queues.

    mailbox_api = make_mailbox_pipeline({
        'home': home,
        'scan_mailboxes': scan_mailboxes,
        'mbox_idx': mbox_idx,
        'print_block': print_block,
        'log_ledger': log_ledger,
        'outbox_write': outbox_write,
        'compose_sentinel': compose_sentinel,
        'sha256_text': sha256_text,
        'events_api': events_api,
        'ack_receiver': _ack_receiver,
        'should_forward': should_forward,
        'send_handoff': _send_handoff,
        'policies': policies,
        'state': state,
        'mbox_counts': mbox_counts,
        'mbox_last': mbox_last,
        'last_event_ts': last_event_ts,
        'write_status': write_status,
        'write_queue_and_locks': write_queue_and_locks,
        'deliver_paused_box': deliver_paused_box,
        'single_peer_mode': single_peer_mode,
        'schedule_keepalive': keepalive_api.schedule_from_payload,  # For single-peer mode
    })

    # PEER restart configuration
    AUTO_RESTART_ENABLED = True  # Enable automatic restart on crash
    AUTO_RESTART_MAX_ATTEMPTS = 3  # Max restarts in time window
    AUTO_RESTART_WINDOW_SEC = 600  # Time window (10 minutes)

    def count_recent_restarts(peer_label: str, window_sec: int) -> int:
        """Count recent restart events from ledger within time window"""
        try:
            ledger_path = home / "state" / "ledger.jsonl"
            if not ledger_path.exists():
                return 0

            now = time.time()
            cutoff = now - window_sec
            count = 0

            with ledger_path.open('r', encoding='utf-8') as f:
                for line in f:
                    try:
                        entry = json.loads(line)
                        if (entry.get("kind") == "peer-restarted" and
                            entry.get("peer") == peer_label and
                            float(entry.get("ts", 0)) >= cutoff):
                            count += 1
                    except Exception:
                        continue
            return count
        except Exception:
            return 0

    def restart_peer(peer_label: str, reason: str = "manual") -> bool:
        """
        Simple PEER restart: respawn CLI + inject SYSTEM prompt

        Design: New CLI is fresh session, let everything flow naturally.
        Only needs to respawn process and re-inject SYSTEM prompt.
        """
        try:
            pane = paneA if peer_label == "PeerA" else paneB

            # Read CLI command from last_launch.json
            try:
                launch_info = json.loads((state/"last_launch.json").read_text(encoding='utf-8'))
                peer_key = "peerA" if peer_label == "PeerA" else "peerB"
                cmd = launch_info[peer_key]["eff"]
            except Exception as e:
                log_ledger(home, {
                    "from": "system",
                    "kind": "peer-restart-failed",
                    "peer": peer_label,
                    "reason": "cannot read launch command",
                    "error": str(e)[:200]
                })
                return False

            # Respawn CLI process
            stderr_log = str(home / "logs" / f"{peer_label.lower()}.stderr")
            tmux_start_interactive(pane, cmd, stderr_log=stderr_log, remain_on_exit=True)

            log_ledger(home, {
                "from": "system",
                "kind": "peer-restarted",
                "peer": peer_label,
                "reason": reason,
                "ts": time.time()
            })
            print(f"[RESTART] {peer_label} restarted (reason: {reason})")

            # Wait for CLI to initialize (uses startup_wait_seconds from config)
            time.sleep(startup_wait_seconds)

            # Reset preamble flag so it gets re-injected with next message
            try:
                preamble_state = _load_preamble_sent()
                preamble_state[peer_label] = False
                _save_preamble_sent(preamble_state)
            except Exception:
                pass

            # Inject SYSTEM prompt immediately
            try:
                peer_key = "peerA" if peer_label == "PeerA" else "peerB"
                sys_prompt = weave_system(home, peer_key)
                _send_handoff("System", peer_label,
                             f"<FROM_SYSTEM>\n{sys_prompt}\n</FROM_SYSTEM>\n")
                print(f"[RESTART] {peer_label} SYSTEM prompt injected")
            except Exception as e:
                log_ledger(home, {
                    "from": "system",
                    "kind": "peer-restart-system-inject-failed",
                    "peer": peer_label,
                    "error": str(e)[:200]
                })

            return True

        except Exception as e:
            log_ledger(home, {
                "from": "system",
                "kind": "peer-restart-exception",
                "peer": peer_label,
                "error": str(e)[:200]
            })
            print(f"[RESTART] {peer_label} restart failed: {e}")
            return False

    # Initialize API error recovery module
    api_error_ctx = {
        'home': home,
        'tmux': tmux,
        'log_ledger': log_ledger,
        'outbox_write': outbox_write,
        'count_recent_restarts': count_recent_restarts,
        'restart_peer': restart_peer,
        'auto_restart_enabled': AUTO_RESTART_ENABLED,
        'auto_restart_max_attempts': AUTO_RESTART_MAX_ATTEMPTS,
        'auto_restart_window_sec': AUTO_RESTART_WINDOW_SEC,
    }
    api_error_api = make_api_error_recovery(api_error_ctx)

    # Tmux alive check: only check every N loops to minimize overhead
    _tmux_check_counter = 0
    _tmux_check_interval = 10  # Check every 10 loops (e.g., every 20 seconds if loop is 2s)

    # PEER health check: detect crashes and capture diagnostics on-demand
    _peer_health_counter = 0
    _peer_health_interval = 15  # Check every 15 loops (~30 seconds if loop is 2s)
    _peer_crash_detected = {'PeerA': False, 'PeerB': False}  # Track detected crashes to avoid re-capture

    while True:
        # Check if tmux session still exists (for destroy-unattached cleanup)
        # Only check every N loops to reduce overhead
        if _tmux_check_counter % _tmux_check_interval == 0:
            if not _check_tmux_alive():
                try:
                    log_ledger(home, {"from":"system","kind":"tmux_gone","note":"tmux session destroyed, cleaning up"})
                except Exception as e:
                    print(f"[ERROR] Failed to log tmux_gone: {e}")
                print("[TMUX] Session destroyed, initiating cleanup...")
                try:
                    _cleanup_on_exit()
                except Exception as e:
                    print(f"[ERROR] _cleanup_on_exit() failed: {e}")
                    import traceback
                    traceback.print_exc()
                # Force cleanup bridges even if _cleanup_on_exit failed
                try:
                    print("[TMUX] Force calling _cleanup_bridges()...")
                    _cleanup_bridges()
                except Exception as e:
                    print(f"[ERROR] _cleanup_bridges() failed: {e}")
                    import traceback
                    traceback.print_exc()
                break
        _tmux_check_counter += 1

        # PEER health check: detect crashes and capture pane output for diagnostics
        # Only check periodically to minimize overhead (zero-cost when peers are alive)
        if _peer_health_counter % _peer_health_interval == 0:
            for peer_label, pane in [('PeerA', paneA), ('PeerB', paneB)]:
                # Skip if already detected (avoid re-capturing same crash)
                if _peer_crash_detected[peer_label]:
                    continue

                try:
                    # Check if pane is dead (lightweight command, <1ms)
                    rc, out, _ = tmux("list-panes", "-t", pane, "-F", "#{pane_dead}")
                    if rc == 0 and out.strip() == "1":
                        # Pane is dead - capture crash diagnostics (one-time operation)
                        _peer_crash_detected[peer_label] = True

                        # Capture last 1000 lines from tmux pane buffer
                        try:
                            timestamp = time.strftime("%Y%m%d-%H%M%S")
                            crash_log = home / "logs" / f"{peer_label.lower()}_crash_{timestamp}.log"
                            crash_log.parent.mkdir(parents=True, exist_ok=True)

                            rc_cap, content, _ = tmux("capture-pane", "-t", pane, "-p", "-S", "-1000")
                            if rc_cap == 0 and content:
                                crash_log.write_text(content, encoding='utf-8')
                                log_ledger(home, {
                                    "from": "system",
                                    "kind": "peer-crash-detected",
                                    "peer": peer_label,
                                    "pane": pane,
                                    "crash_log": str(crash_log),
                                    "log_size_bytes": len(content)
                                })
                                print(f"[CRASH] {peer_label} pane dead - captured {len(content)} bytes to {crash_log}")
                            else:
                                # Log detection even if capture failed
                                log_ledger(home, {
                                    "from": "system",
                                    "kind": "peer-crash-detected",
                                    "peer": peer_label,
                                    "pane": pane,
                                    "note": "capture failed"
                                })
                                print(f"[CRASH] {peer_label} pane dead - capture failed")
                        except Exception as e:
                            log_ledger(home, {
                                "from": "system",
                                "kind": "peer-crash-detected",
                                "peer": peer_label,
                                "error": str(e)[:200]
                            })

                        # Auto-restart if enabled and within limit
                        if AUTO_RESTART_ENABLED:
                            recent_restarts = count_recent_restarts(peer_label, AUTO_RESTART_WINDOW_SEC)
                            if recent_restarts < AUTO_RESTART_MAX_ATTEMPTS:
                                # Attempt automatic restart
                                print(f"[AUTO-RESTART] {peer_label} attempting restart (attempt {recent_restarts + 1}/{AUTO_RESTART_MAX_ATTEMPTS})")
                                success = restart_peer(peer_label, reason="auto-crash")
                                if success:
                                    # Reset detection flag so we can detect future crashes
                                    _peer_crash_detected[peer_label] = False
                                else:
                                    print(f"[AUTO-RESTART] {peer_label} restart failed")
                            else:
                                # Reached restart limit - notify user
                                print(f"[AUTO-RESTART] {peer_label} restart limit reached ({AUTO_RESTART_MAX_ATTEMPTS} restarts in {AUTO_RESTART_WINDOW_SEC}s)")
                                log_ledger(home, {
                                    "from": "system",
                                    "kind": "peer-restart-limit-reached",
                                    "peer": peer_label,
                                    "attempts": recent_restarts,
                                    "window_sec": AUTO_RESTART_WINDOW_SEC
                                })
                                try:
                                    outbox_write(home, {
                                        "type": "to_user",
                                        "peer": "System",
                                        "text": f"🚨 {peer_label} crashed {AUTO_RESTART_MAX_ATTEMPTS} times in {AUTO_RESTART_WINDOW_SEC//60} minutes. Auto-restart disabled. Please investigate crash logs and restart manually with /restart {peer_label.lower()}."
                                    })
                                except Exception:
                                    pass
                    else:
                        # Pane is alive - check for API errors via module
                        try:
                            api_error_api.check_and_recover(peer_label, pane)
                        except Exception:
                            pass
                except Exception:
                    # Silently continue if health check fails
                    pass
        _peer_health_counter += 1

        # If startup was deferred due to unconfirmed settings or missing roles, trigger launch+resume when ready
        try:
            auto_launch_pending, resolved = launcher_api.tick(resolved, config_deferred)
        except Exception:
            pass
        # Keep it simple: no phase locks; send clear instructions at start; remove runtime SYSTEM hot-reload

        # Non-blocking loop: process any IM bridges & scan A/B mailbox outputs
        try:
            bridge_rt.ensure_telegram_running()
            bridge_rt.ensure_slack_running()
            bridge_rt.ensure_discord_running()
            bridge_rt.ensure_wecom_running()
        except Exception:
            pass
        _process_im_commands()
        # TUI command queue consumption via external runtime (returns updated flags)
        try:
            from .orchestrator.command_queue_runtime import make as make_cq
        except ImportError:
            from orchestrator.command_queue_runtime import make as make_cq
        cq_ctx = {
            'home': home, 'state': state, 'session': session,
            'paneA': paneA, 'paneB': paneB,
            'profileA': profileA, 'profileB': profileB,
            'settings': settings,
            'cli_profiles_path': cli_profiles_path,
            'startup_wait_seconds': startup_wait_seconds,
            'PROCESSED_RETENTION': PROCESSED_RETENTION,
            'single_peer_mode': single_peer_mode,
            'write_status': write_status,
            'weave_system': weave_system,
            'send_handoff': _send_handoff,
            'maybe_prepend_preamble': _maybe_prepend_preamble,
            'process_im_commands': _process_im_commands,
            'run_aux_cli': _run_aux_cli,
            'read_yaml': read_yaml,
            'write_yaml': _write_yaml,
            'load_profiles': load_profiles,
            'tmux': tmux,
            'tmux_start_interactive': tmux_start_interactive,
            'inbox_dir': _inbox_dir,
            'processed_dir': _processed_dir,
            'outbox_write': outbox_write,
            'commands_path': commands_path,
            'commands_paths': commands_paths,
            'commands_last_pos_map': commands_last_pos_map,
            'processed_command_ids': processed_command_ids,
            'resolved': resolved,
            # boxes for by-ref flags
            'deliver_paused_box': deliver_paused_box,
            'shutdown_requested_box': {'v': shutdown_requested},
            # needed for passthru
            '_send_raw_to_cli': _send_raw_to_cli,
            # cleanup function for /quit command to use before tmux kill-session
            'cleanup_bridges': _cleanup_bridges,
            # restart function for /restart command
            'restart_peer': restart_peer,
            # needed for /resume to send NUDGE for pending inbox messages
            'maybe_send_nudge': nudge_api.maybe_send_nudge,
        }
        cq = make_cq(cq_ctx)
        upd = cq.consume(max_items=20)
        deliver_paused = upd.get('deliver_paused', deliver_paused)
        console_state['deliver_paused'] = deliver_paused
        deliver_paused_box['v'] = deliver_paused
        shutdown_requested = upd.get('shutdown_requested', shutdown_requested)
        new_resolved = upd.get('resolved')
        if new_resolved is not None:
            _refresh_role_profiles(new_resolved)
        commands_last_pos_map = upd.get('commands_last_pos_map', commands_last_pos_map)

        if not initial_settings_done:
            if _settings_confirmed_ready():
                initial_settings_done = True
                _announce_ready()
                continue
            time.sleep(0.2)
            continue

        try:
            keepalive_api.tick()
        except Exception:
            pass
        # Auto-compact tick: check for idle peers ready for context compression
        try:
            auto_compact_api.tick()
        except Exception:
            pass
        # Handle ACK first: by mode (file_move watches moves; ack_text parses echoes)
        try:
            if ack_mode == 'file_move':
                for label in ("PeerA","PeerB"):
                    cur = _list_inbox_files(label)
                    prev = prev_inbox.get(label, [])
                    disappeared = [fn for fn in prev if fn not in cur]
                    if disappeared:
                        proc = _processed_dir(home, label)
                        for fn in disappeared:
                            ok = (proc/(fn)).exists()
                            seq = fn[:6]
                            try:
                                _dbg(f"[ACK-FILE] {label} seq={seq} file={fn} ok={bool(ok)}")
                                log_ledger(home, {"from":label,"kind":"ack-file","seq":seq,"file":fn,"ok":bool(ok)})
                                nudge_api.nudge_mark_progress(home, label, seq=seq)
                            except Exception:
                                pass
                    try:
                        added = [fn for fn in cur if fn not in prev]
                        if added:
                            fn0 = sorted(added)[0]
                            if ".cccc-" not in fn0:
                                seq = fn0[:6]
                                path0 = _inbox_dir(home, label)/fn0
                                preview = _safe_headline(path0)
                                suffix = nudge_api.compose_nudge_suffix_for(label, profileA=profileA, profileB=profileB, aux_mode=aux_mode, aux_actor=aux_actor)
                                custom = _compose_detailed_nudge(seq, preview, (_inbox_dir(home, label).as_posix()), suffix=suffix)
                                pane = paneA if label == "PeerA" else paneB
                                prof = profileA if label == "PeerA" else profileB
                                nudge_api.maybe_send_nudge(home, label, pane, prof, custom_text=custom, force=True)
                                try:
                                    last_nudge_ts[label] = time.time()
                                except Exception:
                                    pass
                            for fn in sorted(added):
                                if ".cccc-" in fn:
                                    continue
                                pth = _inbox_dir(home, label)/fn
                                try:
                                    text = pth.read_text(encoding='utf-8', errors='replace')
                                except Exception:
                                    text = ''
                                try:
                                    if handoff_api.maybe_selfcheck([label], text, True):
                                        break
                                except Exception:
                                    pass
                    except Exception:
                        pass
                    prev_inbox[label] = cur
            else:
                for label, pane in (("PeerA", paneA), ("PeerB", paneB)):
                    out = tmux_capture(pane, lines=800)
                    acks, _ = find_acks_from_output(out)
                    if not acks:
                        continue
                    inbox = _inbox_dir(home, label)
                    files = [f for f in inbox.iterdir() if f.is_file()]
                    for tok in acks:
                        if tok in seen_acks[label]:
                            continue
                        seen_acks[label].add(tok)
                        target = None
                        for f in files:
                            if tok in f.name:
                                target = f; break
                        if target:
                            try:
                                target.unlink()
                            except Exception:
                                pass
        except Exception:
            pass
        # Handle ACK first: by mode (file_move watches moves; ack_text parses echoes)
        try:
            if ack_mode == 'file_move':
                for label in ("PeerA","PeerB"):
                    cur = _list_inbox_files(label)
                    prev = prev_inbox.get(label, [])
                    # Detect disappeared files from inbox (consider ACK)
                    disappeared = [fn for fn in prev if fn not in cur]
                    if disappeared:
                        proc = _processed_dir(home, label)
                        for fn in disappeared:
                            ok = (proc/(fn)).exists()
                            seq = fn[:6]
                            try:
                                _dbg(f"[ACK-FILE] {label} seq={seq} file={fn} ok={bool(ok)}")
                                log_ledger(home, {"from":label,"kind":"ack-file","seq":seq,"file":fn,"ok":bool(ok)})
                                # Treat file movement as progress for NUDGE single-flight
                                nudge_api.nudge_mark_progress(home, label, seq=seq)
                            except Exception:
                                pass
                    # Detect newly arrived inbox files (external sources), send an immediate detailed NUDGE once per loop
                    try:
                        added = [fn for fn in cur if fn not in prev]
                        if added:
                            # Send one detailed nudge for the oldest newly added
                            fn0 = sorted(added)[0]
                            if ".cccc-" not in fn0:
                                seq = fn0[:6]
                                path0 = _inbox_dir(home, label)/fn0
                                preview = _safe_headline(path0)
                                # Refresh live profiles/aux for nudge suffix to avoid stale aux/keys
                                try:
                                    live = load_profiles(home)
                                    profA_live = (live.get('peerA') or {}).get('profile') or {}
                                    profB_live = (live.get('peerB') or {}).get('profile') or {}
                                    aux_live = live.get('aux') or {}
                                    aux_actor_live = str(aux_live.get('actor') or '').strip()
                                    aux_mode_live = 'on' if aux_actor_live else 'off'
                                except Exception:
                                    profA_live, profB_live = profileA, profileB
                                    aux_mode_live, aux_actor_live = aux_mode, aux_actor
                                suffix = nudge_api.compose_nudge_suffix_for(label, profileA=profA_live, profileB=profB_live, aux_mode=aux_mode_live, aux_actor=aux_actor_live)
                                custom = _compose_detailed_nudge(seq, preview, (_inbox_dir(home, label).as_posix()), suffix=suffix)
                                pane = paneA if label == "PeerA" else paneB
                                prof = profA_live if label == "PeerA" else profB_live
                                nudge_api.maybe_send_nudge(home, label, pane, prof, custom_text=custom, force=True)
                                try:
                                    last_nudge_ts[label] = time.time()
                                except Exception:
                                    pass
                            # Count all newly added files toward this peer's self-check cadence
                            for fn in sorted(added):
                                if ".cccc-" in fn:
                                    continue
                                pth = _inbox_dir(home, label)/fn
                                try:
                                    text = pth.read_text(encoding='utf-8', errors='replace')
                                except Exception:
                                    text = ''
                                # Reuse the same meaningful rules as handoff
                                try:
                                    if handoff_api.maybe_selfcheck([label], text, True):
                                        break  # one self-check per peer per loop
                                except Exception:
                                    pass
                    except Exception:
                        pass
                    prev_inbox[label] = cur
            else:
                for label, pane in (("PeerA", paneA), ("PeerB", paneB)):
                    out = tmux_capture(pane, lines=800)
                    acks, _ = find_acks_from_output(out)
                    if not acks:
                        continue
                    inbox = _inbox_dir(home, label)
                    files = [f for f in inbox.iterdir() if f.is_file()]
                    for tok in acks:
                        if tok in seen_acks[label]:
                            continue
                        ok = nudge_api.archive_inbox_entry(home, label, tok)
                        # Treat 'inbox-empty' or no files as benign ACKs to avoid loops
                        if (not ok) and (tok.strip().lower() in ("inbox-empty","empty","none") or len(files)==0):
                            ok = True
                        seen_acks[label].add(tok)
                        try:
                            _dbg(f"[ACK] {label} token={tok} ok={bool(ok)}")
                            log_ledger(home, {"from":label,"kind":"ack","token":tok,"ok":bool(ok)})
                            # Any ACK token implies progress; clear inflight
                            nudge_api.nudge_mark_progress(home, label)
                        except Exception:
                            pass
        except Exception:
            pass

        # Periodic NUDGE: when inbox non-empty and enough time has passed since the last reminder
        try:
            nowt = time.time()
            # Refresh live profiles and aux bindings once per loop to ensure correct mode/keys for keepalive
            try:
                live = load_profiles(home)
                profA_live = (live.get('peerA') or {}).get('profile') or {}
                profB_live = (live.get('peerB') or {}).get('profile') or {}
                aux_live = live.get('aux') or {}
                aux_actor_live = str(aux_live.get('actor') or '').strip()
                aux_mode_live = 'on' if aux_actor_live else 'off'
            except Exception:
                profA_live, profB_live = profileA, profileB
                aux_mode_live, aux_actor_live = aux_mode, aux_actor
            for label, pane in (("PeerA", paneA), ("PeerB", paneB)):
                inbox = _inbox_dir(home, label)
                files = sorted([f for f in inbox.iterdir() if f.is_file()], key=lambda p: p.name)
                if not files:
                    continue
                # Before nudging the peer to read the first message, ensure lazy preamble is prepended once
                _maybe_prepend_preamble_inbox(label)
                # Coalesced NUDGE: send only when needed; backoff otherwise
                if label == "PeerA":
                    sent = nudge_api.maybe_send_nudge(
                        home, label, pane, profA_live,
                        suffix=nudge_api.compose_nudge_suffix_for('PeerA', profileA=profA_live, profileB=profB_live, aux_mode=aux_mode_live, aux_actor=aux_actor_live)
                    )
                else:
                    sent = nudge_api.maybe_send_nudge(
                        home, label, pane, profB_live,
                        suffix=nudge_api.compose_nudge_suffix_for('PeerB', profileA=profA_live, profileB=profB_live, aux_mode=aux_mode_live, aux_actor=aux_actor_live)
                    )
                if sent:
                    last_nudge_ts[label] = nowt
        except Exception:
            pass
        # Sleep to maintain main loop tick cadence, then process subsystems
        time.sleep(float(main_loop_tick_seconds))
        if shutdown_requested:
            break
        _stdout_saved = sys.stdout
        try:
            sys.stdout = io.StringIO()
            mailbox_api.process()
            _maybe_dispatch_foreman_message()
            foreman_scheduler.tick()
            _resend_timeouts()
            _try_send_from_queue("PeerA")
            _try_send_from_queue("PeerB")
        finally:
            sys.stdout = _stdout_saved
    # Graceful orchestrator shutdown
    # DO NOT kill tmux session - let TUI detect orchestrator exit and cleanup itself
    # This ensures prompt_toolkit can restore terminal state properly
    print(f"[END] Orchestrator exiting. TUI will detect shutdown and cleanup gracefully.")
    try:
        (state/"tui.ready").unlink()
    except Exception:
        pass
    print("\n[END] Recent commits:")
    run("git --no-pager log -n 5 --oneline")
    print("Ledger:", (home/"state/ledger.jsonl"))

    # Critical: Call cleanup to stop foreman/bridges and remove PID file
    # This ensures no zombie processes or stale files remain
    # _cleanup_on_exit will call sys.exit(0) after cleanup
    _cleanup_on_exit()


def _parse_args():
    import argparse
    parser = argparse.ArgumentParser(description="CCCC orchestrator")
    parser.add_argument("--home", default=".cccc", help="Path to .cccc directory")
    parser.add_argument("--session", help="Tmux session name")
    return parser.parse_args()


if __name__ == "__main__":
    cli_args = _parse_args()
    main(Path(cli_args.home).resolve(), session_name=cli_args.session)
