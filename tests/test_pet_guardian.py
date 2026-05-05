import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


class _FakeGroup:
    def __init__(self, group_id: str, root: Path) -> None:
        self.group_id = group_id
        self.path = root
        self.doc = {"actors": []}

    @property
    def ledger_path(self) -> Path:
        self.path.mkdir(parents=True, exist_ok=True)
        return self.path / "ledger.jsonl"


class TestPetDecisionsOps(unittest.TestCase):
    def _sample_decision(self, group_id: str) -> dict:
        return {
            "id": "decision-1",
            "kind": "suggestion",
            "priority": 80,
            "summary": "Draft the reply in chat.",
            "agent": "Pet Peer",
            "fingerprint": f"group:{group_id}:suggestion:evt-1",
            "action": {
                "type": "draft_message",
                "group_id": group_id,
                "text": "Please follow up with the user.",
                "to": ["user"],
                "reply_to": "evt-1",
            },
            "source": {
                "event_id": "evt-1",
                "suggestion_kind": "reply_required",
            },
            "updated_at": "",
        }

    def test_replace_load_and_clear_pet_decisions(self) -> None:
        from cccc.kernel.pet_decisions import (
            clear_pet_decisions,
            load_pet_decisions,
            replace_pet_decisions,
        )

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            decision = self._sample_decision(group.group_id)
            stored = replace_pet_decisions(
                group,
                decisions=[decision],
                actor_id="pet-peer",
            )
            self.assertEqual(stored, [decision])
            self.assertEqual(load_pet_decisions(group), [decision])

            clear_pet_decisions(group, actor_id="pet-peer")
            self.assertEqual(load_pet_decisions(group), [])

    def test_replace_filters_invalid_decisions(self) -> None:
        from cccc.kernel.pet_decisions import load_pet_decisions, replace_pet_decisions

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            valid = self._sample_decision(group.group_id)
            invalid = {
                "id": "",
                "kind": "suggestion",
                "summary": "missing required fields",
                "action": {"type": "draft_message"},
            }

            stored = replace_pet_decisions(
                group,
                decisions=[invalid, valid],
                actor_id="pet-peer",
            )
            self.assertEqual(stored, [valid])
            self.assertEqual(load_pet_decisions(group), [valid])

    def test_replace_compacts_foreman_draft_message_to_next_step_message(self) -> None:
        from cccc.kernel.pet_decisions import replace_pet_decisions

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            decision = {
                "id": "decision-foreman-1",
                "kind": "suggestion",
                "priority": 90,
                "summary": "Internal judgment: close the reply thread first.",
                "agent": "Pet Peer",
                "fingerprint": f"group:{group.group_id}:suggestion:reply-pressure",
                "action": {
                    "type": "draft_message",
                    "group_id": group.group_id,
                    "to": ["@foreman"],
                    "text": (
                        "The main thread is still waiting on a user-side runtime validation, inbox is empty, "
                        "and blocked/waiting_user/handoff have no new changes. "
                        "If reply pressure does not settle, prioritize the oldest overdue reply thread."
                    ),
                },
                "source": {},
                "updated_at": "",
            }

            stored = replace_pet_decisions(group, decisions=[decision], actor_id="pet-peer")

            action_text = str(stored[0]["action"]["text"] or "")
            self.assertEqual(action_text, "prioritize the oldest overdue reply thread.")
            self.assertNotIn("suggestion", stored[0])
            self.assertNotIn("suggestion_preview", stored[0])

    def test_replace_keeps_non_foreman_draft_message_text(self) -> None:
        from cccc.kernel.pet_decisions import replace_pet_decisions

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            decision = {
                "id": "decision-user-1",
                "kind": "suggestion",
                "priority": 60,
                "summary": "Ask the user for more detail.",
                "agent": "Pet Peer",
                "fingerprint": f"group:{group.group_id}:suggestion:user-followup",
                "action": {
                    "type": "draft_message",
                    "group_id": group.group_id,
                    "to": ["user"],
                    "text": "Please share reproduction steps and the error screenshot.",
                },
                "source": {},
                "updated_at": "",
            }

            stored = replace_pet_decisions(group, decisions=[decision], actor_id="pet-peer")
            self.assertEqual(str(stored[0]["action"]["text"] or ""), "Please share reproduction steps and the error screenshot.")

    def test_replace_preserves_structured_foreman_control_message(self) -> None:
        from cccc.kernel.pet_decisions import replace_pet_decisions

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            decision = {
                "id": "decision-foreman-structured",
                "kind": "suggestion",
                "priority": 88,
                "summary": "Send one structured control message to foreman.",
                "agent": "Pet Peer",
                "fingerprint": f"group:{group.group_id}:suggestion:waiting-user-structured",
                "action": {
                    "type": "draft_message",
                    "group_id": group.group_id,
                    "to": ["@foreman"],
                    "text": (
                        "Please keep T031 open for now.\n\n"
                        "- Leave it in waiting_user.\n"
                        "- I will confirm whether to continue or close after I review the latest evidence."
                    ),
                },
                "source": {},
                "updated_at": "",
            }

            stored = replace_pet_decisions(group, decisions=[decision], actor_id="pet-peer")

            self.assertEqual(
                str(stored[0]["action"]["text"] or ""),
                "Please keep T031 open for now.\n\n- Leave it in waiting_user.\n- I will confirm whether to continue or close after I review the latest evidence.",
            )

    def test_replace_keeps_task_proposal_reply_pressure_text_from_pet(self) -> None:
        from cccc.kernel.pet_decisions import replace_pet_decisions

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            decision = {
                "id": "reply-pressure-oldest-followup",
                "kind": "task_proposal",
                "priority": 90,
                "summary": "先收口最老的一条 overdue reply 线程",
                "agent": "claude-1",
                "fingerprint": "task_proposal:reply_pressure:oldest_followup",
                "action": {
                    "type": "task_proposal",
                    "group_id": group.group_id,
                    "operation": "propose",
                    "title": "收口最老一条 overdue reply 链路",
                    "assignee": "claude-1",
                    "text": "先挑最老的一条等待回复链路做收口：要么给出当前结论，要么明确缺哪条运行态证据，别继续让 overdue 堆着。",
                },
                "source": {
                    "suggestion_kind": "reply_pressure",
                },
                "updated_at": "",
            }

            stored = replace_pet_decisions(group, decisions=[decision], actor_id="pet-peer")

            self.assertEqual(str(stored[0]["summary"] or ""), "先收口最老的一条 overdue reply 线程")
            self.assertEqual(
                str(stored[0]["action"]["text"] or ""),
                "先挑最老的一条等待回复链路做收口：要么给出当前结论，要么明确缺哪条运行态证据，别继续让 overdue 堆着。",
            )

    def test_replace_filters_recently_executed_fingerprint(self) -> None:
        from cccc.kernel.pet_decisions import replace_pet_decisions
        from cccc.kernel.pet_outcomes import append_pet_decision_outcome

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            append_pet_decision_outcome(
                group,
                by="user",
                fingerprint="task_proposal:reply_pressure:oldest_followup",
                outcome="executed",
            )
            decision = {
                "id": "reply-pressure-oldest-followup",
                "kind": "task_proposal",
                "priority": 90,
                "summary": "先收口最老的一条 overdue reply 线程",
                "agent": "claude-1",
                "fingerprint": "task_proposal:reply_pressure:oldest_followup",
                "action": {
                    "type": "task_proposal",
                    "group_id": group.group_id,
                    "operation": "propose",
                    "title": "收口最老一条 overdue reply 链路",
                    "assignee": "claude-1",
                    "text": "先挑最老的一条等待回复链路做收口。",
                },
                "source": {"suggestion_kind": "reply_pressure"},
                "updated_at": "",
            }

            stored = replace_pet_decisions(group, decisions=[decision], actor_id="pet-peer")

            self.assertEqual(stored, [])

    def test_replace_resolves_fingerprint_collisions_by_suffixing_distinct_decisions(self) -> None:
        from cccc.kernel.pet_decisions import replace_pet_decisions

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            first = self._sample_decision(group.group_id)
            second = dict(first)
            second["id"] = "decision-2"
            second["summary"] = "Ask foreman to clarify the next step."
            second["action"] = {
                "type": "draft_message",
                "group_id": group.group_id,
                "text": "Please clarify the next step.",
                "to": ["@foreman"],
            }
            stored = replace_pet_decisions(group, decisions=[first, second], actor_id="pet-peer")

            self.assertEqual(len(stored), 2)
            self.assertNotEqual(str(stored[0]["fingerprint"]), str(stored[1]["fingerprint"]))
            self.assertTrue(str(stored[1]["fingerprint"]).startswith(str(first["fingerprint"])))

    def test_replace_filters_removed_send_suggestion_alias(self) -> None:
        from cccc.kernel.pet_decisions import replace_pet_decisions

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            legacy = self._sample_decision(group.group_id)
            legacy["action"] = {
                "type": "send_suggestion",
                "group_id": group.group_id,
                "text": "Please follow up with the user.",
                "to": ["user"],
            }

            stored = replace_pet_decisions(group, decisions=[legacy], actor_id="pet-peer")

            self.assertEqual(stored, [])

    def test_replace_rolls_back_when_ledger_append_fails(self) -> None:
        from cccc.daemon.pet.pet_decision_ops import handle_pet_decisions_replace
        from cccc.kernel.pet_decisions import load_pet_decisions, replace_pet_decisions

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            previous = self._sample_decision(group.group_id)
            replace_pet_decisions(group, decisions=[previous], actor_id="pet-peer")

            updated = dict(previous)
            updated["summary"] = "Updated summary"

            with patch("cccc.daemon.pet.pet_decision_ops.load_group", return_value=group), patch(
                "cccc.daemon.pet.pet_decision_ops.get_pet_actor",
                return_value={"id": "pet-peer", "internal_kind": "pet"},
            ), patch(
                "cccc.daemon.pet.pet_decision_ops.append_event",
                side_effect=RuntimeError("ledger failed"),
            ):
                resp = handle_pet_decisions_replace(
                    {
                        "group_id": group.group_id,
                        "actor_id": "pet-peer",
                        "decisions": [updated],
                    }
                )

            self.assertFalse(resp.ok)
            self.assertEqual(load_pet_decisions(group), [previous])

    def test_clear_rolls_back_when_ledger_append_fails(self) -> None:
        from cccc.daemon.pet.pet_decision_ops import handle_pet_decisions_clear
        from cccc.kernel.pet_decisions import load_pet_decisions, replace_pet_decisions

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            previous = self._sample_decision(group.group_id)
            replace_pet_decisions(group, decisions=[previous], actor_id="pet-peer")

            with patch("cccc.daemon.pet.pet_decision_ops.load_group", return_value=group), patch(
                "cccc.daemon.pet.pet_decision_ops.get_pet_actor",
                return_value={"id": "pet-peer", "internal_kind": "pet"},
            ), patch(
                "cccc.daemon.pet.pet_decision_ops.append_event",
                side_effect=RuntimeError("ledger failed"),
            ):
                resp = handle_pet_decisions_clear(
                    {
                        "group_id": group.group_id,
                        "actor_id": "pet-peer",
                    }
                )

            self.assertFalse(resp.ok)
            self.assertEqual(load_pet_decisions(group), [previous])

    def test_replace_keeps_empty_when_actor_returns_no_decisions(self) -> None:
        from cccc.daemon.pet.pet_decision_ops import handle_pet_decisions_replace

        with tempfile.TemporaryDirectory() as tmp, patch(
            "cccc.daemon.pet.pet_decision_ops.load_group",
            return_value=_FakeGroup("g-demo", Path(tmp)),
        ), patch(
            "cccc.daemon.pet.pet_decision_ops.get_pet_actor",
            return_value={"id": "pet-peer", "internal_kind": "pet"},
        ):
            resp = handle_pet_decisions_replace(
                {
                    "group_id": "g-demo",
                    "actor_id": "pet-peer",
                    "decisions": [],
                }
            )

        self.assertTrue(resp.ok, getattr(resp, "error", None))
        decisions = ((resp.result or {}).get("decisions") or [])
        self.assertEqual(decisions, [])

    def test_clear_always_clears_even_if_no_new_decisions_are_generated(self) -> None:
        from cccc.daemon.pet.pet_decision_ops import handle_pet_decisions_clear
        from cccc.kernel.pet_decisions import load_pet_decisions, replace_pet_decisions

        with tempfile.TemporaryDirectory() as tmp, patch(
            "cccc.daemon.pet.pet_decision_ops.load_group",
            return_value=_FakeGroup("g-demo", Path(tmp)),
        ), patch(
            "cccc.daemon.pet.pet_decision_ops.get_pet_actor",
            return_value={"id": "pet-peer", "internal_kind": "pet"},
        ):
            group = _FakeGroup("g-demo", Path(tmp))
            replace_pet_decisions(group, decisions=[self._sample_decision(group.group_id)], actor_id="pet-peer")
            resp = handle_pet_decisions_clear(
                {
                    "group_id": "g-demo",
                    "actor_id": "pet-peer",
                }
            )

        self.assertTrue(resp.ok, getattr(resp, "error", None))
        self.assertTrue(bool((resp.result or {}).get("cleared")))
        self.assertEqual(load_pet_decisions(group), [])


    def test_replace_marks_pet_review_job_completed(self) -> None:
        from cccc.daemon.pet.pet_decision_ops import handle_pet_decisions_replace

        with tempfile.TemporaryDirectory() as tmp, patch(
            "cccc.daemon.pet.pet_decision_ops.load_group",
            return_value=_FakeGroup("g-demo", Path(tmp)),
        ), patch(
            "cccc.daemon.pet.pet_decision_ops.get_pet_actor",
            return_value={"id": "pet-peer", "internal_kind": "pet"},
        ), patch("cccc.daemon.pet.pet_decision_ops.mark_job_completed") as mark_completed:
            resp = handle_pet_decisions_replace(
                {
                    "group_id": "g-demo",
                    "actor_id": "pet-peer",
                    "decisions": [self._sample_decision("g-demo")],
                }
            )

        self.assertTrue(resp.ok, getattr(resp, "error", None))
        mark_completed.assert_called_once()


