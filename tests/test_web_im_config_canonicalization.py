import os
import tempfile
import unittest

from fastapi.testclient import TestClient


class TestWebImConfigCanonicalization(unittest.TestCase):
    def test_canonicalize_wecom_env_refs(self) -> None:
        from cccc.ports.im.config_schema import canonicalize_im_config

        result = canonicalize_im_config({
            "platform": "wecom",
            "wecom_bot_id": "WECOM_BOT_ID",
            "wecom_secret": "WECOM_SECRET",
        })

        self.assertEqual(result, {
            "platform": "wecom",
            "wecom_bot_id_env": "WECOM_BOT_ID",
            "wecom_secret_env": "WECOM_SECRET",
        })

    def test_canonicalize_wecom_raw_values_in_env_slots_fall_back_to_value_keys(self) -> None:
        from cccc.ports.im.config_schema import canonicalize_im_config

        result = canonicalize_im_config({
            "platform": "wecom",
            "wecom_bot_id_env": "corp123",
            "wecom_secret_env": "sec456",
        })

        self.assertEqual(result, {
            "platform": "wecom",
            "wecom_bot_id": "corp123",
            "wecom_secret": "sec456",
        })

    def test_canonicalize_drops_legacy_weixin_command(self) -> None:
        from cccc.ports.im.config_schema import canonicalize_im_config

        result = canonicalize_im_config({
            "platform": "weixin",
            "weixin_command": "node scripts/im/weixin_sidecar.mjs",
        })

        self.assertEqual(result, {"platform": "weixin"})

    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return td, cleanup

    def _create_group(self, title: str = "im-cfg") -> str:
        from cccc.kernel.group import create_group
        from cccc.kernel.registry import load_registry

        reg = load_registry()
        group = create_group(reg, title=title, topic="")
        return group.group_id

    def test_im_set_canonicalizes_legacy_token_fields(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("im-cfg-canon")
            with TestClient(create_app()) as client:
                r1 = client.post(
                    "/api/im/set",
                    json={
                        "group_id": gid,
                        "platform": "telegram",
                        "token_env": "TELEGRAM_BOT_TOKEN",
                    },
                )
                self.assertEqual(r1.status_code, 200)
                self.assertTrue(r1.json().get("ok"))

                c1 = client.get(f"/api/im/config?group_id={gid}")
                self.assertEqual(c1.status_code, 200)
                im1 = ((c1.json().get("result") or {}).get("im") or {})
                self.assertEqual(str(im1.get("platform") or ""), "telegram")
                self.assertEqual(str(im1.get("bot_token_env") or ""), "TELEGRAM_BOT_TOKEN")
                self.assertNotIn("token_env", im1)

                r2 = client.post(
                    "/api/im/set",
                    json={
                        "group_id": gid,
                        "platform": "telegram",
                        "token_env": "raw-secret-token",
                    },
                )
                self.assertEqual(r2.status_code, 200)
                self.assertTrue(r2.json().get("ok"))

                c2 = client.get(f"/api/im/config?group_id={gid}")
                self.assertEqual(c2.status_code, 200)
                im2 = ((c2.json().get("result") or {}).get("im") or {})
                self.assertEqual(str(im2.get("bot_token") or ""), "raw-secret-token")
                self.assertNotIn("token", im2)
                self.assertNotIn("token_env", im2)

                r3 = client.post(
                    "/api/im/set",
                    json={
                        "group_id": gid,
                        "platform": "slack",
                        "token_env": "SLACK_BOT_TOKEN",
                        "app_token_env": "SLACK_APP_TOKEN",
                    },
                )
                self.assertEqual(r3.status_code, 200)
                self.assertTrue(r3.json().get("ok"))

                c3 = client.get(f"/api/im/config?group_id={gid}")
                self.assertEqual(c3.status_code, 200)
                im3 = ((c3.json().get("result") or {}).get("im") or {})
                self.assertEqual(str(im3.get("platform") or ""), "slack")
                self.assertEqual(str(im3.get("bot_token_env") or ""), "SLACK_BOT_TOKEN")
                self.assertEqual(str(im3.get("app_token_env") or ""), "SLACK_APP_TOKEN")
        finally:
            cleanup()


    def test_im_set_wecom_fields_round_trip(self) -> None:
        from cccc.ports.web.app import create_app

        _, cleanup = self._with_home()
        try:
            gid = self._create_group("im-cfg-wecom")
            with TestClient(create_app()) as client:
                r = client.post(
                    "/api/im/set",
                    json={
                        "group_id": gid,
                        "platform": "wecom",
                        "wecom_bot_id": "corp123",
                        "wecom_secret": "sec456",
                    },
                )
                self.assertEqual(r.status_code, 200)
                self.assertTrue(r.json().get("ok"))

                c = client.get(f"/api/im/config?group_id={gid}")
                self.assertEqual(c.status_code, 200)
                im = ((c.json().get("result") or {}).get("im") or {})
                self.assertEqual(str(im.get("platform") or ""), "wecom")
                self.assertEqual(str(im.get("wecom_bot_id") or ""), "corp123")
                self.assertEqual(str(im.get("wecom_secret") or ""), "sec456")
                self.assertNotIn("wecom_agent_id", im)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
