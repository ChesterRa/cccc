import re
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]


def read(relative: str) -> str:
    return (REPO_ROOT / relative).read_text(encoding="utf-8")


class TestDeepSeekDefaultModelParity(unittest.TestCase):
    """The daemon launches the default model; the Web shows it; the guides name it."""

    def test_web_and_guides_show_the_model_the_daemon_launches(self) -> None:
        contract = re.search(
            r'pub const DEEPSEEK_DEFAULT_MODEL: &str = "([^"]+)";',
            read("crates/cccc-contracts/src/deepseek.rs"),
        )
        web = re.search(
            r'export const DEEPSEEK_MODEL = "([^"]+)";',
            read("web/src/components/modals/deepseekSettingsModel.ts"),
        )
        self.assertIsNotNone(contract)
        self.assertIsNotNone(web)
        model = contract.group(1)
        self.assertEqual(web.group(1), model)
        for guide in ("docs/guide/runtimes.md", "docs/guide/web-ui.md"):
            self.assertIn(f"`{model}`", read(guide), guide)


if __name__ == "__main__":
    unittest.main()
