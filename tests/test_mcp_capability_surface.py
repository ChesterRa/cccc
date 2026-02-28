from __future__ import annotations

import unittest

from cccc.kernel.capabilities import BUILTIN_CAPABILITY_PACKS, CORE_TOOL_NAMES
from cccc.ports.mcp.toolspecs import MCP_TOOLS


class TestMcpCapabilitySurface(unittest.TestCase):
    def test_core_and_pack_coverage_matches_toolspecs(self) -> None:
        names = {str(t.get("name") or "").strip() for t in MCP_TOOLS if isinstance(t, dict)}
        core = {str(x) for x in CORE_TOOL_NAMES}
        pack_union = {
            str(tool_name)
            for pack in BUILTIN_CAPABILITY_PACKS.values()
            for tool_name in (pack.get("tool_names") or ())
        }

        self.assertTrue(core.issubset(names), msg=f"core tools missing: {sorted(core - names)}")
        self.assertTrue(pack_union.issubset(names), msg=f"pack tools missing: {sorted(pack_union - names)}")

        missing_mapping = sorted(names - core - pack_union)
        self.assertEqual(
            missing_mapping,
            [],
            msg=f"tools missing from capability surface model: {missing_mapping}",
        )

    def test_core_surface_budget_is_small(self) -> None:
        total = len(MCP_TOOLS)
        core = len(CORE_TOOL_NAMES)
        # Keep core constrained while allowing a few high-frequency tools to stay first-class.
        self.assertLessEqual(core, (total // 2) + 3, msg=f"core surface too large: core={core}, total={total}")

    def test_capability_meta_tools_are_core(self) -> None:
        core = set(CORE_TOOL_NAMES)
        self.assertIn("cccc_capability_search", core)
        self.assertIn("cccc_capability_enable", core)
        self.assertIn("cccc_capability_block", core)
        self.assertIn("cccc_capability_state", core)
        self.assertIn("cccc_capability_uninstall", core)
        self.assertIn("cccc_capability_use", core)
        self.assertIn("cccc_context_agent", core)
        self.assertIn("cccc_memory", core)


if __name__ == "__main__":
    unittest.main()