class TestPetActorSeed(unittest.TestCase):
    def test_pet_actor_seed_uses_foreman_settings(self) -> None:
        from cccc.kernel.pet_actor import _pet_actor_seed

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            group.doc["actors"] = [
                {
                    "id": "foreman-1",
                    "runtime": "gpt-5.4",
                    "runner": "headless",
                    "command": ["codex"],
                    "env": {"FOO": "bar"},
                    "default_scope_key": "scope-a",
                    "submit": "ctrl-enter",
                },
            ]
            seed = _pet_actor_seed(group)
            self.assertEqual(seed["runtime"], "gpt-5.4")
            self.assertEqual(seed["runner"], "headless")
            self.assertEqual(seed["default_scope_key"], "scope-a")
            self.assertEqual(seed["submit"], "ctrl-enter")
            self.assertEqual(seed["env"], {"FOO": "bar"})

    def test_pet_actor_seed_requires_foreman(self) -> None:
        from cccc.kernel.pet_actor import _pet_actor_seed

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            with self.assertRaisesRegex(ValueError, "desktop pet requires a foreman actor"):
                _pet_actor_seed(group)

    def test_pet_actor_seed_uses_available_runtime_when_foreman_runtime_missing(self) -> None:
        from cccc.kernel.pet_actor import _pet_actor_seed

        with tempfile.TemporaryDirectory() as tmp, patch(
            "cccc.kernel.internal_assistant_runtime.runtime_start_preflight_error",
            side_effect=lambda runtime, command, runner="pty": "missing" if runtime == "codex" else "",
        ), patch(
            "cccc.kernel.internal_assistant_runtime.detect_runtime",
            side_effect=lambda name: type("RuntimeInfo", (), {"available": name == "claude"})(),
        ), patch(
            "cccc.kernel.internal_assistant_runtime.get_runtime_command_with_flags",
            side_effect=lambda name: [name, "--dangerously-skip-permissions"],
        ):
            group = _FakeGroup("g-demo", Path(tmp))
            group.doc["actors"] = [
                {
                    "id": "foreman-1",
                    "runtime": "codex",
                    "runner": "pty",
                    "command": ["codex"],
                },
            ]
            seed = _pet_actor_seed(group)

        self.assertEqual(seed["runtime"], "claude")
        self.assertEqual(seed["command"], ["claude", "--dangerously-skip-permissions"])


