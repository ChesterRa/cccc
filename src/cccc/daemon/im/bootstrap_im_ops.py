"""IM bridge bootstrap helpers."""

from __future__ import annotations

import logging
import os
import subprocess
import sys
import time
from pathlib import Path

from .im_bridge_ops import read_live_im_bridge_pid, sanitize_im_bridge_env
from ...kernel.group import load_group
from ...util.conv import coerce_bool
from ...util.process import resolve_background_python_argv, supervised_process_popen_kwargs

logger = logging.getLogger("cccc.daemon.server")


def autostart_enabled_im_bridges(home: Path) -> None:
    """Autostart IM bridges marked enabled in group settings (best effort)."""
    base = home / "groups"
    if not base.exists():
        return

    for group_yaml in base.glob("*/group.yaml"):
        group_id = group_yaml.parent.name
        group = load_group(group_id)
        if group is None:
            continue

        im_cfg = group.doc.get("im") if isinstance(group.doc.get("im"), dict) else None
        if not isinstance(im_cfg, dict) or not coerce_bool(im_cfg.get("enabled"), default=False):
            continue

        platform = str(im_cfg.get("platform") or "telegram").strip() or "telegram"
        pid_path = group.path / "state" / "im_bridge.pid"

        if pid_path.exists():
            if read_live_im_bridge_pid(pid_path) is not None:
                continue

        state_dir = group.path / "state"
        try:
            state_dir.mkdir(parents=True, exist_ok=True)
        except Exception:
            pass
        log_path = state_dir / "im_bridge.log"

        try:
            with log_path.open("a", encoding="utf-8") as log_file:
                env = sanitize_im_bridge_env(os.environ.copy())
                env["CCCC_HOME"] = str(home)
                proc = subprocess.Popen(
                    resolve_background_python_argv([sys.executable, "-m", "cccc.ports.im.bridge", group_id, platform]),
                    env=env,
                    stdout=log_file,
                    stderr=log_file,
                    stdin=subprocess.DEVNULL,
                    cwd=str(home),
                    **supervised_process_popen_kwargs(),
                )
                time.sleep(0.25)
                rc = proc.poll()
                if rc is not None:
                    logger.warning(
                        "IM bridge autostart failed for %s (platform=%s, code=%s). See log: %s",
                        group_id,
                        platform,
                        rc,
                        log_path,
                    )
                    continue
                try:
                    pid_path.write_text(str(proc.pid), encoding="utf-8")
                except Exception:
                    pass
        except Exception as e:
            logger.warning("IM bridge autostart failed for %s (platform=%s): %s", group_id, platform, e)
