import os
import tempfile
import unittest
from unittest.mock import patch


class TestMcpBootstrapLedgerTail(unittest.TestCase):
    def test_bootstrap_ledger_tail_respects_to_visibility(self) -> None:
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.ledger import append_event
        from cccc.kernel.registry import load_registry
        from cccc.ports.mcp import server as mcp_server
        from cccc.ports.mcp.handlers import cccc_core, cccc_group_actor
        from cccc.ports.mcp.handlers import context as cccc_context

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                reg = load_registry()
                group = create_group(reg, title="test")

                # Ensure roles are stable: first enabled actor is foreman.
                add_actor(group, actor_id="judge", enabled=True, runtime="codex", runner="pty")
                add_actor(group, actor_id="peer1", enabled=True, runtime="codex", runner="pty")

                append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="user",
                    data={"text": "b1", "to": []},
                )
                append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="user",
                    data={"text": "m2", "to": ["@foreman"]},
                )
                append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="user",
                    data={"text": "m3", "to": ["peer1"]},
                )
                append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="peer1",
                    data={"text": "m4", "to": ["user"]},
                )
                append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="judge",
                    data={"text": "m5", "to": ["user"]},
                )
                append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="user",
                    data={"text": "m6", "to": ["@peers"]},
                )

                common_patches = [
                    patch.object(
                        cccc_group_actor,
                        "group_info",
                        return_value={"group": {"group_id": group.group_id}},
                    ),
                    patch.object(cccc_group_actor, "actor_list", return_value={"actors": []}),
                    patch.object(cccc_core, "project_info", return_value={"ok": True}),
                    patch.object(cccc_context, "context_get", return_value={"ok": True}),
                    patch.object(cccc_core, "inbox_list", return_value={"messages": []}),
                ]

                for p in common_patches:
                    p.start()
                try:
                    peer_boot = mcp_server.bootstrap(
                        group_id=group.group_id,
                        actor_id="peer1",
                        ledger_tail_limit=50,
                        ledger_tail_max_chars=8000,
                    )
                    peer_texts = [m.get("text") for m in peer_boot.get("ledger_tail", [])]
                    self.assertEqual(peer_texts, ["b1", "m3", "m4", "m6"])

                    judge_boot = mcp_server.bootstrap(
                        group_id=group.group_id,
                        actor_id="judge",
                        ledger_tail_limit=50,
                        ledger_tail_max_chars=8000,
                    )
                    judge_texts = [m.get("text") for m in judge_boot.get("ledger_tail", [])]
                    self.assertEqual(judge_texts, ["b1", "m2", "m5"])
                finally:
                    for p in reversed(common_patches):
                        p.stop()
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()

