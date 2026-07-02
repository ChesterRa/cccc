import json
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class TestMcpAgentStateUpdate(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            td_ctx.__exit__(None, None, None)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    @staticmethod
    def _fake_call_daemon(req, timeout_s=None, paths=None):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        _ = timeout_s
        home = str(getattr(paths, "home", "") or "").strip()
        old_home = os.environ.get("CCCC_HOME")
        if home:
            os.environ["CCCC_HOME"] = home
        try:
            resp, _meta = handle_request(DaemonRequest.model_validate(req))
            if bool(resp.ok):
                return {"ok": True, "result": resp.result}
            err = resp.error
            return {
                "ok": False,
                "error": {
                    "code": str(getattr(err, "code", "") or "daemon_error"),
                    "message": str(getattr(err, "message", "") or "daemon error"),
                    "details": dict(getattr(err, "details", {}) or {}),
                },
            }
        finally:
            if home:
                if old_home is None:
                    os.environ.pop("CCCC_HOME", None)
                else:
                    os.environ["CCCC_HOME"] = old_home

    @staticmethod
    def _write_runtime_meta(home: str, group_id: str, actor_id: str, meta: dict, *, with_group_doc: bool = False) -> None:
        group_dir = Path(home) / "groups" / group_id
        state_dir = group_dir / "state"
        state_dir.mkdir(parents=True, exist_ok=True)
        if with_group_doc:
            (group_dir / "group.yaml").write_text(f"group_id: {group_id}\ntitle: shadow\n", encoding="utf-8")
            (group_dir / "ledger.jsonl").touch()
        (state_dir / "automation.json").write_text(
            json.dumps({"actors": {actor_id: dict(meta)}}, ensure_ascii=False),
            encoding="utf-8",
        )

    @staticmethod
    def _read_automation_state(home: str, group_id: str) -> dict:
        path = Path(home) / "groups" / group_id / "state" / "automation.json"
        return json.loads(path.read_text(encoding="utf-8"))

    def test_agent_state_update_returns_post_write_state_and_hygiene(self) -> None:
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        _home, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "mcp-agent-state", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer-impl",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            with patch.object(mcp_common, "call_daemon", side_effect=self._fake_call_daemon), patch.dict(
                os.environ,
                {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "peer-impl"},
                clear=False,
            ):
                out = mcp_server.handle_tool_call(
                    "cccc_agent_state",
                    {
                        "action": "update",
                        "focus": "ship exact confirmation",
                        "next_action": "verify post-write state",
                        "what_changed": "added confirmation path",
                        "open_loops": ["confirm normalized values"],
                        "commitments": ["report test evidence"],
                        "environment_summary": "clean temp home",
                        "user_model": "prefers high-signal confirmations",
                        "persona_notes": "be direct",
                    },
                )

            self.assertIn("changes", out)
            agent_state = out.get("agent_state") if isinstance(out.get("agent_state"), dict) else {}
            self.assertEqual(str(agent_state.get("id") or ""), "peer-impl")
            hot = agent_state.get("hot") if isinstance(agent_state.get("hot"), dict) else {}
            warm = agent_state.get("warm") if isinstance(agent_state.get("warm"), dict) else {}
            self.assertEqual(str(hot.get("focus") or ""), "ship exact confirmation")
            self.assertEqual(str(hot.get("next_action") or ""), "verify post-write state")
            self.assertEqual(str(warm.get("what_changed") or ""), "added confirmation path")
            self.assertEqual(warm.get("open_loops"), ["confirm normalized values"])
            self.assertEqual(warm.get("commitments"), ["report test evidence"])
            self.assertEqual(str(warm.get("environment_summary") or ""), "clean temp home")
            self.assertEqual(str(warm.get("user_model") or ""), "prefers high-signal confirmations")
            self.assertEqual(str(warm.get("persona_notes") or ""), "be direct")

            hygiene = out.get("context_hygiene") if isinstance(out.get("context_hygiene"), dict) else {}
            self.assertEqual(str(hygiene.get("actor_id") or ""), "peer-impl")
            self.assertTrue(bool(hygiene.get("present")))
            self.assertEqual(str(hygiene.get("recommendation") or ""), "state_healthy")
            self.assertEqual(str((hygiene.get("execution_health") or {}).get("status") or ""), "ready")
            self.assertEqual(str((hygiene.get("mind_context_health") or {}).get("status") or ""), "ready")
        finally:
            cleanup()

    def test_agent_state_update_confirmation_reads_runtime_override_home(self) -> None:
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.common import runtime_context_override

        default_home, cleanup = self._with_home()
        override_ctx = tempfile.TemporaryDirectory()
        override_home = override_ctx.__enter__()
        try:
            with patch.dict(os.environ, {"CCCC_HOME": override_home}, clear=False):
                create_resp, _ = self._call("group_create", {"title": "override-agent-state", "topic": "", "by": "user"})
                self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
                group_id = str((create_resp.result or {}).get("group_id") or "").strip()
                self.assertTrue(group_id)
                add_resp, _ = self._call(
                    "actor_add",
                    {
                        "group_id": group_id,
                        "actor_id": "peer-impl",
                        "runtime": "codex",
                        "runner": "headless",
                        "by": "user",
                    },
                )
                self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            self._write_runtime_meta(
                default_home,
                group_id,
                "peer-impl",
                {
                    "mind_context_touched_at": "2000-01-01T00:00:00Z",
                    "hot_only_updates_since_mind_touch": 9,
                },
                with_group_doc=True,
            )

            with patch.object(mcp_common, "call_daemon", side_effect=self._fake_call_daemon), runtime_context_override(
                home=override_home,
                group_id=group_id,
                actor_id="peer-impl",
            ):
                out = mcp_server.handle_tool_call(
                    "cccc_agent_state",
                    {
                        "action": "update",
                        "focus": "confirm override home",
                        "next_action": "inspect hygiene source",
                        "what_changed": "wrote through override home",
                        "environment_summary": "override runtime home",
                        "user_model": "expects scoped confirmation",
                        "persona_notes": "avoid cross-home reads",
                    },
                )

            hygiene = out.get("context_hygiene") if isinstance(out.get("context_hygiene"), dict) else {}
            mind = hygiene.get("mind_context_health") if isinstance(hygiene.get("mind_context_health"), dict) else {}
            self.assertEqual(mind.get("hot_only_updates_since_touch"), 0)
            self.assertEqual(str(mind.get("status") or ""), "ready")
        finally:
            override_ctx.__exit__(None, None, None)
            cleanup()

    def test_agent_state_update_confirmation_tolerates_malformed_runtime_counter(self) -> None:
        from cccc.ports.mcp import common as mcp_common
        from cccc.ports.mcp import server as mcp_server

        home, cleanup = self._with_home()
        try:
            create_resp, _ = self._call("group_create", {"title": "mcp-agent-state-counter", "topic": "", "by": "user"})
            self.assertTrue(create_resp.ok, getattr(create_resp, "error", None))
            group_id = str((create_resp.result or {}).get("group_id") or "").strip()
            self.assertTrue(group_id)

            add_resp, _ = self._call(
                "actor_add",
                {
                    "group_id": group_id,
                    "actor_id": "peer-impl",
                    "runtime": "codex",
                    "runner": "headless",
                    "by": "user",
                },
            )
            self.assertTrue(add_resp.ok, getattr(add_resp, "error", None))

            payload = {
                "action": "update",
                "focus": "repeat confirmation",
                "next_action": "keep existing plan",
                "what_changed": "state already written",
                "environment_summary": "stable env",
                "user_model": "same user model",
                "persona_notes": "same persona",
            }
            with patch.object(mcp_common, "call_daemon", side_effect=self._fake_call_daemon), patch.dict(
                os.environ,
                {"CCCC_GROUP_ID": group_id, "CCCC_ACTOR_ID": "peer-impl"},
                clear=False,
            ):
                first = mcp_server.handle_tool_call("cccc_agent_state", payload)
                self.assertNotIn("post_update_warning", first)

                state = self._read_automation_state(home, group_id)
                state.setdefault("actors", {}).setdefault("peer-impl", {})["hot_only_updates_since_mind_touch"] = "not-a-number"
                (Path(home) / "groups" / group_id / "state" / "automation.json").write_text(
                    json.dumps(state, ensure_ascii=False),
                    encoding="utf-8",
                )

                out = mcp_server.handle_tool_call("cccc_agent_state", payload)

            self.assertNotIn("post_update_warning", out)
            hygiene = out.get("context_hygiene") if isinstance(out.get("context_hygiene"), dict) else {}
            mind = hygiene.get("mind_context_health") if isinstance(hygiene.get("mind_context_health"), dict) else {}
            self.assertEqual(mind.get("hot_only_updates_since_touch"), 0)
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
