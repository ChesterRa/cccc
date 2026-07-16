import unittest


class TestCliCommonLocalUrl(unittest.TestCase):
    def test_display_local_host_maps_wildcards_to_localhost(self) -> None:
        from cccc.cli.common import _display_local_host

        self.assertEqual(_display_local_host("0.0.0.0"), "localhost")
        self.assertEqual(_display_local_host("::"), "localhost")
        self.assertEqual(_display_local_host("[::]"), "localhost")
        self.assertEqual(_display_local_host("127.0.0.1"), "127.0.0.1")

    def test_http_host_literal_wraps_ipv6(self) -> None:
        from cccc.cli.common import _http_host_literal

        self.assertEqual(_http_host_literal("::1"), "[::1]")
        self.assertEqual(_http_host_literal("[::1]"), "[::1]")
        self.assertEqual(_http_host_literal("localhost"), "localhost")


if __name__ == "__main__":
    unittest.main()
