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
        spec = next((item for item in MCP_TOOLS if str(item.get("name") or "") == "cccc_space"), None)
        self.assertIsInstance(spec, dict)
        desc = str(spec.get("description") or "") if isinstance(spec, dict) else ""
        self.assertIn("status=pending|queued", desc)
        self.assertIn("wait for the later system.notify", desc)
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

    def test_memory_actions_match_reme_surface(self) -> None:
        spec = next((item for item in MCP_TOOLS if str(item.get("name") or "") == "cccc_memory"), None)
        self.assertIsInstance(spec, dict)
        schema = spec.get("inputSchema") if isinstance(spec, dict) else {}
        self.assertIsInstance(schema, dict)
        props = schema.get("properties") if isinstance(schema, dict) else {}
        self.assertIsInstance(props, dict)
        action = props.get("action") if isinstance(props, dict) else {}
        self.assertIsInstance(action, dict)
        self.assertEqual(action.get("enum"), ["layout_get", "search", "get", "write"])

    def test_memory_admin_actions_match_reme_surface(self) -> None:
        spec = next((item for item in MCP_TOOLS if str(item.get("name") or "") == "cccc_memory_admin"), None)
        self.assertIsInstance(spec, dict)
        schema = spec.get("inputSchema") if isinstance(spec, dict) else {}
        self.assertIsInstance(schema, dict)
        props = schema.get("properties") if isinstance(schema, dict) else {}
        self.assertIsInstance(props, dict)
        action = props.get("action") if isinstance(props, dict) else {}
        self.assertIsInstance(action, dict)
        self.assertEqual(
            action.get("enum"),
            ["index_sync", "context_check", "compact", "daily_flush"],
        )

    def test_messaging_toolspec_priority_matches_runtime_surface(self) -> None:
        for tool_name in ("cccc_message_send", "cccc_message_reply", "cccc_file"):
            spec = next((item for item in MCP_TOOLS if str(item.get("name") or "") == tool_name), None)
            self.assertIsInstance(spec, dict, msg=f"missing toolspec for {tool_name}")
            schema = spec.get("inputSchema") if isinstance(spec, dict) else {}
            self.assertIsInstance(schema, dict)
            props = schema.get("properties") if isinstance(schema, dict) else {}
            self.assertIsInstance(props, dict)
            priority = props.get("priority") if isinstance(props, dict) else {}
            self.assertIsInstance(priority, dict)
            self.assertEqual(priority.get("enum"), ["normal", "attention"])

        file_spec = next((item for item in MCP_TOOLS if str(item.get("name") or "") == "cccc_file"), None)
        file_schema = file_spec.get("inputSchema") if isinstance(file_spec, dict) else {}
        file_props = file_schema.get("properties") if isinstance(file_schema, dict) else {}
        action = file_props.get("action") if isinstance(file_props, dict) else {}
        self.assertEqual(action.get("enum"), ["send", "blob_path", "info", "read"])
        self.assertIn("max_bytes", file_props)

    def test_task_toolspec_exposes_type_enum(self) -> None:
        spec = next((item for item in MCP_TOOLS if str(item.get("name") or "") == "cccc_task"), None)
        self.assertIsInstance(spec, dict)
        schema = spec.get("inputSchema") if isinstance(spec, dict) else {}
        self.assertIsInstance(schema, dict)
        props = schema.get("properties") if isinstance(schema, dict) else {}
        self.assertIsInstance(props, dict)
        task_type = props.get("type") if isinstance(props, dict) else {}
        self.assertIsInstance(task_type, dict)
        self.assertEqual(
            task_type.get("enum"),
            ["free", "standard", "optimization"],
        )

    def test_remote_runtime_repo_tools_are_split_by_write_risk(self) -> None:
        repo = next((item for item in MCP_TOOLS if str(item.get("name") or "") == "cccc_repo"), None)
        repo_edit = next((item for item in MCP_TOOLS if str(item.get("name") or "") == "cccc_repo_edit"), None)
        self.assertIsInstance(repo, dict)
        self.assertIsInstance(repo_edit, dict)

        repo_annotations = repo.get("annotations") if isinstance(repo, dict) else {}
        self.assertIsInstance(repo_annotations, dict)
        self.assertTrue(repo_annotations.get("readOnlyHint"))
        repo_props = ((repo.get("inputSchema") or {}).get("properties") or {}) if isinstance(repo, dict) else {}
        self.assertEqual((repo_props.get("action") or {}).get("enum"), ["info", "list", "list_dir", "read"])
        self.assertIn("start_line", repo_props)
        self.assertIn("end_line", repo_props)

        edit_annotations = repo_edit.get("annotations") if isinstance(repo_edit, dict) else {}
        self.assertIsInstance(edit_annotations, dict)
        self.assertFalse(edit_annotations.get("readOnlyHint"))
        self.assertTrue(edit_annotations.get("destructiveHint"))
        edit_props = ((repo_edit.get("inputSchema") or {}).get("properties") or {}) if isinstance(repo_edit, dict) else {}
        self.assertEqual((edit_props.get("action") or {}).get("enum"), ["replace", "multi_replace", "write", "apply_patch", "mkdir", "delete", "move"])
        self.assertIn("old_text", edit_props)
        self.assertIn("replacements", edit_props)
        self.assertIn("expected_sha256", edit_props)
        apply_patch = next((item for item in MCP_TOOLS if str(item.get("name") or "") == "cccc_apply_patch"), None)
        self.assertIsInstance(apply_patch, dict)
        self.assertIn("Codex-style", str(apply_patch.get("description") or ""))

    def test_web_model_local_power_tools_are_not_generic_core_tools(self) -> None:
        from cccc.kernel.capabilities import CORE_BASIC_TOOLS, WEB_MODEL_CORE_TOOLS, resolve_core_tool_names

        web_model_only_tools = {
            "cccc_runtime_wait_next_turn",
            "cccc_runtime_complete_turn",
            "cccc_code_exec",
            "cccc_code_wait",
            "cccc_repo_edit",
            "cccc_apply_patch",
            "cccc_shell",
            "cccc_exec_command",
            "cccc_write_stdin",
            "cccc_git",
        }
        self.assertFalse(web_model_only_tools & set(CORE_BASIC_TOOLS))
        self.assertTrue(web_model_only_tools <= set(WEB_MODEL_CORE_TOOLS))
        self.assertFalse(web_model_only_tools & resolve_core_tool_names(actor_role="peer"))
        self.assertTrue(web_model_only_tools <= resolve_core_tool_names(actor_role="peer", is_web_model=True))

    def test_web_model_turn_tools_describe_transport_boundary(self) -> None:
        wait = next((item for item in MCP_TOOLS if str(item.get("name") or "") == "cccc_runtime_wait_next_turn"), None)
        complete = next((item for item in MCP_TOOLS if str(item.get("name") or "") == "cccc_runtime_complete_turn"), None)
        self.assertIsInstance(wait, dict)
        self.assertIsInstance(complete, dict)

        wait_desc = str(wait.get("description") or "") if isinstance(wait, dict) else ""
        complete_desc = str(complete.get("description") or "") if isinstance(complete, dict) else ""
        self.assertIn("when no turn was browser-delivered", wait_desc)
        self.assertIn("does not mark messages read", wait_desc)
        self.assertIn("whether it was browser-delivered or pulled", complete_desc)


if __name__ == "__main__":
    unittest.main()
