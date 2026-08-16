from pathlib import Path
from unittest.mock import patch

from cccc.ports.web.filesystem_suggestions import (
    existing_windows_drive_roots,
    recent_directory_suggestions,
)


def test_windows_drive_roots_keep_only_available_drives() -> None:
    roots = existing_windows_drive_roots(
        platform="nt",
        is_dir=lambda path: path in {"C:\\", "E:\\"},
    )

    assert roots == ["C:\\", "E:\\"]


def test_recent_suggestions_put_windows_drives_after_home(tmp_path: Path) -> None:
    with patch(
        "cccc.ports.web.filesystem_suggestions.existing_windows_drive_roots",
        return_value=["C:\\", "D:\\"],
    ):
        suggestions = recent_directory_suggestions(tmp_path, tmp_path)

    assert suggestions[:3] == [
        {"name": "Home", "path": str(tmp_path), "icon": "home"},
        {"name": "Drive C:", "path": "C:\\", "icon": "drive"},
        {"name": "Drive D:", "path": "D:\\", "icon": "drive"},
    ]
