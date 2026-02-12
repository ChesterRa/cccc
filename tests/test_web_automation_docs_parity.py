import re
import unittest
from pathlib import Path


def _tokenize(text: str) -> list[str]:
    return [t for t in re.findall(r"[a-z0-9]+", text.lower()) if t]


def _contains_all_tokens(haystack: str, phrase: str) -> bool:
    hay_tokens = set(_tokenize(haystack))
    need_tokens = _tokenize(phrase)
    return bool(need_tokens) and all(t in hay_tokens for t in need_tokens)


class TestWebAutomationDocsParity(unittest.TestCase):
    def test_web_guide_covers_editor_labels(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        editor_path = repo_root / "web" / "src" / "components" / "modals" / "settings" / "AutomationRuleEditorModal.tsx"
        guide_path = repo_root / "docs" / "guide" / "web-ui.md"

        editor_text = editor_path.read_text(encoding="utf-8")
        guide_text = guide_path.read_text(encoding="utf-8")

        schedule_labels = {
            str(label or "").strip()
            for _, label in re.findall(
                r'<option value="(interval|cron|at)">(.*?)</option>',
                editor_text,
            )
            if str(label or "").strip()
        }
        action_labels = {
            str(label or "").strip()
            for _, label in re.findall(
                r'<option value="(notify|group_state|actor_control)">(.*?)</option>',
                editor_text,
            )
            if str(label or "").strip()
        }

        missing = sorted(
            label
            for label in sorted(schedule_labels | action_labels)
            if not _contains_all_tokens(guide_text, label)
        )
        self.assertEqual(
            missing,
            [],
            msg=f"docs/guide/web-ui.md missing automation labels from editor: {missing}",
        )

    def test_reference_features_covers_automation_contract_terms(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        features_path = repo_root / "docs" / "reference" / "features.md"
        text = features_path.read_text(encoding="utf-8")

        for trigger_kind in ("`every_seconds`", "`cron`", "`at`"):
            self.assertIn(trigger_kind, text)

        for action_kind in ("`notify`", "`group_state`", "`actor_control`"):
            self.assertIn(action_kind, text)

        self.assertIn("One-time rules auto-mark as completed", text)


if __name__ == "__main__":
    unittest.main()
