import unittest

from cccc.ports.mcp.toolspecs import MCP_TOOLS


class TestMcpToolspecSchemaGuard(unittest.TestCase):
    def test_toolspec_entries_have_required_fields(self) -> None:
        self.assertIsInstance(MCP_TOOLS, list)
        self.assertGreater(len(MCP_TOOLS), 0)
        for idx, spec in enumerate(MCP_TOOLS):
            self.assertIsInstance(spec, dict, msg=f"MCP_TOOLS[{idx}] must be dict")
            self.assertIn("name", spec, msg=f"MCP_TOOLS[{idx}] missing name")
            self.assertIn("description", spec, msg=f"MCP_TOOLS[{idx}] missing description")
            self.assertIn("inputSchema", spec, msg=f"MCP_TOOLS[{idx}] missing inputSchema")

            name = str(spec.get("name") or "").strip()
            desc = str(spec.get("description") or "").strip()
            self.assertTrue(name, msg=f"MCP_TOOLS[{idx}] empty name")
            self.assertTrue(desc, msg=f"MCP_TOOLS[{idx}] empty description")
            self.assertTrue(name.startswith("cccc_"), msg=f"MCP_TOOLS[{idx}] invalid name prefix: {name}")

    def test_input_schema_shape_is_consistent(self) -> None:
        for idx, spec in enumerate(MCP_TOOLS):
            schema = spec.get("inputSchema")
            self.assertIsInstance(schema, dict, msg=f"MCP_TOOLS[{idx}] inputSchema must be dict")
            self.assertEqual(schema.get("type"), "object", msg=f"MCP_TOOLS[{idx}] inputSchema.type must be object")
            props = schema.get("properties")
            required = schema.get("required")
            self.assertIsInstance(props, dict, msg=f"MCP_TOOLS[{idx}] inputSchema.properties must be dict")
            self.assertIsInstance(required, list, msg=f"MCP_TOOLS[{idx}] inputSchema.required must be list")

    def test_space_query_toolspec_options_are_explicit(self) -> None:
        spec = next((item for item in MCP_TOOLS if str(item.get("name") or "") == "cccc_space_query"), None)
        self.assertIsInstance(spec, dict)
        schema = spec.get("inputSchema") if isinstance(spec, dict) else {}
        self.assertIsInstance(schema, dict)
        props = schema.get("properties") if isinstance(schema, dict) else {}
        self.assertIsInstance(props, dict)
        options = props.get("options") if isinstance(props, dict) else {}
        self.assertIsInstance(options, dict)
        opt_props = options.get("properties") if isinstance(options, dict) else {}
        self.assertIsInstance(opt_props, dict)
        self.assertIn("source_ids", opt_props)
        self.assertNotIn("language", opt_props)
        self.assertNotIn("lang", opt_props)


if __name__ == "__main__":
    unittest.main()
