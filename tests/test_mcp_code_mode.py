import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestMcpCodeMode(unittest.TestCase):
    def _with_home_and_group(self):
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import attach_scope_to_group, create_group
        from cccc.kernel.registry import load_registry
        from cccc.kernel.scope import detect_scope

        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        home = Path(td) / "home"
        workspace = Path(td) / "repo"
        home.mkdir()
        workspace.mkdir()
        os.environ["CCCC_HOME"] = str(home)
        reg = load_registry()
        group = create_group(reg, title="code-mode", topic="")
        group = attach_scope_to_group(reg, group, detect_scope(workspace), set_active=True)
        add_actor(group, actor_id="foreman1", title="Foreman", runtime="codex", runner="headless")
        add_actor(group, actor_id="peer1", title="ChatGPT Web Model", runtime="web_model", runner="headless")

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return home, workspace, group, cleanup

    def test_code_exec_orchestrates_repo_patch_shell_git_and_message_tools(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, workspace, group, cleanup = self._with_home_and_group()
        try:
            (workspace / "app.txt").write_text("one\ntwo\n", encoding="utf-8")
            import subprocess

            subprocess.run(["git", "init"], cwd=workspace, check=True, capture_output=True, text=True)
            subprocess.run(["git", "add", "app.txt"], cwd=workspace, check=True, capture_output=True, text=True)
            subprocess.run(
                ["git", "-c", "user.email=test@example.com", "-c", "user.name=Test", "commit", "-m", "init"],
                cwd=workspace,
                check=True,
                capture_output=True,
                text=True,
            )
            source = r'''
const before = await tools.cccc_repo({ action: "read", path: "app.txt" });
const patch = [
  "*** Begin Patch",
  "*** Update File: app.txt",
  "@@",
  " one",
  "-two",
  "+TWO",
  "*** End Patch",
  "",
].join("\n");
const applied = await tools.cccc_apply_patch({ patch });
const exec = await tools.cccc_exec_command({
  command: "printf exec-start; sleep 0.1; printf exec-done",
  yield_time_ms: 10,
});
let execOutput = exec.output || "";
if (exec.running && exec.session_id) {
  const poll = await tools.cccc_write_stdin({ session_id: exec.session_id, yield_time_ms: 300 });
  execOutput += poll.output || "";
}
const diff = await tools.cccc_git({ action: "diff" });
const sent = await tools.cccc_message_send({ text: "code mode report", to: ["user"] });
text(JSON.stringify({
  before: before.content,
  applied: applied.applied,
  execOutput,
  diff: diff.stdout,
  sent: sent.ok,
}));
'''

            def _fake_daemon(req, **_kwargs):
                if req.get("op") == "send":
                    return {"ok": True, "event_id": "ev_code_mode"}
                return {"ok": True}

            from cccc.ports.mcp.handlers import cccc_messaging

            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"), patch.object(
                cccc_messaging, "_call_daemon_or_raise", side_effect=_fake_daemon
            ):
                out = mcp_server.handle_tool_call("cccc_code_exec", {"source": source, "yield_time_ms": 5000})

            self.assertEqual(out.get("status"), "completed")
            self.assertFalse(out.get("running"))
            self.assertIn('"before":"one\\ntwo\\n"', str(out.get("output") or ""))
            self.assertIn('"applied":true', str(out.get("output") or ""))
            self.assertIn("exec-done", str(out.get("output") or ""))
            self.assertIn("+TWO", str(out.get("output") or ""))
            self.assertEqual((workspace / "app.txt").read_text(encoding="utf-8"), "one\nTWO\n")
        finally:
            cleanup()

    def test_exec_command_and_write_stdin_preserve_zero_yield(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"), patch.object(
                mcp_server,
                "exec_command_tool",
                return_value={"running": True, "session_id": "exec-1"},
            ) as exec_tool, patch.object(
                mcp_server,
                "write_stdin_tool",
                return_value={"running": True, "session_id": "exec-1"},
            ) as stdin_tool:
                mcp_server.handle_tool_call("cccc_exec_command", {"command": "sleep 1", "yield_time_ms": 0})
                mcp_server.handle_tool_call("cccc_write_stdin", {"session_id": "exec-1", "yield_time_ms": 0})

            self.assertEqual(exec_tool.call_args.kwargs.get("yield_time_ms"), 0)
            self.assertEqual(stdin_tool.call_args.kwargs.get("yield_time_ms"), 0)
        finally:
            cleanup()

    def test_code_exec_yields_and_code_wait_resumes(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            source = r'''
text("phase-1");
yield_control();
await new Promise((resolve) => setTimeout(resolve, 100));
text("phase-2");
'''
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                first = mcp_server.handle_tool_call("cccc_code_exec", {"source": source, "yield_time_ms": 5000})
                self.assertEqual(first.get("status"), "running")
                cell_id = str(first.get("cell_id") or "")
                self.assertTrue(cell_id)
                second = mcp_server.handle_tool_call(
                    "cccc_code_wait",
                    {"cell_id": cell_id, "yield_time_ms": 1000},
                )

            self.assertEqual(second.get("status"), "completed")
            self.assertIn("phase-2", str(second.get("output") or ""))
        finally:
            cleanup()

    def test_code_wait_can_terminate_running_cell(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            source = r'''
text("before-wait");
await new Promise(() => {});
'''
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                first = mcp_server.handle_tool_call("cccc_code_exec", {"source": source, "yield_time_ms": 50})
                self.assertEqual(first.get("status"), "running")
                cell_id = str(first.get("cell_id") or "")
                self.assertTrue(cell_id)
                terminated = mcp_server.handle_tool_call("cccc_code_wait", {"cell_id": cell_id, "terminate": True})
                missing = mcp_server.handle_tool_call("cccc_code_wait", {"cell_id": cell_id, "yield_time_ms": 1})

            self.assertEqual(terminated.get("status"), "terminated")
            self.assertEqual(missing.get("status"), "missing")
        finally:
            cleanup()

    def test_code_wait_is_bound_to_creating_actor(self) -> None:
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import load_group
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            add_actor(group, actor_id="peer2", title="Second Web Model", runtime="web_model", runner="headless")
            source = r'''
text("peer1-secret-output");
await new Promise(() => {});
'''
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                first = mcp_server.handle_tool_call("cccc_code_exec", {"source": source, "yield_time_ms": 50})
                self.assertEqual(first.get("status"), "running")
                cell_id = str(first.get("cell_id") or "")
                self.assertTrue(cell_id)

            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer2"):
                blocked_wait = mcp_server.handle_tool_call("cccc_code_wait", {"cell_id": cell_id, "yield_time_ms": 1})
                blocked_terminate = mcp_server.handle_tool_call(
                    "cccc_code_wait",
                    {"cell_id": cell_id, "terminate": True},
                )

            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                still_running = mcp_server.handle_tool_call("cccc_code_wait", {"cell_id": cell_id, "yield_time_ms": 1})
                terminated = mcp_server.handle_tool_call("cccc_code_wait", {"cell_id": cell_id, "terminate": True})

            self.assertEqual(load_group(group.group_id).group_id, group.group_id)
            self.assertEqual(blocked_wait.get("status"), "missing")
            self.assertNotIn("peer1-secret-output", str(blocked_wait.get("output") or ""))
            self.assertEqual(blocked_terminate.get("status"), "missing")
            self.assertEqual(still_running.get("status"), "running")
            self.assertEqual(terminated.get("status"), "terminated")
        finally:
            cleanup()

    def test_code_mode_store_and_load_survive_between_cells(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                first = mcp_server.handle_tool_call(
                    "cccc_code_exec",
                    {"source": 'store("answer", { value: 42 }); text("stored");', "yield_time_ms": 5000},
                )
                second = mcp_server.handle_tool_call(
                    "cccc_code_exec",
                    {"source": 'text(JSON.stringify(load("answer")));', "yield_time_ms": 5000},
                )
            self.assertEqual(first.get("status"), "completed")
            self.assertEqual(second.get("status"), "completed")
            self.assertEqual(str(second.get("output") or "").strip(), '{"value":42}')
        finally:
            cleanup()

    def test_web_model_tools_keep_direct_fallbacks_visible(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            expected = {
                "cccc_code_exec",
                "cccc_code_wait",
                "cccc_repo",
                "cccc_apply_patch",
                "cccc_exec_command",
                "cccc_write_stdin",
                "cccc_git",
                "cccc_message_send",
            }
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                names = {str(spec.get("name") or "") for spec in mcp_server.list_tools_for_caller()}
            self.assertTrue(expected.issubset(names), expected - names)
        finally:
            cleanup()

    def test_code_mode_env_kill_switch_hides_and_blocks_code_tools(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            with patch.dict(os.environ, {"CCCC_WEB_MODEL_CODE_MODE": "0"}), runtime_context_override(
                home=str(home), group_id=group.group_id, actor_id="peer1"
            ):
                names = {str(spec.get("name") or "") for spec in mcp_server.list_tools_for_caller()}
                self.assertNotIn("cccc_code_exec", names)
                self.assertNotIn("cccc_code_wait", names)
                self.assertIn("cccc_repo", names)
                with self.assertRaises(mcp_server.MCPError) as cm:
                    mcp_server.handle_tool_call("cccc_code_exec", {"source": "text('blocked')"})
            self.assertEqual(cm.exception.code, "code_mode_disabled")
        finally:
            cleanup()

    def test_code_mode_does_not_bypass_web_model_foreman_tool_gate(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            source = r'''
try {
  await tools.cccc_actor({ action: "list" });
  text("unexpected");
} catch (err) {
  text(String(err.message || err));
}
'''
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                out = mcp_server.handle_tool_call("cccc_code_exec", {"source": source, "yield_time_ms": 5000})
            self.assertEqual(out.get("status"), "completed")
            self.assertIn("requires a Web Model foreman actor", str(out.get("output") or ""))
        finally:
            cleanup()

    def test_code_mode_rejects_recursive_exec(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            source = 'text(String(ALL_TOOLS.some((tool) => tool.raw_name === "cccc_code_exec")));'
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                out = mcp_server.handle_tool_call("cccc_code_exec", {"source": source, "yield_time_ms": 5000})
            self.assertEqual(out.get("status"), "completed")
            self.assertEqual(str(out.get("output") or "").strip(), "false")
        finally:
            cleanup()

    def test_code_mode_exposes_tool_help_and_common_work_loops(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            source = r'''
const help = tool_help("repo");
const full = tool_help("repo", { detail: "schema" });
const turn = tool_help("turn_complete");
const finish = tool_help("finish_turn");
const names = tool_names("turn_complete");
const finishNames = tool_names("finish_turn");
const repoNames = tool_names("repo");
const messageNames = tool_names("message");
const files = list_tools("file");
text(JSON.stringify({
  hasRepo: help.tools.some((tool) => tool.raw_name === "cccc_repo"),
  compactByDefault: help.tools.every((tool) => tool.summary && !tool.description),
  fullHasSchemaText: full.tools.some((tool) => tool.description && tool.description.includes("inputSchema")),
  turnCompleteAlias: names[0] === "cccc_runtime_complete_turn",
  finishCurated: finishNames.slice(0, 4).join(",") === "cccc_message_reply,cccc_message_send,cccc_agent_state,cccc_runtime_complete_turn",
  finishNoRepoNoise: !finishNames.includes("cccc_apply_patch"),
  finishToolsNarrow: finish.tools.slice(0, 4).every((tool) => ["cccc_message_reply", "cccc_message_send", "cccc_agent_state", "cccc_runtime_complete_turn"].includes(tool.raw_name)),
  repoRanking: repoNames.slice(0, 4).join(",") === "cccc_repo,cccc_repo_edit,cccc_apply_patch,cccc_git",
  messageRanking: messageNames.slice(0, 2).join(",") === "cccc_message_reply,cccc_message_send",
  finishLoopAlias: turn.common_work_loops.some((loop) => loop.name === "finish_turn"),
  fileListCompact: files.some((tool) => tool.raw_name === "cccc_file" && tool.summary),
  hasPatchLoop: COMMON_WORK_LOOPS.some((loop) => loop.name === "patch_safely"),
  usage: help.usage.includes("tools.<name>"),
}));
'''
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                out = mcp_server.handle_tool_call("cccc_code_exec", {"source": source, "yield_time_ms": 5000})
            self.assertEqual(out.get("status"), "completed")
            payload = str(out.get("output") or "")
            self.assertIn('"hasRepo":true', payload)
            self.assertIn('"compactByDefault":true', payload)
            self.assertIn('"fullHasSchemaText":true', payload)
            self.assertIn('"turnCompleteAlias":true', payload)
            self.assertIn('"finishCurated":true', payload)
            self.assertIn('"finishNoRepoNoise":true', payload)
            self.assertIn('"finishToolsNarrow":true', payload)
            self.assertIn('"repoRanking":true', payload)
            self.assertIn('"messageRanking":true', payload)
            self.assertIn('"finishLoopAlias":true', payload)
            self.assertIn('"fileListCompact":true', payload)
            self.assertIn('"hasPatchLoop":true', payload)
            self.assertIn('"usage":true', payload)
        finally:
            cleanup()

    def test_code_mode_nested_errors_include_recommended_action(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, workspace, group, cleanup = self._with_home_and_group()
        try:
            (workspace / "app.txt").write_text("alpha\nbeta\n", encoding="utf-8")
            source = r'''
try {
  await tools.cccc_repo_edit({ action: "replace", path: "app.txt", old_text: "missing", new_text: "x" });
  text("unexpected");
} catch (err) {
  text(String(err.message || err));
}
'''
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                out = mcp_server.handle_tool_call("cccc_code_exec", {"source": source, "yield_time_ms": 5000})
            self.assertEqual(out.get("status"), "completed")
            output = str(out.get("output") or "")
            self.assertIn("old_text_not_found", output)
            self.assertIn("recommended_action", output)
            self.assertIn("cccc_repo(action='read')", output)
        finally:
            cleanup()

    def test_code_mode_keeps_node_host_apis_out_of_sandbox(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            source = "text([typeof console, typeof require, typeof process, typeof fetch, typeof WebSocket].join(','));"
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                out = mcp_server.handle_tool_call("cccc_code_exec", {"source": source, "yield_time_ms": 5000})
            self.assertEqual(out.get("status"), "completed")
            self.assertEqual(str(out.get("output") or "").strip(), "undefined,undefined,undefined,undefined,undefined")
        finally:
            cleanup()

    def test_code_mode_blocks_constructor_escape_to_node_process(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            source = r'''
const attempts = [
  ["global", () => this.constructor.constructor("return process")()],
  ["text", () => text.constructor.constructor("return process")()],
  ["tool", () => tools.cccc_repo.constructor.constructor("return process")()],
  ["object", () => ({}).constructor.constructor("return process")()],
];
const results = [];
for (const [name, fn] of attempts) {
  try {
    const escaped = fn();
    results.push(`${name}:${String(Boolean(escaped && escaped.env && escaped.env.HOME))}`);
  } catch (err) {
    results.push(`${name}:blocked`);
  }
}
try {
  const escaped = load("missing").constructor.constructor("return process")();
  results.push(`load:${String(Boolean(escaped && escaped.env && escaped.env.HOME))}`);
} catch (err) {
  results.push("load:blocked");
}
text(results.join(","));
'''
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                out = mcp_server.handle_tool_call("cccc_code_exec", {"source": source, "yield_time_ms": 5000})
            self.assertEqual(out.get("status"), "completed")
            self.assertEqual(
                str(out.get("output") or "").strip(),
                "global:blocked,text:blocked,tool:blocked,object:blocked,load:blocked",
            )
        finally:
            cleanup()

    def test_code_mode_rejects_require_and_import_source(self) -> None:
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        home, _workspace, group, cleanup = self._with_home_and_group()
        try:
            with runtime_context_override(home=str(home), group_id=group.group_id, actor_id="peer1"):
                with self.assertRaises(mcp_server.MCPError) as require_cm:
                    mcp_server.handle_tool_call("cccc_code_exec", {"source": "const fs = require('node:fs');"})
                with self.assertRaises(mcp_server.MCPError) as import_cm:
                    mcp_server.handle_tool_call("cccc_code_exec", {"source": "import('node:fs')"})
                ok = mcp_server.handle_tool_call(
                    "cccc_code_exec",
                    {"source": "const important = 1; text(String(important));", "yield_time_ms": 5000},
                )
            self.assertEqual(require_cm.exception.code, "unsupported_js")
            self.assertEqual(import_cm.exception.code, "unsupported_js")
            self.assertEqual(str(ok.get("output") or "").strip(), "1")
        finally:
            cleanup()

    def test_code_exec_requires_web_model_actor(self) -> None:
        from cccc.ports.mcp import server as mcp_server

        class _FakeGroup:
            pass

        with patch.object(mcp_server, "_resolve_group_id", return_value="g_test"), patch.object(
            mcp_server, "_resolve_self_actor_id", return_value="peer1"
        ), patch.object(mcp_server, "load_group", return_value=_FakeGroup()), patch.object(
            mcp_server, "find_actor", return_value={"id": "peer1", "runtime": "codex", "runner": "headless"}
        ):
            with self.assertRaises(mcp_server.MCPError) as cm:
                mcp_server.handle_tool_call("cccc_code_exec", {"source": "text('blocked')"})
        self.assertEqual(cm.exception.code, "invalid_actor_runtime")


if __name__ == "__main__":
    unittest.main()