class TestPetVisibleChatBoundary(unittest.TestCase):
    def test_pet_cannot_send_visible_chat_directly(self) -> None:
        from cccc.daemon.messaging import chat_ops

        with tempfile.TemporaryDirectory() as tmp:
            fake_group = _FakeGroup("g-demo", Path(tmp))
            with patch.object(chat_ops, "load_group", return_value=fake_group), patch.object(
                chat_ops,
                "get_pet_actor",
                return_value={"id": "pet-peer", "internal_kind": "pet"},
            ):
                resp = chat_ops.handle_send(
                    {"group_id": "g-demo", "by": "pet-peer", "text": "hi"},
                    coerce_bool=bool,
                    normalize_attachments=lambda _group, _raw: [],
                    effective_runner_kind=lambda runner: runner,
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _gid, _kinds: None,
                )

        self.assertFalse(resp.ok)
        self.assertEqual(getattr(resp.error, "code", ""), "pet_visible_chat_forbidden")

    def test_pet_cannot_reply_visible_chat_directly(self) -> None:
        from cccc.daemon.messaging import chat_ops

        with tempfile.TemporaryDirectory() as tmp:
            fake_group = _FakeGroup("g-demo", Path(tmp))
            with patch.object(chat_ops, "load_group", return_value=fake_group), patch.object(
                chat_ops,
                "get_pet_actor",
                return_value={"id": "pet-peer", "internal_kind": "pet"},
            ):
                resp = chat_ops.handle_reply(
                    {"group_id": "g-demo", "by": "pet-peer", "text": "hi", "reply_to": "evt-1"},
                    coerce_bool=bool,
                    normalize_attachments=lambda _group, _raw: [],
                    effective_runner_kind=lambda runner: runner,
                    auto_wake_recipients=lambda _group, _to, _by: [],
                    automation_on_resume=lambda _group: None,
                    automation_on_new_message=lambda _group: None,
                    clear_pending_system_notifies=lambda _gid, _kinds: None,
                )

        self.assertFalse(resp.ok)
        self.assertEqual(getattr(resp.error, "code", ""), "pet_visible_chat_forbidden")


