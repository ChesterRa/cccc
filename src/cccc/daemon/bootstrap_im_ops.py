"""IM bridge bootstrap helpers."""

from __future__ import annotations

import logging
import os
import subprocess
import sys
import time
from pathlib import Path

from ..kernel.group import load_group
from ..util.conv import coerce_bool

logger = logging.getLogger("cccc.daemon.server")


def _pid_alive(pid: int) -> bool:
    try:
        if pid <= 0:
            return False
        os.kill(pid, 0)
        return True
    except Exception:
        return False


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
            try:
                pid = int(pid_path.read_text(encoding="utf-8").strip())
            except Exception:
                pid = 0
            if pid > 0 and _pid_alive(pid):
                continue
            try:
                pid_path.unlink(missing_ok=True)
            except Exception:
                pass

        state_dir = group.path / "state"
        try:
            state_dir.mkdir(parents=True, exist_ok=True)
        except Exception:
            pass
        log_path = state_dir / "im_bridge.log"

        try:
            with log_path.open("a", encoding="utf-8") as log_file:
                env = os.environ.copy()
                env["CCCC_HOME"] = str(home)
                proc = subprocess.Popen(
                    [sys.executable, "-m", "cccc.ports.im.bridge", group_id, platform],
                    env=env,
                    stdout=log_file,
                    stderr=log_file,
                    stdin=subprocess.DEVNULL,
                    start_new_session=True,
                    cwd=str(home),
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
