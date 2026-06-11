import unittest


class TestDelegationContract(unittest.TestCase):
    def test_request_natural_body_first_protocol_in_comment(self) -> None:
        from cccc.daemon.messaging.delegation_contract import (
            PROTOCOL_COMMENT_OPEN,
            REQUEST_MARKER,
            render_delegation_request,
        )

        text = render_delegation_request(
            delegation_id="dlg_abc",
            source_group_id="g_src",
            target_group_id="g_dst",
            original_request="总结两个 skill,跟 #self-agent 说一下填到钉钉文档",
            relay_sender="agent-a",
            target_actor_id="dst-foreman",
            contact_text="请总结两个 skill，并把结果填到钉钉文档。",
        )
        # Natural chat body comes first; the machine marker is not the first line.
        self.assertFalse(text.lstrip().startswith(REQUEST_MARKER))
        self.assertLess(text.index("请总结两个 skill"), text.index(REQUEST_MARKER))
        self.assertLess(text.index("请总结两个 skill"), text.index(PROTOCOL_COMMENT_OPEN))

        # Raw text still carries the full, parseable protocol in the comment.
        self.assertIn(PROTOCOL_COMMENT_OPEN, text)
        self.assertIn("[cccc-delegation:v1]", text)
        self.assertIn("delegation_id: dlg_abc", text)
        self.assertIn("source_contact:", text)
        self.assertIn("target_contact:", text)
        self.assertIn("target_actor_id: dst-foreman", text)
        self.assertIn("Communication protocol:", text)
        self.assertIn("Do not treat #tokens", text)
        natural = text[: text.index(PROTOCOL_COMMENT_OPEN)]
        self.assertEqual(natural.strip(), "请总结两个 skill，并把结果填到钉钉文档。")
        self.assertNotIn("#self-agent", natural)
        self.assertNotIn("用户让我来联系你", natural)
        self.assertNotIn("你好，我是来自", natural)
        self.assertNotIn("自然任务", natural)
        self.assertNotIn("不要把用户原话", natural)
        self.assertNotIn("delegation_id", natural)
        self.assertIn("Respond to the user's intent", text)
        self.assertNotIn("请先确认是否接收", text)
        self.assertNotIn("First send an ack", text)
        # No dropped heavy task-brief fields.
        for gone in ("Task brief:", "Goal:", "Expected output:", "Return instructions:"):
            self.assertNotIn(gone, text)

    def test_strip_and_extract_protocol(self) -> None:
        from cccc.daemon.messaging.delegation_contract import (
            extract_delegation_protocol,
            render_delegation_request,
            strip_delegation_protocol,
        )

        text = render_delegation_request(
            delegation_id="dlg_abc",
            source_group_id="g_src",
            target_group_id="g_dst",
            original_request="跟 #self-agent 说一下",
            relay_sender="agent-a",
            target_actor_id="dst-foreman",
            contact_text="请说一下。",
        )
        natural = strip_delegation_protocol(text)
        # Natural body is a fluent contact message, not a visible operations
        # brief. Raw route tokens stay in the hidden protocol reference.
        self.assertEqual(natural, "请说一下。")
        self.assertNotIn("#self-agent", natural)
        self.assertNotIn("dst-foreman", natural)
        self.assertNotIn("自然任务", natural)
        self.assertNotIn("不要把用户原话", natural)
        self.assertNotIn("请先确认是否接收", natural)
        self.assertNotIn("[cccc-delegation:v1]", natural)
        self.assertNotIn("delegation_id:", natural)
        self.assertNotIn("delegation_id=", natural)
        self.assertNotIn("source_contact:", natural)

        fallback = render_delegation_request(
            delegation_id="dlg_fallback",
            source_group_id="g_src",
            target_group_id="g_dst",
            original_request="跟 #self-agent 打个招呼",
            relay_sender="agent-a",
            target_actor_id="dst-foreman",
        )
        self.assertEqual(strip_delegation_protocol(fallback), "打个招呼。")

        protocol = extract_delegation_protocol(text)
        self.assertIn("[cccc-delegation:v1]", protocol)
        self.assertIn("delegation_id: dlg_abc", protocol)
        self.assertIn("source_contact:", protocol)
        self.assertIn("Original user message (reference only):", protocol)
        self.assertIn("跟 #self-agent 说一下", protocol)
        self.assertIn("Do not treat #tokens in the user message as recipients", protocol)
        self.assertIn("Do not merely confirm that the relay was received", protocol)

    def test_result_render_normalizes_status(self) -> None:
        from cccc.daemon.messaging.delegation_contract import render_delegation_result

        ok = render_delegation_result(
            delegation_id="dlg_abc", source_group_id="g_src", target_group_id="g_dst",
            status="done", responder="agent-b", result="filled the doc",
        )
        self.assertIn("[cccc-delegation-result:v1]", ok)
        self.assertIn("delegation_id: dlg_abc", ok)
        self.assertIn("status: done", ok)
        self.assertIn("filled the doc", ok)

        bad = render_delegation_result(
            delegation_id="d", source_group_id="s", target_group_id="t",
            status="weird", responder="r", result="x",
        )
        self.assertIn("status: failed", bad)

    def test_parse_roundtrip(self) -> None:
        from cccc.daemon.messaging.delegation_contract import (
            parse_delegation_marker,
            render_delegation_request,
            render_delegation_result,
        )

        req = render_delegation_request(
            delegation_id="dlg_1", source_group_id="g_src", source_event_id="ev_1",
            target_group_id="g_dst", requested_by="user", relay_sender="a", original_request="do x",
        )
        parsed = parse_delegation_marker(req)
        assert parsed is not None
        self.assertEqual(parsed["kind"], "request")
        self.assertEqual(parsed["fields"]["delegation_id"], "dlg_1")
        self.assertEqual(parsed["fields"]["source_group_id"], "g_src")
        self.assertIn("do x", parsed["body"])

        res = render_delegation_result(
            delegation_id="dlg_1", source_group_id="g_src", target_group_id="g_dst",
            status="ack", responder="b", result="on it",
        )
        parsed_res = parse_delegation_marker(res)
        assert parsed_res is not None
        self.assertEqual(parsed_res["kind"], "result")
        self.assertEqual(parsed_res["fields"]["status"], "ack")

        self.assertIsNone(parse_delegation_marker("just a normal message"))


if __name__ == "__main__":
    unittest.main()
