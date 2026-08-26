from __future__ import annotations

import os
import socket
import subprocess
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from unittest.mock import patch

from cccc.daemon.ops import cloudflared_supervisor, membership_ops
from cccc.daemon.ops.membership_ops import (
    handle_membership_login,
    handle_membership_login_poll,
    handle_membership_logout,
    handle_membership_reach_off,
    handle_membership_reach_on,
    handle_membership_status,
    set_account_transport_for_tests,
    set_reach_command_for_tests,
)
from cccc.daemon.ops.remote_access_ops import (
    handle_remote_access_configure,
    handle_remote_access_state,
)
from cccc.kernel.access_tokens import create_access_token
from cccc.kernel.membership import LOGOUT_WARNING, load_membership, save_membership
from cccc.kernel.settings import (
    get_remote_access_settings,
    update_remote_access_settings,
)
from tests.test_membership_account import FakeAccount
from cccc.ports.web.runtime_control import write_web_runtime_state


class TestMembershipOps(unittest.TestCase):
    def setUp(self) -> None:
        self._old_home = os.environ.get("CCCC_HOME")
        self._old_origin = os.environ.get("CCCC_ACCOUNT_ORIGIN")
        self._old_unauth = os.environ.get("CCCC_WEB_ALLOW_UNAUTHENTICATED")
        self._old_timeout = os.environ.get("CCCC_ACCOUNT_TIMEOUT_S")
        self._tmp = tempfile.TemporaryDirectory()
        os.environ["CCCC_HOME"] = self._tmp.name
        os.environ.pop("CCCC_ACCOUNT_ORIGIN", None)
        os.environ.pop("CCCC_WEB_ALLOW_UNAUTHENTICATED", None)
        os.environ.pop("CCCC_ACCOUNT_TIMEOUT_S", None)
        set_account_transport_for_tests(None)
        set_reach_command_for_tests(None)

    def tearDown(self) -> None:
        cloudflared_supervisor.stop()
        set_account_transport_for_tests(None)
        set_reach_command_for_tests(None)
        self._tmp.cleanup()
        if self._old_home is None:
            os.environ.pop("CCCC_HOME", None)
        else:
            os.environ["CCCC_HOME"] = self._old_home
        if self._old_origin is None:
            os.environ.pop("CCCC_ACCOUNT_ORIGIN", None)
        else:
            os.environ["CCCC_ACCOUNT_ORIGIN"] = self._old_origin
        if self._old_unauth is None:
            os.environ.pop("CCCC_WEB_ALLOW_UNAUTHENTICATED", None)
        else:
            os.environ["CCCC_WEB_ALLOW_UNAUTHENTICATED"] = self._old_unauth
        if self._old_timeout is None:
            os.environ.pop("CCCC_ACCOUNT_TIMEOUT_S", None)
        else:
            os.environ["CCCC_ACCOUNT_TIMEOUT_S"] = self._old_timeout

    def _record_live_web(self, port: int = 0) -> int:
        runtime_id = "web_membership_test"

        class ReadyHandler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                if self.path != "/api/v1/ready":
                    self.send_error(404)
                    return
                body = (
                    '{"ok":true,"result":{"web":"ready","runtime_id":"'
                    + runtime_id
                    + '"}}'
                ).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, _format: str, *_args: object) -> None:
                return

        server = ThreadingHTTPServer(("127.0.0.1", port), ReadyHandler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()

        def stop_server() -> None:
            server.shutdown()
            thread.join(1.0)
            server.server_close()

        self.addCleanup(stop_server)
        live_port = int(server.server_address[1])
        write_web_runtime_state(
            pid=os.getpid(),
            host="127.0.0.1",
            port=live_port,
            mode="normal",
            supervisor_managed=True,
            supervisor_pid=None,
            launch_source="test",
            runtime_id=runtime_id,
        )
        return live_port

    def test_reach_origin_rejects_a_recorded_port_that_is_not_listening(self) -> None:
        listener = socket.socket()
        listener.bind(("127.0.0.1", 0))
        port = int(listener.getsockname()[1])
        listener.close()
        write_web_runtime_state(
            pid=os.getpid(),
            host="127.0.0.1",
            port=port,
            mode="normal",
            supervisor_managed=True,
            supervisor_pid=None,
            launch_source="test",
        )

        with self.assertRaisesRegex(RuntimeError, "did not prove its runtime identity"):
            membership_ops._live_web_port()

    def test_reach_origin_rejects_a_listener_with_the_wrong_runtime_identity(self) -> None:
        port = self._record_live_web()
        runtime = membership_ops.read_web_runtime_state()
        write_web_runtime_state(
            pid=os.getpid(),
            host="127.0.0.1",
            port=port,
            mode="normal",
            supervisor_managed=True,
            supervisor_pid=None,
            launch_source="test",
            runtime_id=f"{runtime.get('runtime_id')}-wrong",
        )

        with self.assertRaisesRegex(RuntimeError, "did not prove its runtime identity"):
            membership_ops._live_web_port()

    def test_reach_origin_rejects_a_live_web_binding_that_cannot_accept_loopback(
        self,
    ) -> None:
        write_web_runtime_state(
            pid=os.getpid(),
            host="192.0.2.10",
            port=8848,
            mode="normal",
            supervisor_managed=True,
            supervisor_pid=None,
            launch_source="test",
        )
        with self.assertRaisesRegex(RuntimeError, "127.0.0.1"):
            membership_ops._live_web_port()

    def test_status_is_logged_out_by_default(self) -> None:
        resp = handle_membership_status({"by": "user"})
        self.assertTrue(resp.ok)
        membership = resp.result["membership"]
        self.assertFalse(membership["logged_in"])
        self.assertIsNone(membership["hostname"])
        self.assertFalse(membership["online"])
        self.assertIsInstance(membership["reach_supported"], bool)
        self.assertNotIn("connector_url", membership)

    def test_status_rejects_non_user_callers_before_exposing_token_urls(self) -> None:
        create_access_token("admin", is_admin=True, custom_token="acc_admin_secret")
        save_membership({"logged_in": True, "hostname": "https://d-abc.example.test"})
        resp = handle_membership_status({"by": "peer1"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "permission_denied")

    def test_default_account_origin_is_operator_domain(self) -> None:
        from cccc.kernel.membership import DEFAULT_ACCOUNT_ORIGIN, account_origin

        self.assertEqual(account_origin(), DEFAULT_ACCOUNT_ORIGIN)
        self.assertEqual(DEFAULT_ACCOUNT_ORIGIN, "https://account.cccc.sh")

    def test_retired_account_origin_rewrites_to_operator_domain(self) -> None:
        from cccc.kernel.membership import DEFAULT_ACCOUNT_ORIGIN, account_origin

        os.environ["CCCC_ACCOUNT_ORIGIN"] = "https://account.cccc.foo"
        self.assertEqual(account_origin(), DEFAULT_ACCOUNT_ORIGIN)
        self.assertEqual(
            account_origin("https://account.cccc.foo/"), DEFAULT_ACCOUNT_ORIGIN
        )

    def test_login_with_origin_but_no_server_is_network(self) -> None:
        os.environ["CCCC_ACCOUNT_ORIGIN"] = "http://127.0.0.1:1"
        os.environ["CCCC_ACCOUNT_TIMEOUT_S"] = "0.3"
        resp = handle_membership_login({"by": "user"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "membership_network")

    def test_login_pending_then_poll_grants_device(self) -> None:
        os.environ["CCCC_ACCOUNT_ORIGIN"] = "https://account.test"
        account = FakeAccount()
        set_account_transport_for_tests(account)
        started = handle_membership_login({"by": "user"})
        self.assertTrue(started.ok)
        pending = started.result["membership"]["pending"]
        self.assertEqual(pending["user_code"], "WDJB-MJHT")
        self.assertIn("user_code=WDJB-MJHT", pending["verification_uri_complete"])
        self.assertFalse(started.result["membership"]["logged_in"])
        waiting = handle_membership_login_poll({"by": "user"})
        self.assertTrue(waiting.ok)
        self.assertFalse(waiting.result["membership"]["logged_in"])
        account.approved = True
        granted = handle_membership_login_poll({"by": "user"})
        self.assertTrue(granted.ok)
        self.assertTrue(granted.result["membership"]["logged_in"])
        self.assertEqual(granted.result["membership"]["device_id"], "d_abc")
        self.assertEqual(
            granted.result["membership"]["hostname"], "https://d-abc.example.test"
        )
        self.assertEqual(load_membership()["account_origin"], "https://account.test")
        replayed = handle_membership_login_poll({"by": "user"})
        self.assertTrue(replayed.ok)
        self.assertTrue(replayed.result["membership"]["logged_in"])

    def test_login_replays_an_unexpired_pending_grant(self) -> None:
        account = FakeAccount()
        set_account_transport_for_tests(account)

        started = handle_membership_login(
            {"by": "user", "account_origin": "https://issuer.example.test"}
        )
        replayed = handle_membership_login(
            {"by": "user", "account_origin": "https://wrong.example.test"}
        )

        self.assertTrue(started.ok)
        self.assertTrue(replayed.ok)
        self.assertEqual(
            replayed.result["membership"]["pending"],
            started.result["membership"]["pending"],
        )
        self.assertEqual(
            account.calls,
            [("POST", "https://issuer.example.test/v1/device/code")],
        )

    def _assert_terminal_login_can_restart(self, terminal_code: str) -> None:
        calls: list[str] = []

        def transport(method, url, headers, body, timeout_s):
            _ = method, headers, body, timeout_s
            calls.append(url)
            if url.endswith("/v1/device/code"):
                return 200, {
                    "device_code": f"dc-{calls.count(url)}",
                    "user_code": "FRESH-CODE",
                    "verification_uri": "https://issuer.example.test/device",
                    "expires_in": 600,
                    "interval": 1,
                }
            return 400, {"error": terminal_code}

        set_account_transport_for_tests(transport)
        started = handle_membership_login(
            {"by": "user", "account_origin": "https://issuer.example.test"}
        )
        self.assertTrue(started.ok)

        failed = handle_membership_login_poll({"by": "user"})
        self.assertFalse(failed.ok)
        self.assertIsNone(load_membership().get("pending_login"))

        restarted = handle_membership_login(
            {"by": "user", "account_origin": "https://issuer.example.test"}
        )
        self.assertTrue(restarted.ok)
        self.assertEqual(
            sum(url.endswith("/v1/device/code") for url in calls),
            2,
        )

    def test_denied_login_can_start_a_fresh_grant(self) -> None:
        self._assert_terminal_login_can_restart("access_denied")

    def test_expired_login_can_start_a_fresh_grant(self) -> None:
        self._assert_terminal_login_can_restart("expired_token")

    def test_login_issuer_remains_bound_after_the_daemon_environment_changes(
        self,
    ) -> None:
        account = FakeAccount()
        set_account_transport_for_tests(account)
        started = handle_membership_login(
            {"by": "user", "account_origin": "https://issuer.example.test"}
        )
        self.assertTrue(started.ok)
        os.environ["CCCC_ACCOUNT_ORIGIN"] = "https://wrong.example.test"
        waiting = handle_membership_login_poll({"by": "user"})
        self.assertTrue(waiting.ok)
        account.approved = True
        granted = handle_membership_login_poll({"by": "user"})
        self.assertTrue(granted.ok)
        self.assertEqual(
            {url.split("/v1/", 1)[0] for _method, url in account.calls},
            {"https://issuer.example.test"},
        )
        self.assertEqual(
            granted.result["membership"]["account_origin"],
            "https://issuer.example.test",
        )

    def test_reach_on_requires_admin_token(self) -> None:
        resp = handle_membership_reach_on({"by": "user"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "membership_gate")

    def test_reach_on_requires_login_after_admin_token(self) -> None:
        create_access_token("admin", is_admin=True)
        resp = handle_membership_reach_on({"by": "user"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "membership_not_logged_in")

    def test_reach_on_rejects_unauthenticated_override(self) -> None:
        os.environ["CCCC_WEB_ALLOW_UNAUTHENTICATED"] = "1"
        create_access_token("admin", is_admin=True)
        resp = handle_membership_reach_on({"by": "user"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "membership_gate")

    def test_reach_on_refuses_enabled_tailscale_provider(self) -> None:
        create_access_token("admin", is_admin=True)
        update_remote_access_settings(
            {"provider": "tailscale", "enabled": True, "web_host": "0.0.0.0"}
        )
        resp = handle_membership_reach_on({"by": "user"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "membership_gate")

    def test_reach_on_reports_disabled_device(self) -> None:
        create_access_token("admin", is_admin=True)
        save_membership(
            {
                "logged_in": True,
                "disabled": True,
                "device_id": "dev_test",
                "device_token": "tok",
            }
        )
        resp = handle_membership_reach_on({"by": "user"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "membership_disabled")

    def test_reach_on_starts_helper_and_writes_remote_access(self) -> None:
        os.environ["CCCC_ACCOUNT_ORIGIN"] = "https://account.test"
        account = FakeAccount()
        set_account_transport_for_tests(account)
        set_reach_command_for_tests(
            [sys.executable, "-c", "import time; time.sleep(30)"]
        )
        update_remote_access_settings(
            {
                "provider": "manual",
                "enabled": True,
                "web_port": 9000,
                "web_public_url": "https://manual.example.test",
            }
        )
        live_port = self._record_live_web()
        create_access_token("admin", is_admin=True, custom_token="acc_admin_fixture")
        save_membership(
            {
                "logged_in": True,
                "device_id": "d_abc",
                "device_token": "devtok",
                "account_origin": "https://account.test",
            }
        )
        resp = handle_membership_reach_on({"by": "user"})
        self.assertTrue(resp.ok, resp.error)
        self.assertIn({"origin_port": live_port}, account.payloads)
        helper_pid = cloudflared_supervisor.running_pid()
        self.assertIsNotNone(helper_pid)
        membership = resp.result["membership"]
        self.assertTrue(membership["online"])
        self.assertEqual(membership["hostname"], "https://d-abc.example.test")
        self.assertEqual(membership["web_url"], "https://d-abc.example.test/ui/")
        self.assertNotIn("acc_admin_fixture", membership["web_url"] or "")
        self.assertNotIn("acc_admin_fixture", membership["hostname"] or "")
        remote = get_remote_access_settings()
        self.assertEqual(remote["provider"], "reach")
        self.assertTrue(remote["enabled"])
        self.assertEqual(remote["web_public_url"], "https://d-abc.example.test")
        off = handle_membership_reach_off({"by": "user"})
        self.assertTrue(off.ok)
        self.assertIsNone(cloudflared_supervisor.running_pid())
        assert helper_pid is not None
        if os.name != "nt":
            with self.assertRaises(ProcessLookupError):
                os.kill(helper_pid, 0)
        self.assertFalse(off.result["membership"]["online"])
        self.assertEqual(get_remote_access_settings()["provider"], "reach")
        self.assertFalse(get_remote_access_settings()["enabled"])

    def test_reach_on_stops_helper_when_remote_state_cannot_be_committed(self) -> None:
        os.environ["CCCC_ACCOUNT_ORIGIN"] = "https://account.test"
        account = FakeAccount()
        set_account_transport_for_tests(account)
        set_reach_command_for_tests(
            [sys.executable, "-c", "import time; time.sleep(30)"]
        )
        create_access_token("admin", is_admin=True)
        self._record_live_web()
        update_remote_access_settings(
            {
                "provider": "manual",
                "enabled": True,
                "web_public_url": "https://manual.example.test",
            }
        )
        save_membership(
            {
                "logged_in": True,
                "device_id": "d_abc",
                "device_token": "devtok",
                "account_origin": "https://account.test",
            }
        )

        with patch(
            "cccc.daemon.ops.membership_ops.update_remote_access_settings",
            side_effect=OSError("settings write failed"),
        ):
            with self.assertRaisesRegex(OSError, "settings write failed"):
                handle_membership_reach_on({"by": "user"})

        self.assertIsNone(cloudflared_supervisor.running_pid())
        remote = get_remote_access_settings()
        self.assertEqual(remote["provider"], "manual")
        self.assertEqual(remote["web_public_url"], "https://manual.example.test")

    def test_status_stops_helper_when_account_reports_cut(self) -> None:
        os.environ["CCCC_ACCOUNT_ORIGIN"] = "https://account.test"
        account = FakeAccount()
        set_account_transport_for_tests(account)
        set_reach_command_for_tests(
            [sys.executable, "-c", "import time; time.sleep(30)"]
        )
        create_access_token("admin", is_admin=True)
        self._record_live_web()
        save_membership(
            {
                "logged_in": True,
                "device_id": "d_abc",
                "device_token": "devtok",
                "account_origin": "https://account.test",
            }
        )
        started = handle_membership_reach_on({"by": "user"})
        self.assertTrue(started.ok, started.error)
        account.disabled = True
        status = handle_membership_status({"by": "user"})
        self.assertTrue(status.result["membership"]["cut"])
        self.assertFalse(status.result["membership"]["online"])
        self.assertFalse(get_remote_access_settings()["enabled"])
        again = handle_membership_reach_on({"by": "user"})
        self.assertFalse(again.ok)
        self.assertEqual(again.error.code, "membership_disabled")

    def test_status_treats_a_missing_linked_device_as_terminal(self) -> None:
        def missing_device(method, url, headers, body, timeout_s):
            _ = method, url, headers, body, timeout_s
            return 401, {"error": {"code": "unauthorized", "message": "not logged in"}}

        set_account_transport_for_tests(missing_device)
        save_membership(
            {
                "logged_in": True,
                "device_id": "d_deleted",
                "device_token": "deleted-token",
                "account_origin": "https://account.test",
            }
        )
        update_remote_access_settings(
            {
                "provider": "reach",
                "enabled": True,
                "web_public_url": "https://deleted.example.test",
            }
        )

        with patch.object(cloudflared_supervisor, "stop") as stop:
            response = handle_membership_status({"by": "user"})

        self.assertTrue(response.ok, response.error)
        self.assertTrue(response.result["membership"]["cut"])
        stop.assert_called_once_with()
        remote = get_remote_access_settings()
        self.assertFalse(remote["enabled"])
        self.assertEqual(remote["web_public_url"], "")

    def test_status_preserves_binding_on_transient_account_failure(self) -> None:
        def unavailable(method, url, headers, body, timeout_s):
            _ = method, url, headers, body, timeout_s
            return 503, {"error": {"code": "network", "message": "try later"}}

        set_account_transport_for_tests(unavailable)
        save_membership(
            {
                "logged_in": True,
                "device_id": "d_transient",
                "device_token": "transient-token",
                "account_origin": "https://account.test",
            }
        )

        response = handle_membership_status({"by": "user"})

        self.assertTrue(response.ok, response.error)
        membership = response.result["membership"]
        self.assertTrue(membership["logged_in"])
        self.assertFalse(membership["cut"])
        self.assertFalse(membership["account_reachable"])
        self.assertEqual(load_membership()["device_token"], "transient-token")

    def test_status_fails_closed_when_cut_cannot_stop_the_helper(self) -> None:
        account = FakeAccount()
        account.disabled = True
        set_account_transport_for_tests(account)
        save_membership(
            {
                "logged_in": True,
                "device_id": "d_abc",
                "device_token": "devtok",
                "account_origin": "https://account.test",
            }
        )
        update_remote_access_settings(
            {
                "provider": "reach",
                "enabled": True,
                "web_public_url": "https://old.example.test",
            }
        )

        with patch.object(
            cloudflared_supervisor,
            "stop",
            side_effect=RuntimeError("tracked helper did not exit"),
        ):
            response = handle_membership_status({"by": "user"})

        self.assertFalse(response.ok)
        self.assertEqual(response.error.code, "membership_subprocess")
        self.assertTrue(load_membership()["disabled"])
        remote = get_remote_access_settings()
        self.assertTrue(remote["enabled"])
        self.assertEqual(remote["web_public_url"], "https://old.example.test")

    def test_reach_issuance_disabled_applies_cut_and_stops_the_existing_helper(
        self,
    ) -> None:
        class ReachDisabledAccount(FakeAccount):
            def __call__(self, method, url, headers, body, timeout_s):
                if url.endswith("/v1/reach"):
                    self.calls.append((method, url))
                    return 403, {
                        "error": {"code": "disabled", "message": "device disabled"}
                    }
                return super().__call__(method, url, headers, body, timeout_s)

        account = ReachDisabledAccount()
        set_account_transport_for_tests(account)
        set_reach_command_for_tests(
            [sys.executable, "-c", "import time; time.sleep(30)"]
        )
        create_access_token("admin", is_admin=True)
        self._record_live_web()
        save_membership(
            {
                "logged_in": True,
                "account_origin": "https://account.test",
                "device_id": "d_abc",
                "device_token": "devtok",
            }
        )
        update_remote_access_settings(
            {
                "provider": "reach",
                "enabled": True,
                "web_public_url": "https://old.example.test",
            }
        )
        with patch.object(cloudflared_supervisor, "stop") as stop:
            response = handle_membership_reach_on({"by": "user"})
        self.assertFalse(response.ok)
        self.assertEqual(response.error.code, "membership_disabled")
        stop.assert_called_once_with()
        self.assertTrue(load_membership()["disabled"])
        self.assertFalse(get_remote_access_settings()["enabled"])
        self.assertEqual(get_remote_access_settings()["web_public_url"], "")

    def test_global_membership_status_does_not_select_an_actor_connector(self) -> None:
        save_membership({"logged_in": True, "hostname": "https://d-abc.example.test"})
        status = handle_membership_status({"by": "user"})
        self.assertNotIn("connector_url", status.result["membership"])
        self.assertEqual(
            status.result["membership"]["hostname"], "https://d-abc.example.test"
        )

    def test_status_never_embeds_local_credentials_in_an_unsafe_hostname(self) -> None:
        create_access_token(
            "admin", is_admin=True, custom_token="acc_admin_must_not_escape"
        )
        save_membership({"logged_in": True, "hostname": "http://attacker.example.test"})

        status = handle_membership_status({"by": "user"})

        self.assertTrue(status.ok)
        membership = status.result["membership"]
        self.assertIsNone(membership["hostname"])
        self.assertIsNone(membership["web_url"])
        self.assertNotIn("connector_url", membership)

    def test_settings_preserve_reach_provider(self) -> None:
        update_remote_access_settings({"provider": "reach", "enabled": False})
        self.assertEqual(get_remote_access_settings()["provider"], "reach")

    def test_remote_access_does_not_report_reach_running_without_a_live_helper(
        self,
    ) -> None:
        update_remote_access_settings(
            {
                "provider": "reach",
                "enabled": True,
                "web_public_url": "https://d-abc.example.test",
            }
        )
        resp = handle_remote_access_state({"by": "user"})
        self.assertTrue(resp.ok)
        remote = resp.result["remote_access"]
        self.assertNotEqual(remote["status"], "running")
        self.assertIsNone(remote["endpoint"])

    def test_membership_status_requires_the_account_tunnel_to_be_online(self) -> None:
        class OfflineAccount(FakeAccount):
            def __call__(self, method, url, headers, body, timeout_s):
                if url.endswith("/v1/device"):
                    self.calls.append((method, url))
                    self.timeouts.append(timeout_s)
                    return 200, {
                        "device_id": "d_abc",
                        "hostname": "https://d-abc.example.test",
                        "disabled": False,
                        "online": False,
                    }
                return super().__call__(method, url, headers, body, timeout_s)

        account = OfflineAccount()
        set_account_transport_for_tests(account)
        save_membership(
            {
                "logged_in": True,
                "device_id": "d_abc",
                "device_token": "devtok",
                "account_origin": "https://account.test",
                "hostname": "https://d-abc.example.test",
            }
        )
        update_remote_access_settings({"provider": "reach", "enabled": True})

        with patch.object(
            cloudflared_supervisor, "status", return_value={"running": True, "pid": 123}
        ):
            response = handle_membership_status({"by": "user"})

        self.assertTrue(response.ok)
        self.assertFalse(response.result["membership"]["online"])
        self.assertEqual(account.timeouts[-1], 2.0)

    def test_configure_rejects_setting_reach(self) -> None:
        resp = handle_remote_access_configure({"provider": "reach", "by": "user"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "remote_access_invalid_config")

    def test_configure_rejects_changes_while_reach_is_active(self) -> None:
        update_remote_access_settings({"provider": "reach", "enabled": True})
        resp = handle_remote_access_configure(
            {"provider": "manual", "web_port": 9000, "by": "user"}
        )
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "remote_access_invalid_config")
        remote = get_remote_access_settings()
        self.assertEqual(remote["provider"], "reach")
        self.assertTrue(remote["enabled"])

    def test_reach_off_requires_reach_provider(self) -> None:
        resp = handle_membership_reach_off({"by": "user"})
        self.assertFalse(resp.ok)
        self.assertEqual(resp.error.code, "membership_not_in_reach")

    def test_cli_parser_exposes_membership_verbs(self) -> None:
        from cccc.cli.main import build_parser

        parser = build_parser()
        self.assertEqual(parser.parse_args(["login"]).func.__name__, "cmd_login")
        self.assertEqual(parser.parse_args(["logout"]).func.__name__, "cmd_logout")
        self.assertEqual(parser.parse_args(["reach", "on"]).action, "on")
        self.assertEqual(parser.parse_args(["reach", "install"]).action, "install")

    def test_logout_clears_identity_and_warns(self) -> None:
        account = FakeAccount()
        set_account_transport_for_tests(account)
        save_membership(
            {
                "logged_in": True,
                "account_origin": "https://account.test",
                "device_id": "dev_test",
                "device_token": "devtok",
                "hostname": "https://d-x.example",
            }
        )
        update_remote_access_settings(
            {
                "provider": "reach",
                "enabled": False,
                "web_public_url": "https://d-x.example",
            }
        )
        resp = handle_membership_logout({"by": "user"})
        self.assertTrue(resp.ok)
        self.assertEqual(resp.result["membership"]["warning"], LOGOUT_WARNING)
        self.assertFalse(resp.result["membership"]["logged_in"])
        self.assertIsNone(resp.result["membership"]["hostname"])
        self.assertIsNone(resp.result["membership"]["web_url"])
        self.assertEqual(get_remote_access_settings()["web_public_url"], "")
        self.assertIsNone(
            handle_membership_status({}).result["membership"]["device_id"]
        )
        self.assertTrue(account.disabled)

    def test_logout_preserves_local_identity_when_remote_retirement_fails(self) -> None:
        def unavailable(method, url, headers, body, timeout_s):
            _ = method, url, headers, body, timeout_s
            raise membership_ops.AccountError("membership_network", "offline")

        set_account_transport_for_tests(unavailable)
        save_membership(
            {
                "logged_in": True,
                "account_origin": "https://account.test",
                "device_id": "dev_test",
                "device_token": "devtok",
            }
        )

        response = handle_membership_logout({"by": "user"})

        self.assertFalse(response.ok)
        self.assertEqual(response.error.code, "membership_network")
        self.assertTrue(load_membership()["logged_in"])

    def test_logout_stops_a_tracked_helper_even_after_provider_drift(self) -> None:
        save_membership(
            {
                "logged_in": True,
                "device_id": "dev_test",
                "hostname": "https://d-x.example",
            }
        )
        update_remote_access_settings(
            {
                "provider": "manual",
                "enabled": True,
                "web_public_url": "https://manual.example",
            }
        )
        with patch.object(cloudflared_supervisor, "stop") as stop:
            resp = handle_membership_logout({"by": "user"})
        self.assertTrue(resp.ok)
        stop.assert_called_once_with()
        remote = get_remote_access_settings()
        self.assertEqual(remote["provider"], "manual")
        self.assertEqual(remote["web_public_url"], "https://manual.example")

    def test_supervisor_keeps_tunnel_token_out_of_process_arguments(self) -> None:
        class FakeProcess:
            pid = 999999

            @staticmethod
            def poll():
                return None

        captured: list[str] = []

        def fake_popen(argv, **_kwargs):
            captured.extend(str(part) for part in argv)
            return FakeProcess()

        with (
            patch.object(
                cloudflared_supervisor, "inspect", return_value={"matches_pin": True}
            ),
            patch.object(
                cloudflared_supervisor,
                "binary_path",
                return_value=Path(self._tmp.name) / "cloudflared",
            ),
            patch.object(
                cloudflared_supervisor.subprocess, "Popen", side_effect=fake_popen
            ),
            patch.object(
                cloudflared_supervisor,
                "_resolve_executable",
                return_value=Path(self._tmp.name) / "cloudflared",
            ),
        ):
            cloudflared_supervisor.start("super-secret-token")

        self.assertIn("--token-file", captured)
        self.assertNotIn("--token", captured)
        self.assertNotIn("super-secret-token", captured)
        token_file = cloudflared_supervisor.token_path()
        self.assertEqual(
            token_file.read_text(encoding="utf-8").strip(), "super-secret-token"
        )
        if os.name != "nt":
            self.assertEqual(token_file.stat().st_mode & 0o777, 0o600)

    def test_supervisor_retains_tracking_when_process_stop_fails(self) -> None:
        pid_file = cloudflared_supervisor.pid_path()
        token_file = cloudflared_supervisor.token_path()
        pid_file.parent.mkdir(parents=True, exist_ok=True)
        pid_file.write_text(str(os.getpid()), encoding="utf-8")
        token_file.write_text("tracked-token", encoding="utf-8")

        def fail_term(pid: int, sig: int) -> None:
            _ = pid
            if sig == 0:
                return
            raise PermissionError("not allowed")

        try:
            with patch.object(cloudflared_supervisor.os, "kill", side_effect=fail_term):
                with self.assertRaises(RuntimeError):
                    cloudflared_supervisor.stop()
            self.assertTrue(pid_file.exists())
            self.assertTrue(token_file.exists())
        finally:
            pid_file.unlink(missing_ok=True)
            token_file.unlink(missing_ok=True)

    @unittest.skipUnless(os.name == "posix", "helper process fixture is Unix-specific")
    def test_supervisor_reaps_child_when_pid_tracking_cannot_be_written(self) -> None:
        helper = Path(self._tmp.name) / "cloudflared-pid-fixture"
        helper.write_text("#!/bin/sh\nexec sleep 30\n", encoding="utf-8")
        helper.chmod(0o700)
        children = []
        real_popen = subprocess.Popen

        def spawn(*args, **kwargs):
            child = real_popen(*args, **kwargs)
            children.append(child)
            return child

        with patch.object(
            cloudflared_supervisor.subprocess,
            "Popen",
            side_effect=spawn,
        ), patch.object(
            cloudflared_supervisor,
            "atomic_write_json",
            side_effect=OSError("pid tracking failed"),
        ):
            with self.assertRaises(OSError):
                cloudflared_supervisor.start("secret-token", command=[str(helper)])

        self.assertEqual(len(children), 1)
        self.assertIsNotNone(children[0].poll())

    @unittest.skipUnless(
        os.name == "posix", "process command inspection is Unix-specific"
    )
    def test_supervisor_refuses_to_kill_a_reused_non_cloudflared_pid(self) -> None:
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)"])
        pid_file = cloudflared_supervisor.pid_path()
        token_file = cloudflared_supervisor.token_path()
        pid_file.parent.mkdir(parents=True, exist_ok=True)
        managed_binary = cloudflared_supervisor.binary_path()
        managed_binary.parent.mkdir(parents=True, exist_ok=True)
        managed_binary.symlink_to("/bin/sleep")
        pid_file.write_text(str(child.pid), encoding="utf-8")
        token_file.write_text("tracked-token", encoding="utf-8")
        try:
            with self.assertRaisesRegex(RuntimeError, "tracked cloudflared executable"):
                cloudflared_supervisor.stop()
            self.assertIsNone(child.poll())
            self.assertTrue(pid_file.exists())
            self.assertTrue(token_file.exists())
        finally:
            if child.poll() is None:
                child.terminate()
            child.wait(timeout=5)
            pid_file.unlink(missing_ok=True)
            token_file.unlink(missing_ok=True)

    @unittest.skipUnless(
        os.name == "posix", "process executable inspection is Unix-specific"
    )
    def test_supervisor_does_not_trust_cloudflared_text_in_process_arguments(
        self,
    ) -> None:
        child = subprocess.Popen(
            [
                sys.executable,
                "-c",
                "import time; time.sleep(30)",
                "cloudflared-decoy",
            ]
        )
        pid_file = cloudflared_supervisor.pid_path()
        token_file = cloudflared_supervisor.token_path()
        pid_file.parent.mkdir(parents=True, exist_ok=True)
        managed_binary = cloudflared_supervisor.binary_path()
        managed_binary.parent.mkdir(parents=True, exist_ok=True)
        managed_binary.symlink_to("/bin/sleep")
        pid_file.write_text(str(child.pid), encoding="utf-8")
        token_file.write_text("tracked-token", encoding="utf-8")
        try:
            with self.assertRaisesRegex(RuntimeError, "tracked cloudflared executable"):
                cloudflared_supervisor.stop()
            self.assertIsNone(child.poll())
            self.assertTrue(pid_file.exists())
            self.assertTrue(token_file.exists())
        finally:
            if child.poll() is None:
                child.terminate()
            child.wait(timeout=5)
            pid_file.unlink(missing_ok=True)
            token_file.unlink(missing_ok=True)


if __name__ == "__main__":
    unittest.main()
