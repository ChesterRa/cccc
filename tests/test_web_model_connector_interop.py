from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import yaml


class TestWebModelConnectorInterop(unittest.TestCase):
    def test_rust_settings_store_migrates_into_python_connector_file(self) -> None:
        from cccc.kernel.web_model_connectors import (
            load_web_model_connectors,
            verify_web_model_connector_secret,
        )

        with tempfile.TemporaryDirectory() as raw_home:
            home = Path(raw_home)
            (home / "web_model_connectors.yaml").write_text(
                yaml.safe_dump(
                    {
                        "connectors": {
                            "wmc_python": {
                                "group_id": "g_python",
                                "actor_id": "web1",
                                "provider": "chatgpt_web",
                                "secret": "wmcs_python",
                                "created_at": "2026-08-09T00:00:00Z",
                                "updated_at": "2026-08-09T00:00:00Z",
                                "revoked": False,
                            }
                        }
                    },
                    sort_keys=False,
                ),
                encoding="utf-8",
            )
            (home / "settings.yaml").write_text(
                yaml.safe_dump(
                    {
                        "observability": {"log_level": "debug"},
                        "web_model_connectors": [
                            {
                                "connector_id": "wmc_rust",
                                "group_id": "g_rust",
                                "actor_id": "web2",
                                "provider": "chatgpt",
                                "secret": "wmcs_rust",
                                "created_at": "2026-08-10T00:00:00Z",
                                "updated_at": "2026-08-10T00:00:00Z",
                                "revoked": False,
                            }
                        ],
                    },
                    sort_keys=False,
                ),
                encoding="utf-8",
            )

            first = load_web_model_connectors(home)
            second = load_web_model_connectors(home)

            self.assertEqual(set(first), {"wmc_python", "wmc_rust"})
            self.assertEqual(set(second), set(first))
            self.assertIsNotNone(
                verify_web_model_connector_secret("wmc_rust", "wmcs_rust", home)
            )
            settings = (
                yaml.safe_load((home / "settings.yaml").read_text(encoding="utf-8"))
                or {}
            )
            self.assertNotIn("web_model_connectors", settings)
            self.assertEqual(
                (settings.get("observability") or {}).get("log_level"), "debug"
            )
            canonical = (
                yaml.safe_load(
                    (home / "web_model_connectors.yaml").read_text(encoding="utf-8")
                )
                or {}
            )
            self.assertEqual(
                set((canonical.get("connectors") or {}).keys()),
                {"wmc_python", "wmc_rust"},
            )

    def test_rotation_after_migration_does_not_resurrect_old_connector(self) -> None:
        from cccc.kernel.web_model_connectors import (
            create_web_model_connector,
            load_web_model_connectors,
        )

        with tempfile.TemporaryDirectory() as raw_home:
            home = Path(raw_home)
            (home / "settings.yaml").write_text(
                yaml.safe_dump(
                    {
                        "web_model_connectors": [
                            {
                                "connector_id": "wmc_old",
                                "group_id": "g_shared",
                                "actor_id": "web1",
                                "provider": "chatgpt",
                                "secret": "wmcs_old",
                                "created_at": "2026-08-09T00:00:00Z",
                                "updated_at": "2026-08-09T00:00:00Z",
                                "revoked": False,
                            }
                        ]
                    },
                    sort_keys=False,
                ),
                encoding="utf-8",
            )

            created = create_web_model_connector(
                group_id="g_shared",
                actor_id="web1",
                provider="chatgpt_web",
                home=home,
            )
            reloaded = load_web_model_connectors(home)

            self.assertIn("wmc_old", created.get("replaced_connector_ids") or [])
            self.assertTrue(bool((reloaded.get("wmc_old") or {}).get("revoked")))
            self.assertFalse(
                bool(
                    (reloaded.get(str(created.get("connector_id") or "")) or {}).get(
                        "revoked"
                    )
                )
            )
            settings = (
                yaml.safe_load((home / "settings.yaml").read_text(encoding="utf-8"))
                or {}
            )
            self.assertNotIn("web_model_connectors", settings)

    def test_rust_default_chatgpt_provider_enables_python_browser_delivery(
        self,
    ) -> None:
        from cccc.daemon.actors.web_model_browser_delivery import (
            web_model_browser_delivery_enabled,
        )
        from cccc.kernel.web_model_connectors import create_web_model_connector

        with (
            tempfile.TemporaryDirectory() as raw_home,
            patch.dict(os.environ, {"CCCC_HOME": raw_home}, clear=False),
        ):
            create_web_model_connector(
                group_id="g_shared",
                actor_id="web1",
                provider="chatgpt",
            )
            actor = {
                "id": "web1",
                "runtime": "web_model",
                "runner": "headless",
                "env": {},
            }

            self.assertTrue(web_model_browser_delivery_enabled("g_shared", actor))


if __name__ == "__main__":
    unittest.main()
