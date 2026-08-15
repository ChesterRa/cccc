import io
import unittest
from unittest.mock import patch


class TestWebBanner(unittest.TestCase):
    def test_loopback_binding_only_shows_local_url(self) -> None:
        from cccc.ports.web import web_banner

        stderr = io.StringIO()
        with patch.object(web_banner, "detect_lan_ipv4", return_value="192.168.1.20") as detect:
            web_banner.print_web_banner(
                "127.0.0.1", 8848, implementation="python", stream=stderr
            )

        self.assertIn("[cccc]   Local:   http://127.0.0.1:8848", stderr.getvalue())
        self.assertNotIn("Network:", stderr.getvalue())
        detect.assert_not_called()

    def test_wildcard_binding_shows_local_and_network_urls(self) -> None:
        from cccc.ports.web import web_banner

        stderr = io.StringIO()
        with patch.object(web_banner, "detect_lan_ipv4", return_value="192.168.1.20"):
            web_banner.print_web_banner("0.0.0.0", 8848, implementation="python", stream=stderr)

        self.assertIn("[cccc]   Local:   http://localhost:8848", stderr.getvalue())
        self.assertIn("[cccc]   Network: http://192.168.1.20:8848", stderr.getvalue())

    def test_explicit_interface_binding_does_not_advertise_another_interface(self) -> None:
        from cccc.ports.web import web_banner

        self.assertEqual(
            web_banner.urls("192.168.1.30", 8848, "192.168.1.20"),
            ("http://192.168.1.30:8848", None),
        )


if __name__ == "__main__":
    unittest.main()
