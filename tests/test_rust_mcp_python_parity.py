from __future__ import annotations

import json
import re
import unittest
from pathlib import Path

from cccc.ports.mcp.toolspecs import MCP_TOOLS


class TestRustMcpPythonParity(unittest.TestCase):
    def test_python_and_rust_use_the_same_language_neutral_contract(self) -> None:
        root = Path(__file__).resolve().parents[1]
        contract_path = root / "resources/mcp_tools.json"
        contract = json.loads(contract_path.read_text(encoding="utf-8"))
        rust = (root / "crates/cccc-mcp/src/tools.rs").read_text(encoding="utf-8")
        rust_help = (root / "crates/cccc-core/src/group_prompts.rs").read_text(encoding="utf-8")

        self.assertEqual(MCP_TOOLS, contract)
        self.assertIn('include_str!("../../../resources/mcp_tools.json")', rust)
        self.assertIn('include_str!("../../../resources/cccc-help.md")', rust_help)
        self.assertFalse((root / "crates/cccc-mcp/resources/cccc-help.md").exists())
        self.assertFalse((root / "crates/cccc-web/resources/cccc-help.md").exists())
        self.assertFalse((root / "crates/cccc-mcp/src/schemas.rs").exists())

    def test_transitional_python_package_resources_match_the_canonical_files(self) -> None:
        root = Path(__file__).resolve().parents[1]
        names = [
            "cccc-help.md",
            "cccc-self-evolution.md",
            "code_mode_metadata.json",
            "code_mode_runtime.js",
            "mcp_tools.json",
        ]
        for name in names:
            self.assertEqual(
                (root / "src/cccc/resources" / name).read_bytes(),
                (root / "resources" / name).read_bytes(),
                name,
            )

    def test_python_and_rust_web_model_fixed_surfaces_match(self) -> None:
        from cccc.kernel.capabilities import WEB_MODEL_CORE_TOOLS

        root = Path(__file__).resolve().parents[1]
        rust = (root / "crates/cccc-core/src/capability_builtin.rs").read_text(encoding="utf-8")
        match = re.search(
            r"pub const WEB_MODEL_CORE_TOOL_NAMES: &\[&str\] = &\[(.*?)\];",
            rust,
            flags=re.DOTALL,
        )
        self.assertIsNotNone(match)
        rust_names = re.findall(r'"(cccc_[a-z0-9_]+)"', match.group(1) if match else "")

        self.assertEqual(rust_names, list(WEB_MODEL_CORE_TOOLS))
        self.assertEqual(len(rust_names), 33)

    def test_python_and_rust_core_message_guidance_is_identical(self) -> None:
        from cccc.kernel.system_prompt import MESSAGE_DELIVERY_GUIDANCE

        root = Path(__file__).resolve().parents[1]
        rust = (root / "crates/cccc-core/src/system_prompt.rs").read_text(encoding="utf-8")
        match = re.search(
            r'pub const MESSAGE_DELIVERY_GUIDANCE: &str = "((?:\\.|[^"\\])*)";',
            rust,
        )
        self.assertIsNotNone(match)
        encoded_guidance = match.group(1) if match else ""
        rust_guidance = json.loads(f'"{encoded_guidance}"')
        self.assertEqual(rust_guidance, MESSAGE_DELIVERY_GUIDANCE)

    def test_full_contract_has_unique_complete_entries(self) -> None:
        names = [str(tool.get("name") or "") for tool in MCP_TOOLS]
        self.assertEqual(len(names), 61)
        self.assertEqual(len(set(names)), len(names))
        for tool in MCP_TOOLS:
            self.assertEqual(set(tool) - {"annotations"}, {"name", "description", "inputSchema"})
            self.assertEqual(tool["inputSchema"].get("type"), "object")

    def test_read_only_hints_are_truthful_for_fixed_and_mixed_surfaces(self) -> None:
        by_name = {str(tool.get("name") or ""): tool for tool in MCP_TOOLS}
        for name in {
            "cccc_help",
            "cccc_bootstrap",
            "cccc_project_info",
            "cccc_repo",
            "cccc_runtime_list",
            "cccc_capability_state",
            "cccc_context_get",
            "cccc_debug",
            "cccc_message_history",
        }:
            self.assertTrue((by_name[name].get("annotations") or {}).get("readOnlyHint"), name)

        for name in {
            "cccc_capability_search",
            "cccc_file",
            "cccc_inbox_read",
            "cccc_presentation",
            "cccc_memory",
            "cccc_terminal",
            "cccc_runtime_wait_next_turn",
        }:
            self.assertIsNot((by_name[name].get("annotations") or {}).get("readOnlyHint"), True, name)


if __name__ == "__main__":
    unittest.main()
