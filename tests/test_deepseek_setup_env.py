from __future__ import annotations

from cccc.daemon.actors.deepseek_setup import ensure_deepseek_setup


def _ready(*_args, **_kwargs) -> str:
    return ""


def test_setup_enables_node_environment_proxy_by_default(tmp_path, monkeypatch) -> None:
    monkeypatch.delenv("NODE_USE_ENV_PROXY", raising=False)
    env = {"HOME": str(tmp_path), "PATH": ""}

    ensure_deepseek_setup(
        env,
        external_preflight=_ready,
        ready_preflight=_ready,
    )

    assert env["NODE_USE_ENV_PROXY"] == "1"


def test_setup_preserves_inherited_node_proxy_setting(tmp_path, monkeypatch) -> None:
    monkeypatch.setenv("NODE_USE_ENV_PROXY", "0")
    env = {"HOME": str(tmp_path), "PATH": ""}

    ensure_deepseek_setup(
        env,
        external_preflight=_ready,
        ready_preflight=_ready,
    )

    assert env["NODE_USE_ENV_PROXY"] == "0"


def test_setup_preserves_actor_node_proxy_setting(tmp_path, monkeypatch) -> None:
    monkeypatch.setenv("NODE_USE_ENV_PROXY", "1")
    env = {
        "HOME": str(tmp_path),
        "PATH": "",
        "NODE_USE_ENV_PROXY": "custom",
    }

    ensure_deepseek_setup(
        env,
        external_preflight=_ready,
        ready_preflight=_ready,
    )

    assert env["NODE_USE_ENV_PROXY"] == "custom"
