from __future__ import annotations

import os
import string
from collections.abc import Callable
from pathlib import Path
from typing import Any


def existing_windows_drive_roots(
    *,
    platform: str | None = None,
    is_dir: Callable[[str], bool] = os.path.isdir,
) -> list[str]:
    if (platform or os.name) != "nt":
        return []
    candidates = [f"{letter}:\\" for letter in string.ascii_uppercase]
    return [path for path in candidates if is_dir(path)]


def recent_directory_suggestions(home: Path, cwd: Path) -> list[dict[str, Any]]:
    suggestions: list[dict[str, Any]] = [
        {"name": "Home", "path": str(home), "icon": "home"}
    ]
    suggestions.extend(
        {"name": f"Drive {path[:2]}", "path": path, "icon": "drive"}
        for path in existing_windows_drive_roots()
    )
    for name in [
        "dev",
        "projects",
        "code",
        "src",
        "workspace",
        "repos",
        "github",
        "work",
    ]:
        path = home / name
        if path.is_dir():
            suggestions.append(
                {"name": name.title(), "path": str(path), "icon": "folder"}
            )
    for name, icon in [
        ("Desktop", "desktop"),
        ("Documents", "document"),
        ("Downloads", "download"),
    ]:
        path = home / name
        if path.is_dir():
            suggestions.append({"name": name, "path": str(path), "icon": icon})
    if cwd != home:
        suggestions.append({"name": "Current Dir", "path": str(cwd), "icon": "current"})
    return suggestions[:10]
