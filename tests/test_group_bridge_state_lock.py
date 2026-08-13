from __future__ import annotations

from pathlib import Path

import pytest

from cccc.kernel.group_bridge import state_lock


def test_state_lock_reenters_for_equivalent_home_paths(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    real_home = tmp_path / "real-home"
    real_home.mkdir()
    alias_home = tmp_path / "alias-home"
    try:
        alias_home.symlink_to(real_home, target_is_directory=True)
    except OSError as exc:
        pytest.skip(f"directory symlinks are unavailable: {exc}")

    acquired: list[Path] = []
    released: list[object] = []
    handle = object()
    monkeypatch.setattr(
        state_lock,
        "acquire_lockfile",
        lambda path, *, blocking: acquired.append(path) or handle,
    )
    monkeypatch.setattr(state_lock, "release_lockfile", released.append)

    with state_lock.group_bridge_state_lock(alias_home):
        with state_lock.group_bridge_state_lock(real_home):
            assert len(acquired) == 1

    assert acquired == [(real_home / "group_bridge_state.lock").resolve()]
    assert released == [handle]
