import json
import os
import shutil
import tempfile
import unittest


class TestDelegationRelay(unittest.TestCase):
    def _with_home(self):
        old_home = os.environ.get("CCCC_HOME")
        td_ctx = tempfile.TemporaryDirectory()
        td = td_ctx.__enter__()
        os.environ["CCCC_HOME"] = td

        def cleanup() -> None:
            try:
                td_ctx.__exit__(None, None, None)
            except OSError:
                pass
            shutil.rmtree(td, ignore_errors=True)
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

        return td, cleanup

    def _call(self, op: str, args: dict):
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        return handle_request(DaemonRequest.model_validate({"op": op, "args": args}))

    def _new_group(self, title: str) -> str:
        create, _ = self._call("group_create", {"title": title, "topic": "", "by": "user"})
        self.assertTrue(create.ok, getattr(create, "error", None))
        gid = str((create.result or {}).get("group_id") or "").strip()
        self.assertTrue(gid)
        return gid

    def _add_agent(self, group_id: str, actor_id: str) -> None:
        resp, _ = self._call(
            "actor_add",
            {"group_id": group_id, "actor_id": actor_id, "runtime": "claude", "by": "user"},
        )
        self.assertTrue(resp.ok, getattr(resp, "error", None))

    def _messages(self, group_id: str):
        from cccc.kernel.group import load_group

        group = load_group(group_id)
        assert group is not None
        events = []
        for line in group.ledger_path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line:
                continue
            try:
                ev = json.loads(line)
            except Exception:
                continue
            if isinstance(ev, dict) and str(ev.get("kind") or "") == "chat.message":
                events.append(ev)
        return events

    def test_relay_sends_v1_contract_to_target_agent(self) -> None:
        _, cleanup = self._with_home()
        try:
            src = self._new_group("src")
            dst = self._new_group("dst")
            self._add_agent(src, "需求规划专家")
            self._add_agent(dst, "dst-foreman")

            resp, _ = self._call(
                "relay_user_delegation",
                {
                    "group_id": src,
                    "dst_group_id": dst,
                    "by": "user",
                    "text": "总结两个 skill 并填到钉钉文档",
                    "contact_text": "请总结两个 skill，并把结果填到钉钉文档。",
                    "delegation_token": "self-agent",
                    "source_event_id": "ev_src_1",
                },
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            relay = (resp.result or {}).get("relay") if isinstance(resp.result, dict) else {}
            assert isinstance(relay, dict)
            self.assertTrue(str(relay.get("delegation_id") or "").startswith("dlg_"))
            self.assertEqual(relay.get("target_actor_id"), "dst-foreman")
            self.assertEqual(relay.get("sender"), "需求规划专家")
            self.assertTrue(relay.get("src_event_id"))
            self.assertTrue(relay.get("dst_event_id"))

            msgs = self._messages(dst)
            relayed = [m for m in msgs if str((m.get("data") or {}).get("src_group_id") or "") == src]
            self.assertTrue(relayed)
            ev = relayed[-1]
            data = ev.get("data") or {}
            text = str(data.get("text") or "")
            self.assertIn("[cccc-delegation:v1]", text)
            self.assertIn(f"delegation_id: {relay.get('delegation_id')}", text)
            # Communication-protocol contract (not the dropped heavy task brief).
            self.assertIn("Communication protocol:", text)
            self.assertIn("source_contact:", text)
            self.assertIn("target_contact:", text)
            self.assertIn("target_actor_id: dst-foreman", text)
            self.assertIn("Do not treat #tokens", text)
            self.assertIn("Respond to the user's intent", text)
            visible = text.split("<!-- cccc-delegation-protocol", 1)[0]
            self.assertEqual(visible.strip(), "请总结两个 skill，并把结果填到钉钉文档。")
            self.assertNotIn("用户让我来联系你", visible)
            self.assertNotIn("你好，我是来自", visible)
            self.assertNotIn("自然任务", visible)
            self.assertNotIn("不要把用户原话", visible)
            self.assertNotIn("delegation_id", visible)
            self.assertNotIn("First send an ack", text)
            self.assertNotIn("请先确认是否接收", text)
            self.assertNotIn("Task brief:", text)
            # The raw user message is preserved in the protocol reference, not
            # promoted as the natural message body to repeat.
            self.assertIn("Original user message (reference only):", text)
            self.assertIn("总结两个 skill", text)
            # Addressed to the specific target agent, never @all.
            self.assertEqual(list(data.get("to") or []), ["dst-foreman"])
            self.assertNotIn("@all", list(data.get("to") or []))
            self.assertEqual(str(ev.get("by") or ""), "需求规划专家")
        finally:
            cleanup()

    def test_relay_errors_when_no_source_agent(self) -> None:
        _, cleanup = self._with_home()
        try:
            src = self._new_group("src-empty")
            dst = self._new_group("dst")
            self._add_agent(dst, "dst-a")
            resp, _ = self._call(
                "relay_user_delegation",
                {"group_id": src, "dst_group_id": dst, "by": "user", "text": "do x"},
            )
            self.assertFalse(resp.ok)
            self.assertEqual(str(getattr(resp.error, "code", "") or ""), "no_relay_agent")
        finally:
            cleanup()

    def test_relay_errors_when_no_target_foreman(self) -> None:
        _, cleanup = self._with_home()
        try:
            src = self._new_group("src")
            dst = self._new_group("dst-empty")
            self._add_agent(src, "src-a")
            resp, _ = self._call(
                "relay_user_delegation",
                {"group_id": src, "dst_group_id": dst, "by": "user", "text": "do x"},
            )
            self.assertFalse(resp.ok)
            self.assertEqual(str(getattr(resp.error, "code", "") or ""), "no_target_foreman")
        finally:
            cleanup()

    def test_relay_honors_explicit_target_agent(self) -> None:
        _, cleanup = self._with_home()
        try:
            src = self._new_group("src")
            dst = self._new_group("dst")
            self._add_agent(src, "src-a")
            self._add_agent(dst, "dst-foreman")
            self._add_agent(dst, "dst-peer")

            resp, _ = self._call(
                "relay_user_delegation",
                {"group_id": src, "dst_group_id": dst, "by": "user", "text": "do x", "target_actor": "dst-peer"},
            )
            self.assertTrue(resp.ok, getattr(resp, "error", None))
            relay = (resp.result or {}).get("relay") if isinstance(resp.result, dict) else {}
            assert isinstance(relay, dict)
            # Explicit agent is honored, not silently re-routed to the foreman.
            self.assertEqual(relay.get("target_actor_id"), "dst-peer")
            ev = [m for m in self._messages(dst) if str((m.get("data") or {}).get("src_group_id") or "") == src][-1]
            self.assertEqual(list((ev.get("data") or {}).get("to") or []), ["dst-peer"])
        finally:
            cleanup()

    def test_relay_errors_when_explicit_target_agent_missing(self) -> None:
        _, cleanup = self._with_home()
        try:
            src = self._new_group("src")
            dst = self._new_group("dst")
            self._add_agent(src, "src-a")
            self._add_agent(dst, "dst-foreman")

            resp, _ = self._call(
                "relay_user_delegation",
                {"group_id": src, "dst_group_id": dst, "by": "user", "text": "x", "target_actor": "ghost"},
            )
            self.assertFalse(resp.ok)
            self.assertEqual(str(getattr(resp.error, "code", "") or ""), "target_agent_not_found")

            # 'user' is never a valid delegatee -> unavailable, not silent re-route.
            resp2, _ = self._call(
                "relay_user_delegation",
                {"group_id": src, "dst_group_id": dst, "by": "user", "text": "x", "target_actor": "user"},
            )
            self.assertFalse(resp2.ok)
            self.assertEqual(str(getattr(resp2.error, "code", "") or ""), "target_agent_unavailable")
        finally:
            cleanup()

    def test_relay_errors_when_target_group_missing(self) -> None:
        _, cleanup = self._with_home()
        try:
            src = self._new_group("src")
            self._add_agent(src, "src-a")
            resp, _ = self._call(
                "relay_user_delegation",
                {"group_id": src, "dst_group_id": "g_does_not_exist", "by": "user", "text": "do x"},
            )
            self.assertFalse(resp.ok)
            self.assertEqual(str(getattr(resp.error, "code", "") or ""), "group_not_found")
        finally:
            cleanup()

    def test_closed_loop_target_agent_reports_back_to_source(self) -> None:
        from cccc.daemon.messaging.delegation_contract import render_delegation_result

        _, cleanup = self._with_home()
        try:
            src = self._new_group("src")
            dst = self._new_group("dst")
            self._add_agent(src, "需求规划专家")
            self._add_agent(dst, "dst-foreman")

            relay, _ = self._call(
                "relay_user_delegation",
                {"group_id": src, "dst_group_id": dst, "by": "user", "text": "fill the doc"},
            )
            self.assertTrue(relay.ok, getattr(relay, "error", None))
            delegation_id = str(((relay.result or {}).get("relay") or {}).get("delegation_id") or "")
            self.assertTrue(delegation_id)

            # Simulate the target agent reporting the result back to the source
            # group via the standard cross-group send (not reply_to).
            result_text = render_delegation_result(
                delegation_id=delegation_id,
                source_group_id=src,
                target_group_id=dst,
                status="done",
                responder="dst-foreman",
                result="Document filled in DingTalk.",
            )
            back, _ = self._call(
                "send_cross_group",
                {
                    "group_id": dst,
                    "dst_group_id": src,
                    "by": "dst-foreman",
                    "text": result_text,
                    "to": ["需求规划专家"],
                },
            )
            self.assertTrue(back.ok, getattr(back, "error", None))

            # The source group's ledger must now observe the result, same
            # delegation_id, provenance pointing at the target group.
            src_msgs = self._messages(src)
            results = [
                m
                for m in src_msgs
                if "[cccc-delegation-result:v1]" in str((m.get("data") or {}).get("text") or "")
            ]
            self.assertTrue(results, "source group did not receive the delegation result")
            rev = results[-1]
            rdata = rev.get("data") or {}
            self.assertIn(f"delegation_id: {delegation_id}", str(rdata.get("text") or ""))
            self.assertEqual(str(rdata.get("src_group_id") or ""), dst)
            self.assertEqual(str(rev.get("by") or ""), "dst-foreman")
        finally:
            cleanup()


if __name__ == "__main__":
    unittest.main()
