import os
import unittest


class TestCliEnvBoolParsing(unittest.TestCase):
    def test_env_flag_parses_falsey_strings(self) -> None:
        from cccc.cli import _env_flag

        old = os.environ.get("CCCC_WEB_RELOAD")
        try:
            os.environ["CCCC_WEB_RELOAD"] = "false"
            self.assertFalse(_env_flag("CCCC_WEB_RELOAD"))
            os.environ["CCCC_WEB_RELOAD"] = "0"
            self.assertFalse(_env_flag("CCCC_WEB_RELOAD"))
            os.environ["CCCC_WEB_RELOAD"] = "off"
            self.assertFalse(_env_flag("CCCC_WEB_RELOAD"))
            os.environ["CCCC_WEB_RELOAD"] = "true"
            self.assertTrue(_env_flag("CCCC_WEB_RELOAD"))
        finally:
            if old is None:
                os.environ.pop("CCCC_WEB_RELOAD", None)
            else:
                os.environ["CCCC_WEB_RELOAD"] = old


if __name__ == "__main__":
    unittest.main()
