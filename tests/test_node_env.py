import unittest


class TestNodeEnv(unittest.TestCase):
    def test_appends_no_deprecation_by_default(self) -> None:
        from cccc.util.node_env import with_node_deprecation_warnings_suppressed

        env = with_node_deprecation_warnings_suppressed({"NODE_OPTIONS": "--max-old-space-size=4096"})

        self.assertEqual(env["NODE_OPTIONS"], "--max-old-space-size=4096 --no-deprecation")

    def test_preserves_explicit_deprecation_debug_flags(self) -> None:
        from cccc.util.node_env import with_node_deprecation_warnings_suppressed

        env = with_node_deprecation_warnings_suppressed({"NODE_OPTIONS": "--trace-deprecation"})

        self.assertEqual(env["NODE_OPTIONS"], "--trace-deprecation")

    def test_can_be_disabled(self) -> None:
        from cccc.util.node_env import with_node_deprecation_warnings_suppressed

        env = with_node_deprecation_warnings_suppressed(
            {
                "CCCC_SUPPRESS_NODE_DEPRECATION_WARNINGS": "0",
                "NODE_OPTIONS": "--max-old-space-size=4096",
            }
        )

        self.assertEqual(env["NODE_OPTIONS"], "--max-old-space-size=4096")

    def test_process_env_update_is_idempotent(self) -> None:
        from cccc.util.node_env import suppress_node_deprecation_warnings_in_process

        env = {"NODE_OPTIONS": "--max-old-space-size=4096"}
        suppress_node_deprecation_warnings_in_process(env)
        suppress_node_deprecation_warnings_in_process(env)

        self.assertEqual(env["NODE_OPTIONS"], "--max-old-space-size=4096 --no-deprecation")


if __name__ == "__main__":
    unittest.main()
