import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from cccc.daemon.bootstrap_im_ops import autostart_enabled_im_bridges


class TestBootstrapImOps(unittest.TestCase):
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

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def test_no_groups_is_noop(self) -> None:
        home, cleanup = self._with_home()
        try:
            autostart_enabled_im_bridges(home)
        finally:
            cleanup()

    def test_groups_without_enabled_im_do_not_spawn(self) -> None:
        home, cleanup = self._with_home()
        try:
            create, _ = self._call("group_create", {"title": "no-im", "topic": "", "by": "user"})
            self.assertTrue(create.ok, getattr(create, "error", None))
            with patch("cccc.daemon.bootstrap_im_ops.subprocess.Popen") as mock_popen:
                autostart_enabled_im_bridges(home)
                mock_popen.assert_not_called()
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
