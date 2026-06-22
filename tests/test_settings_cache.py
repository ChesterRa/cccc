import os
import tempfile
import unittest
from pathlib import Path
from unittest import mock


class TestSettingsCache(unittest.TestCase):
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

        return Path(td), cleanup

    def test_reuses_settings_when_file_is_unchanged(self) -> None:
        from cccc.kernel.settings import get_observability_settings, get_web_branding_settings, load_settings

        home, cleanup = self._with_home()
        try:
            settings_file = home / "settings.yaml"
            settings_file.write_text(
                "observability:\n"
                "  log_level: DEBUG\n"
                "web_branding:\n"
                "  product_name: CCCC Test\n",
                encoding="utf-8",
            )
            with mock.patch(
                "cccc.kernel.settings.Path.read_text",
                autospec=True,
                return_value=settings_file.read_text(encoding="utf-8"),
            ) as read_text:
                self.assertEqual(load_settings()["observability"]["log_level"], "DEBUG")
                self.assertEqual(get_observability_settings()["log_level"], "DEBUG")
                self.assertEqual(get_web_branding_settings()["product_name"], "CCCC Test")

            self.assertEqual(read_text.call_count, 1)
        finally:
            cleanup()
