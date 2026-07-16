import unittest

from cccc.ports.im.adapters.feishu.domain import FEISHU_DOMAIN, LARK_DOMAIN, normalize_domain


class TestFeishuDomainNormalization(unittest.TestCase):
    def test_defaults_empty_domain_to_feishu(self) -> None:
        self.assertEqual(normalize_domain(""), FEISHU_DOMAIN)

    def test_adds_https_to_bare_domain(self) -> None:
        self.assertEqual(normalize_domain("open.feishu.cn"), FEISHU_DOMAIN)

    def test_removes_open_apis_suffix(self) -> None:
        self.assertEqual(normalize_domain("https://open.feishu.cn/open-apis"), FEISHU_DOMAIN)

    def test_preserves_lark_domain(self) -> None:
        self.assertEqual(normalize_domain("open.larkoffice.com"), LARK_DOMAIN)


if __name__ == "__main__":
    unittest.main()