class TestPetPromptContract(unittest.TestCase):
    def test_render_pet_system_prompt_declares_outbound_message_boundary(self) -> None:
        from cccc.kernel.pet_prompt import render_pet_system_prompt

        with tempfile.TemporaryDirectory() as tmp:
            group = _FakeGroup("g-demo", Path(tmp))
            group.doc = {"title": "demo", "state": "active", "actors": []}
            prompt = render_pet_system_prompt(group, actor={"id": "pet-peer"}, context_payload={})

        self.assertIn("user-side draft-first attention assistant", prompt)
        self.assertIn("surface exactly one current highest-value recommendation", prompt)
        self.assertIn("use its review_packet as your initial focus", prompt)
        self.assertIn("Finish every pet_review with exactly one cccc_pet_decisions call", prompt)
        self.assertIn("action.text must already be the exact message the user would likely want to send next", prompt)
        self.assertIn("A draft_message may be multi-sentence or a short bullet list", prompt)
        self.assertIn("Do not call cccc_message_send, cccc_message_reply", prompt)
        self.assertIn("pet_profile_refresh", prompt)
        self.assertIn("data.context.kind=pet_profile_refresh", prompt)
        self.assertIn("do not touch cccc_pet_decisions", prompt)
        self.assertIn('cccc_agent_state(action=update, actor_id=pet-peer, user_model=...)', prompt)
        self.assertNotIn("Working Style:", prompt)
        self.assertNotIn("Pet-Specific Help:", prompt)
        self.assertNotIn("Runtime Snapshot:", prompt)
        self.assertNotIn("Reply Pressure:", prompt)
        self.assertNotIn("Coordination Rhythm:", prompt)
        self.assertNotIn("Proposal Ready:", prompt)
        self.assertNotIn("(no explicit pet persona note)", prompt)


if __name__ == "__main__":
    unittest.main()
