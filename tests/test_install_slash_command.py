import unittest


class TestInstallSlashCommand(unittest.TestCase):
    def test_accepts_multiple_target_shapes(self) -> None:
        from cccc.daemon.messaging.install_slash_command import parse_install_slash_command

        cases = {
            "/install https://github.com/obra/superpowers": ("https://github.com/obra/superpowers", "github"),
            "/install obra/superpowers": ("obra/superpowers", "repo_slug"),
            "/install skill:agent_self_proposed:triage": ("skill:agent_self_proposed:triage", "capability_id"),
            "/install mcp:io.github.example/server": ("mcp:io.github.example/server", "capability_id"),
            "/install ./skills/demo": ("./skills/demo", "local_path"),
            "/install context7": ("context7", "curated_or_named_skill"),
            "/install https://example.com/skill.tar.gz": ("https://example.com/skill.tar.gz", "url"),
        }

        for text, expected in cases.items():
            with self.subTest(text=text):
                parsed = parse_install_slash_command(text)
                self.assertIsNotNone(parsed)
                assert parsed is not None
                self.assertEqual(str(parsed.get("target") or ""), expected[0])
                self.assertEqual(str(parsed.get("target_kind") or ""), expected[1])

        self.assertIsNone(parse_install_slash_command("please /install context7"))

    def test_task_imports_then_enables_for_group_by_default(self) -> None:
        from cccc.daemon.messaging.install_slash_command import (
            parse_install_slash_command,
            render_install_command_task,
        )

        parsed = parse_install_slash_command("/install https://github.com/obra/superpowers")
        self.assertIsNotNone(parsed)
        assert parsed is not None

        task = render_install_command_task(parsed)

        self.assertIn("Default action: call cccc_capability_install for the target with scope=group.", task)
        self.assertIn("The install operation must import registry records from capability ids, repos, URLs, or local SKILL.md paths", task)
        self.assertIn("enable group scope; and return use-ready capability ids.", task)
        self.assertIn("Any activate, assign, autoload, or use step must operate on the imported CCCC capability record.", task)
        self.assertIn("Do not bypass the registry by installing into Codex's local skills directory.", task)
        self.assertNotIn("install or enable the requested item", task)
