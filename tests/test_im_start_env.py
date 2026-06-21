from __future__ import annotations

import argparse
from pathlib import Path
from typing import Any, Dict
from unittest.mock import patch


class FakeGroup:
    def __init__(self, path: Path) -> None:
        self.path = path
        self.group_id = "g_env"
        self.doc: Dict[str, Any] = {
            "im": {
                "enabled": True,
                "platform": "dingtalk",
                "dingtalk_app_key": "app-key",
                "dingtalk_app_secret": "app-secret",
                "dingtalk_robot_code": "robot-code",
            }
        }

    def save(self) -> None:
        return None


def test_im_start_drops_inherited_python_ca_bundle_env(tmp_path, monkeypatch) -> None:
    from cccc.cli.im_cmds import cmd_im_start

    monkeypatch.setenv("SSL_CERT_FILE", "/bad/emsdk/certifi/cacert.pem")
    monkeypatch.setenv("REQUESTS_CA_BUNDLE", "/bad/requests.pem")
    monkeypatch.setenv("CURL_CA_BUNDLE", "/bad/curl.pem")
    captured: Dict[str, Any] = {}

    class FakeProcess:
        pid = 4242

        def poll(self) -> None:
            return None

    def fake_popen(*args: Any, **kwargs: Any) -> FakeProcess:
        captured["env"] = kwargs["env"]
        return FakeProcess()

    group = FakeGroup(tmp_path / "group")
    with (
        patch("cccc.cli.im_cmds._resolve_group_id", return_value="g_env"),
        patch("cccc.cli.im_cmds.load_group", return_value=group),
        patch("cccc.cli.im_cmds._im_find_bridge_pid", return_value=None),
        patch("cccc.cli.im_cmds._im_find_bridge_pids_by_script", return_value=[]),
        patch("cccc.cli.im_cmds.subprocess.Popen", side_effect=fake_popen),
    ):
        rc = cmd_im_start(argparse.Namespace(group="g_env"))

    env = captured["env"]
    assert rc == 0
    assert "SSL_CERT_FILE" not in env
    assert "REQUESTS_CA_BUNDLE" not in env
    assert "CURL_CA_BUNDLE" not in env
    assert env["DINGTALK_APP_KEY"] == "app-key"
    assert env["DINGTALK_APP_SECRET"] == "app-secret"
    assert env["DINGTALK_ROBOT_CODE"] == "robot-code"


def test_shared_im_bridge_env_sanitizer_drops_ca_bundle_overrides() -> None:
    from cccc.daemon.im.im_bridge_ops import sanitize_im_bridge_env

    env = sanitize_im_bridge_env({
        "SSL_CERT_FILE": "/bad/cert.pem",
        "REQUESTS_CA_BUNDLE": "/bad/requests.pem",
        "CURL_CA_BUNDLE": "/bad/curl.pem",
        "DINGTALK_APP_KEY": "app-key",
    })

    assert env == {"DINGTALK_APP_KEY": "app-key"}
