from __future__ import annotations

import os
import secrets
from pathlib import Path
from typing import Optional

from ..paths import ensure_home
from .access_tokens import list_access_tokens

WEB_BOOTSTRAP_TOKEN_FILENAME = "web_bootstrap_token"


def ensure_web_bootstrap_token(home: Optional[Path] = None) -> Optional[Path]:
    root = Path(home) if home is not None else ensure_home()
    path = root / WEB_BOOTSTRAP_TOKEN_FILENAME
    if any(bool(item.get("is_admin")) for item in list_access_tokens(root)):
        path.unlink(missing_ok=True)
        return None
    if _valid_stored_token(path):
        try:
            path.chmod(0o600)
        except OSError:
            path.unlink(missing_ok=True)
            raise
        return path
    path.unlink(missing_ok=True)
    token = f"boot_{secrets.token_hex(16)}\n"
    try:
        descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError:
        return path
    try:
        os.write(descriptor, token.encode("utf-8"))
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    try:
        path.chmod(0o600)
    except OSError:
        path.unlink(missing_ok=True)
        raise
    return path


def consume_web_bootstrap_token(provided: str, home: Optional[Path] = None) -> bool:
    path = ensure_web_bootstrap_token(home)
    if path is None:
        return False
    try:
        expected = path.read_text(encoding="utf-8").strip()
    except FileNotFoundError:
        return False
    candidate = str(provided or "").strip()
    if not candidate or not secrets.compare_digest(candidate, expected):
        return False
    try:
        path.unlink()
    except FileNotFoundError:
        return False
    return True


def _valid_stored_token(path: Path) -> bool:
    if not path.is_file():
        return False
    try:
        return path.read_text(encoding="utf-8").strip().startswith("boot_")
    except OSError:
        return False
