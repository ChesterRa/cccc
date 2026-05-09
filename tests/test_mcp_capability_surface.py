from __future__ import annotations

import unittest

from cccc.kernel.capabilities import (
    BUILTIN_CAPABILITY_PACKS,
    CAPABILITY_ADMIN_TOOLS,
    CORE_ADMIN_TOOLS,
    CORE_BASIC_TOOLS,
    CORE_TOOL_NAMES,
    SPECIALIZED_CORE_TOOL_NAMES,
)
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
        specialized_core = {str(x) for x in SPECIALIZED_CORE_TOOL_NAMES}

        self.assertTrue(core.issubset(names), msg=f"core tools missing: {sorted(core - names)}")
        self.assertTrue(pack_union.issubset(names), msg=f"pack tools missing: {sorted(pack_union - names)}")
        self.assertTrue(
            specialized_core.issubset(names),
            msg=f"specialized core tools missing: {sorted(specialized_core - names)}",
        )

        missing_mapping = sorted(names - core - pack_union - specialized_core)
        self.assertEqual(
            missing_mapping,
            [],
            msg=f"tools missing from capability surface model: {missing_mapping}",
        )

    def test_core_surface_budget_is_small(self) -> None:
        total = len(MCP_TOOLS)
        core = len(CORE_TOOL_NAMES)
        # Keep core constrained while allowing a few high-frequency tools to stay first-class.
        self.assertLessEqual(core, (total // 2) + 4, msg=f"core surface too large: core={core}, total={total}")

    def test_capability_runtime_tools_are_core_and_admin_tools_are_packaged(self) -> None:
        core = set(CORE_TOOL_NAMES)
        basic = set(CORE_BASIC_TOOLS)
        admin = set(CORE_ADMIN_TOOLS)
        capability_admin_pack = set(BUILTIN_CAPABILITY_PACKS["pack:capability-admin"]["tool_names"])

        self.assertIn("cccc_capability_search", core)
        self.assertIn("cccc_capability_enable", core)
        self.assertIn("cccc_capability_state", core)
        self.assertIn("cccc_capability_install", core)
        self.assertIn("cccc_capability_use", core)
        self.assertIn("cccc_capability_enable", basic)
        self.assertIn("cccc_capability_install", basic)
        self.assertIn("cccc_capability_use", basic)
        self.assertEqual(admin, set())
        self.assertEqual(capability_admin_pack, set(CAPABILITY_ADMIN_TOOLS))
        self.assertNotIn("cccc_capability_block", core)
        self.assertNotIn("cccc_capability_import", core)
        self.assertNotIn("cccc_capability_uninstall", core)
        self.assertIn("cccc_agent_state", core)
        self.assertIn("cccc_coordination", core)
        self.assertIn("cccc_memory", core)


if __name__ == "__main__":
    unittest.main()
