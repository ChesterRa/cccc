from pathlib import Path
import unittest


class TestDocsProfileCliGuides(unittest.TestCase):
    def test_getting_started_cli_shows_profile_creation_and_binding(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        doc = repo_root / "docs" / "guide" / "getting-started" / "cli.md"
        text = doc.read_text(encoding="utf-8")

        self.assertIn("cccc actor profile upsert", text)
        self.assertIn("cccc actor add assistant --profile-id", text)

    def test_workflows_mentions_profile_backed_actor_setup(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        doc = repo_root / "docs" / "guide" / "workflows.md"
        text = doc.read_text(encoding="utf-8")

        self.assertIn("cccc actor profile upsert", text)
        self.assertIn("--profile-id", text)

    def test_use_cases_shows_profile_backed_actor_setup(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        doc = repo_root / "docs" / "guide" / "use-cases.md"
        text = doc.read_text(encoding="utf-8")

        self.assertIn("cccc actor profile upsert", text)
        self.assertIn("--profile-id", text)

    def test_best_practices_mentions_profile_backed_runtime_setup(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        doc = repo_root / "docs" / "guide" / "best-practices.md"
        text = doc.read_text(encoding="utf-8")

        self.assertIn("cccc actor profile upsert", text)
        self.assertIn("--profile-id", text)

    def test_faq_mentions_profile_management_path(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        doc = repo_root / "docs" / "guide" / "faq.md"
        text = doc.read_text(encoding="utf-8")

        self.assertIn("cccc actor profile upsert", text)
        self.assertIn("--profile-id", text)

    def test_getting_started_index_mentions_profile_backed_setup(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        doc = repo_root / "docs" / "guide" / "getting-started" / "index.md"
        text = doc.read_text(encoding="utf-8")

        self.assertIn("profile-backed", text)
        self.assertIn("cccc actor profile upsert", text)

    def test_getting_started_web_mentions_profile_backed_setup(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        doc = repo_root / "docs" / "guide" / "getting-started" / "web.md"
        text = doc.read_text(encoding="utf-8")

        self.assertIn("profile-backed", text)
        self.assertIn("cccc actor profile upsert", text)

    def test_guide_index_points_to_profile_backed_start_path(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        doc = repo_root / "docs" / "guide" / "index.md"
        text = doc.read_text(encoding="utf-8")

        self.assertIn("profile-backed", text)
        self.assertIn("cccc actor profile upsert", text)

    def test_features_mentions_cli_profile_surface(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        doc = repo_root / "docs" / "reference" / "features.md"
        text = doc.read_text(encoding="utf-8")

        self.assertIn("cccc actor profile upsert", text)
        self.assertIn("cccc actor add <actor_id> --profile-id", text)

    def test_positioning_mentions_profile_backed_runtime_model(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        doc = repo_root / "docs" / "reference" / "positioning.md"
        text = doc.read_text(encoding="utf-8")

        self.assertIn("profile-backed", text)
        self.assertIn("cccc actor profile upsert", text)

    def test_architecture_mentions_profile_backed_path(self) -> None:
        repo_root = Path(__file__).resolve().parents[1]
        doc = repo_root / "docs" / "reference" / "architecture.md"
        text = doc.read_text(encoding="utf-8")

        self.assertIn("profile-backed", text)
        self.assertIn("cccc actor add <actor_id> --profile-id", text)


if __name__ == "__main__":
    unittest.main()
