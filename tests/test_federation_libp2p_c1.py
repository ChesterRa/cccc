import json
import os
import socket
import tempfile
import unittest
from pathlib import Path


class _HomeEnv:
    def __init__(self, home: Path) -> None:
        self.home = home
        self._old = os.environ.get("CCCC_HOME")

    def __enter__(self) -> Path:
        os.environ["CCCC_HOME"] = str(self.home)
        return self.home

    def __exit__(self, exc_type, exc, tb) -> None:
        if self._old is None:
            os.environ.pop("CCCC_HOME", None)
        else:
            os.environ["CCCC_HOME"] = self._old


class TestFederationLibp2pC1(unittest.TestCase):
    def _make_group(self, home: Path, group_id: str, title: str) -> None:
        from cccc.kernel.ledger_segments import ensure_ledger_layout
        from cccc.util.fs import atomic_write_json, atomic_write_text

        group_dir = home / "groups" / group_id
        group_dir.mkdir(parents=True, exist_ok=True)
        ensure_ledger_layout(group_dir)
        atomic_write_text(
            group_dir / "group.yaml",
            "\n".join(
                [
                    "v: 1",
                    f"group_id: {group_id}",
                    f"title: {title}",
                    "state: active",
                    "running: false",
                    "active_scope_key: ''",
                    "actors: []",
                    "scopes: []",
                    "",
                ]
            ),
        )
        atomic_write_json(
            home / "registry.json",
            {
                "v": 1,
                "groups": {group_id: {"group_id": group_id, "title": title, "path": str(group_dir)}},
                "defaults": {},
            },
        )

    def _ledger_events(self, home: Path, group_id: str) -> list[dict]:
        ledger = home / "groups" / group_id / "ledger.jsonl"
        if not ledger.exists():
            return []
        return [json.loads(line) for line in ledger.read_text(encoding="utf-8").splitlines() if line.strip()]

    def test_identity_generates_stable_real_peer_id_and_listen_multiaddr(self) -> None:
        from cccc.daemon.federation.libp2p.identity import get_libp2p_identity
        from cccc.daemon.federation.libp2p.sidecar import Libp2pNode

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            first = get_libp2p_identity(home=home)
            second = get_libp2p_identity(home=home)

            self.assertEqual(first.peer_id, second.peer_id)
            self.assertTrue(first.peer_id.startswith("12D3Koo"))
            self.assertNotIn("private_key", first.public_dict())

            node = Libp2pNode(home=home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node.start()
            try:
                addrs = node.multiaddrs()
                self.assertEqual(len(addrs), 1)
                self.assertRegex(addrs[0], rf"^/ip4/127\.0\.0\.1/tcp/[0-9]+/p2p/{first.peer_id}$")
            finally:
                node.stop()

    def test_sidecar_supervisor_persists_queryable_listen_multiaddr(self) -> None:
        from cccc.daemon.federation.libp2p.supervisor import read_sidecar_status, start_sidecar

        with tempfile.TemporaryDirectory() as td:
            home = Path(td)
            node = start_sidecar(home=home)
            try:
                status = read_sidecar_status(home=home)
                self.assertEqual(status["peer_id"], node.identity.peer_id)
                self.assertEqual(status["multiaddrs"], node.multiaddrs())
                self.assertTrue(status["pid"])
                host, port = _host_port(status["multiaddrs"][0])
                with socket.create_connection((host, port), timeout=3.0) as sock:
                    self.assertTrue(sock.recv(8192))
            finally:
                node.stop()

    def test_direct_multiaddr_remote_send_writes_ledger_and_replays_duplicate(self) -> None:
        from cccc.daemon.federation.libp2p.sidecar import Libp2pNode
        from cccc.daemon.federation.ops import handle_remote_send
        from cccc.kernel.federation.pairing import approve_pairing_request, create_pairing_invite, create_pairing_request
        from cccc.kernel.federation.registration import get_registration

        with tempfile.TemporaryDirectory() as a_td, tempfile.TemporaryDirectory() as b_td:
            a_home = Path(a_td)
            b_home = Path(b_td)
            self._make_group(a_home, "g_a", "Group A")
            self._make_group(b_home, "g_b", "Group B")

            node_a = Libp2pNode(home=a_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_b = Libp2pNode(home=b_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_a.start()
            node_b.start()
            try:
                a_peer_id = node_a.identity.peer_id
                b_peer_id = node_b.identity.peer_id
                a_multiaddr = node_a.multiaddrs()[0]

                with _HomeEnv(a_home):
                    invite_a = create_pairing_invite(
                        group_id="g_a",
                        remote_group_id="g_b",
                        remote_peer_id=b_peer_id,
                        multiaddrs=node_b.multiaddrs(),
                    )
                    req_a = create_pairing_request(
                        invite_a["pairing_code"],
                        requester_group_id="g_b",
                        requester_peer_id=b_peer_id,
                        requester_multiaddrs=node_b.multiaddrs(),
                    )
                    approve_pairing_request(req_a["request_id"], approver_user_id="test")

                with _HomeEnv(b_home):
                    invite_b = create_pairing_invite(
                        group_id="g_b",
                        remote_group_id="g_a",
                        remote_peer_id=a_peer_id,
                        multiaddrs=[a_multiaddr],
                    )
                    req_b = create_pairing_request(
                        invite_b["pairing_code"],
                        requester_group_id="g_a",
                        requester_peer_id=a_peer_id,
                        requester_multiaddrs=[a_multiaddr],
                    )
                    reg = approve_pairing_request(req_b["request_id"], approver_user_id="test")["registration"]
                    resp = handle_remote_send(
                        {
                            "group_id": "g_b",
                            "registration_id": reg["registration_id"],
                            "idempotency_key": "c1-k1",
                            "payload": {"text": "hello over c1", "to": ["@foreman"]},
                        }
                    )
                    self.assertTrue(resp.ok)
                    receipt = resp.result["receipt"]
                    self.assertEqual(receipt["status"], "sent")
                    self.assertEqual(receipt["transport"], "libp2p_cccc")
                    self.assertTrue(receipt["remote_event_id"])

                    replay = handle_remote_send(
                        {
                            "group_id": "g_b",
                            "registration_id": reg["registration_id"],
                            "idempotency_key": "c1-k1",
                            "payload": {"text": "changed duplicate", "to": ["@foreman"]},
                        }
                    )
                    self.assertEqual(replay.result["receipt"]["remote_event_id"], receipt["remote_event_id"])

                events = [event for event in self._ledger_events(a_home, "g_a") if event.get("kind") == "chat.message"]
                self.assertEqual(len(events), 1)
                self.assertEqual(events[0]["data"]["text"], "hello over c1")
                self.assertEqual(events[0]["data"]["to"], ["@foreman"])
                self.assertEqual(events[0]["data"]["src_group_id"], "g_b")
                self.assertEqual(get_registration(reg["registration_id"], home=b_home)["transport"], "libp2p_cccc")
            finally:
                node_a.stop()
                node_b.stop()

    def test_remote_send_resolves_libp2p_multiaddr_from_address_book(self) -> None:
        from cccc.daemon.federation.libp2p.live_route import ensure_direct_pairing_route
        from cccc.daemon.federation.ops import handle_remote_send
        from cccc.daemon.federation.peer_address_book import record_peer_addresses
        from cccc.kernel.federation.registration import upsert_registration

        with tempfile.TemporaryDirectory() as a_td, tempfile.TemporaryDirectory() as b_td:
            a_home = Path(a_td)
            b_home = Path(b_td)
            self._make_group(a_home, "g_a", "Group A")
            self._make_group(b_home, "g_b", "Group B")

            route = ensure_direct_pairing_route(
                local_home=b_home,
                local_group_id="g_b",
                local_group_title="Group B",
                remote_home=a_home,
                remote_group_id="g_a",
                remote_group_title="Group A",
                approver_user_id="test",
            )
            try:
                with _HomeEnv(b_home):
                    original = route["local_registration"]
                    reg = upsert_registration(
                        "g_b",
                        original["url"],
                        transport="libp2p_cccc",
                        remote_group_id=original["remote_group_id"],
                        remote_peer_id=original["remote_peer_id"],
                        multiaddrs=[],
                        _approved_by_pairing=True,
                    )
                    record_peer_addresses(
                        original["remote_peer_id"],
                        route["remote_node"].multiaddrs(),
                        remote_group_id=original["remote_group_id"],
                    )
                    resp = handle_remote_send(
                        {
                            "group_id": "g_b",
                            "registration_id": reg["registration_id"],
                            "idempotency_key": "address-book-c1-k1",
                            "payload": {"text": "hello via address book", "to": ["@foreman"]},
                        }
                    )

                self.assertTrue(resp.ok)
                self.assertEqual(resp.result["receipt"]["status"], "sent")
                events = [event for event in self._ledger_events(a_home, "g_a") if event.get("kind") == "chat.message"]
                self.assertEqual([event["data"]["text"] for event in events], ["hello via address book"])
            finally:
                route["local_node"].stop()
                route["remote_node"].stop()

    def test_reply_to_received_libp2p_message_is_relayed_back_over_libp2p(self) -> None:
        from cccc.daemon.federation.libp2p.live_route import ensure_direct_pairing_route
        from cccc.daemon.federation.ops import handle_remote_send
        from cccc.contracts.v1 import DaemonRequest
        from cccc.daemon.server import handle_request

        with tempfile.TemporaryDirectory() as a_td, tempfile.TemporaryDirectory() as b_td:
            a_home = Path(a_td)
            b_home = Path(b_td)
            self._make_group(a_home, "g_a", "Group A")
            self._make_group(b_home, "g_b", "Group B")

            route = ensure_direct_pairing_route(
                local_home=b_home,
                local_group_id="g_b",
                local_group_title="Group B",
                remote_home=a_home,
                remote_group_id="g_a",
                remote_group_title="Group A",
                approver_user_id="test",
            )
            try:
                with _HomeEnv(a_home):
                    outbound = handle_remote_send(
                        {
                            "group_id": "g_a",
                            "registration_id": route["remote_registration"]["registration_id"],
                            "idempotency_key": "reply-roundtrip-a-to-b",
                            "payload": {"text": "question from A", "to": ["@foreman"]},
                        }
                    )
                self.assertTrue(outbound.ok)
                self.assertEqual(outbound.result["receipt"]["status"], "sent")

                b_events = [event for event in self._ledger_events(b_home, "g_b") if event.get("kind") == "chat.message"]
                self.assertEqual(len(b_events), 1)
                inbound_to_b = b_events[0]
                self.assertEqual(inbound_to_b["data"]["text"], "question from A")
                self.assertEqual(inbound_to_b["data"]["source_platform"], "libp2p_cccc")
                self.assertEqual(inbound_to_b["data"]["src_group_id"], "g_a")

                with _HomeEnv(b_home):
                    reply, _ = handle_request(
                        DaemonRequest.model_validate(
                            {
                                "op": "reply",
                                "args": {
                                    "group_id": "g_b",
                                    "by": "codex-1",
                                    "reply_to": str(inbound_to_b.get("id") or ""),
                                    "text": "answer from B",
                                    "to": ["user"],
                                },
                            }
                        )
                    )
                self.assertTrue(reply.ok, getattr(reply, "error", None))
                federation_reply = reply.result.get("federation_reply") if isinstance(reply.result, dict) else {}
                self.assertIsInstance(federation_reply, dict)
                self.assertEqual(federation_reply["receipt"]["status"], "sent")

                a_events = [event for event in self._ledger_events(a_home, "g_a") if event.get("kind") == "chat.message"]
                self.assertEqual([event["data"]["text"] for event in a_events], ["answer from B"])
                self.assertEqual(a_events[0]["data"]["source_platform"], "libp2p_cccc")
                self.assertEqual(a_events[0]["data"]["src_group_id"], "g_b")
            finally:
                route["local_node"].stop()
                route["remote_node"].stop()

    def test_live_route_helper_starts_nodes_persists_real_routes_and_sends(self) -> None:
        from cccc.daemon.federation.libp2p.live_route import ensure_direct_pairing_route
        from cccc.daemon.federation.libp2p.supervisor import announce_sidecar_addresses
        from cccc.daemon.federation.ops import handle_remote_send
        from cccc.kernel.federation.peer_addresses import resolve_peer_multiaddrs
        from cccc.kernel.federation.registration import upsert_registration

        with tempfile.TemporaryDirectory() as a_td, tempfile.TemporaryDirectory() as b_td:
            a_home = Path(a_td)
            b_home = Path(b_td)
            self._make_group(a_home, "g_a", "Group A")
            self._make_group(b_home, "g_b", "Group B")

            route = ensure_direct_pairing_route(
                local_home=b_home,
                local_group_id="g_b",
                local_group_title="Group B",
                remote_home=a_home,
                remote_group_id="g_a",
                remote_group_title="Group A",
                approver_user_id="test",
            )
            try:
                self.assertEqual(route["local_registration"]["transport"], "libp2p_cccc")
                self.assertEqual(route["remote_registration"]["transport"], "libp2p_cccc")
                self.assertTrue(route["local_registration"]["remote_peer_id"].startswith("12D3Koo"))
                self.assertTrue(route["remote_registration"]["remote_peer_id"].startswith("12D3Koo"))
                self.assertRegex(route["local_registration"]["multiaddrs"][0], r"^/ip4/127\.0\.0\.1/tcp/[0-9]+/p2p/12D3Koo")
                self.assertRegex(route["remote_registration"]["multiaddrs"][0], r"^/ip4/127\.0\.0\.1/tcp/[0-9]+/p2p/12D3Koo")

                with _HomeEnv(b_home):
                    resp = handle_remote_send(
                        {
                            "group_id": "g_b",
                            "registration_id": route["local_registration"]["registration_id"],
                            "idempotency_key": "live-route-k1",
                            "payload": {"text": "hello through live route", "to": ["@foreman"]},
                        }
                    )
                self.assertTrue(resp.ok)
                receipt = resp.result["receipt"]
                self.assertEqual(receipt["status"], "sent")
                self.assertEqual(receipt["transport"], "libp2p_cccc")
                self.assertTrue(receipt["remote_event_id"])

                events = [event for event in self._ledger_events(a_home, "g_a") if event.get("kind") == "chat.message"]
                self.assertEqual(len(events), 1)
                self.assertEqual(events[0]["data"]["text"], "hello through live route")
            finally:
                route["local_node"].stop()
                route["remote_node"].stop()

    def test_sidecar_address_announce_refreshes_peer_after_restart(self) -> None:
        from cccc.daemon.federation.libp2p.live_route import ensure_direct_pairing_route
        from cccc.daemon.federation.libp2p.sidecar import Libp2pNode
        from cccc.daemon.federation.libp2p.supervisor import announce_sidecar_addresses
        from cccc.daemon.federation.ops import handle_remote_send
        from cccc.kernel.federation.receipts import get_receipt
        from cccc.kernel.federation.peer_addresses import resolve_peer_multiaddrs
        from cccc.kernel.federation.pairing import list_trusts
        from cccc.kernel.federation.registration import get_registration, upsert_registration

        with tempfile.TemporaryDirectory() as a_td, tempfile.TemporaryDirectory() as b_td:
            a_home = Path(a_td)
            b_home = Path(b_td)
            self._make_group(a_home, "g_a", "Group A")
            self._make_group(b_home, "g_b", "Group B")

            route = ensure_direct_pairing_route(
                local_home=b_home,
                local_group_id="g_b",
                local_group_title="Group B",
                remote_home=a_home,
                remote_group_id="g_a",
                remote_group_title="Group A",
                approver_user_id="test",
            )
            try:
                old_a_addrs = route["remote_node"].multiaddrs()
                route["remote_node"].stop()
                restarted_a = Libp2pNode(home=a_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
                restarted_a.start()
                route["remote_node"] = restarted_a
                new_a_addrs = restarted_a.multiaddrs()
                self.assertNotEqual(old_a_addrs, new_a_addrs)

                with _HomeEnv(a_home):
                    upsert_registration(
                        "g_a",
                        route["remote_registration"]["url"],
                        transport="libp2p_cccc",
                        remote_group_id=route["remote_registration"]["remote_group_id"],
                        remote_peer_id=route["remote_registration"]["remote_peer_id"],
                        multiaddrs=[],
                        _approved_by_pairing=True,
                    )

                with _HomeEnv(b_home):
                    refreshed = upsert_registration(
                        "g_b",
                        route["local_registration"]["url"],
                        transport="libp2p_cccc",
                        remote_group_id=route["local_registration"]["remote_group_id"],
                        remote_peer_id=route["local_registration"]["remote_peer_id"],
                        multiaddrs=[],
                        _approved_by_pairing=True,
                    )
                    offline = handle_remote_send(
                        {
                            "group_id": "g_b",
                            "registration_id": refreshed["registration_id"],
                            "idempotency_key": "restart-offline-auto-retry-k1",
                            "payload": {"text": "hello queued during restart", "to": ["@foreman"]},
                        }
                    )
                self.assertTrue(offline.ok)
                self.assertEqual(offline.result["receipt"]["status"], "retrying")

                with _HomeEnv(a_home):
                    announced = announce_sidecar_addresses(restarted_a)

                self.assertEqual(announced["attempted"], 1)
                self.assertEqual(announced["sent"], 1)
                self.assertEqual(announced["retried"], 1)
                self.assertEqual(announced["trust_updates"], 1)
                self.assertEqual(announced["registration_updates"], 1)
                self.assertEqual(
                    resolve_peer_multiaddrs(restarted_a.identity.peer_id, remote_group_id="g_a", home=b_home),
                    tuple(new_a_addrs),
                )
                self.assertEqual(get_registration(refreshed["registration_id"], home=b_home)["multiaddrs"], new_a_addrs)
                refreshed_trusts = [
                    trust
                    for trust in list_trusts(group_id="g_b", home=b_home)
                    if trust.get("registration_id") == refreshed["registration_id"]
                ]
                self.assertEqual(len(refreshed_trusts), 1)
                self.assertEqual(refreshed_trusts[0]["multiaddrs"], new_a_addrs)
                self.assertEqual(
                    get_receipt(refreshed["registration_id"], "restart-offline-auto-retry-k1", home=b_home)["status"],
                    "sent",
                )

                with _HomeEnv(b_home):
                    resp = handle_remote_send(
                        {
                            "group_id": "g_b",
                            "registration_id": refreshed["registration_id"],
                            "idempotency_key": "restart-address-announce-k1",
                            "payload": {"text": "hello after restart", "to": ["@foreman"]},
                        }
                    )
                self.assertTrue(resp.ok)
                self.assertEqual(resp.result["receipt"]["status"], "sent")
                events = [event for event in self._ledger_events(a_home, "g_a") if event.get("kind") == "chat.message"]
                self.assertEqual(
                    [event["data"]["text"] for event in events],
                    ["hello queued during restart", "hello after restart"],
                )
            finally:
                route["local_node"].stop()
                route["remote_node"].stop()

    def test_unapproved_peer_is_rejected_and_does_not_write_ledger(self) -> None:
        from cccc.daemon.federation.libp2p.sidecar import Libp2pNode
        from cccc.daemon.federation.ops import handle_remote_send
        from cccc.kernel.federation.pairing import _upsert_approved_libp2p_registration

        with tempfile.TemporaryDirectory() as a_td, tempfile.TemporaryDirectory() as b_td:
            a_home = Path(a_td)
            b_home = Path(b_td)
            self._make_group(a_home, "g_a", "Group A")
            self._make_group(b_home, "g_b", "Group B")

            node_a = Libp2pNode(home=a_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_b = Libp2pNode(home=b_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_a.start()
            node_b.start()
            try:
                with _HomeEnv(b_home):
                    reg = _upsert_approved_libp2p_registration(
                        "g_b",
                        f"libp2p://{node_a.identity.peer_id}",
                        remote_group_id="g_a",
                        remote_peer_id=node_a.identity.peer_id,
                        multiaddrs=node_a.multiaddrs(),
                    )
                    resp = handle_remote_send(
                        {
                            "group_id": "g_b",
                            "registration_id": reg["registration_id"],
                            "idempotency_key": "c1-reject",
                            "payload": {"text": "should reject"},
                        }
                    )

                self.assertTrue(resp.ok)
                receipt = resp.result["receipt"]
                self.assertEqual(receipt["status"], "failed")
                self.assertEqual(receipt["error"]["code"], "unauthorized_peer")
                self.assertEqual(
                    [event for event in self._ledger_events(a_home, "g_a") if event.get("kind") == "chat.message"],
                    [],
                )
            finally:
                node_a.stop()
                node_b.stop()

    def test_http_trust_authorizes_signed_libp2p_inbound_from_same_peer(self) -> None:
        from cccc.daemon.federation.libp2p.sidecar import Libp2pNode
        from cccc.daemon.federation.ops import handle_remote_send
        from cccc.kernel.federation import pairing as pairing_kernel
        from cccc.kernel.federation.registration import upsert_registration

        with tempfile.TemporaryDirectory() as a_td, tempfile.TemporaryDirectory() as b_td:
            a_home = Path(a_td)
            b_home = Path(b_td)
            self._make_group(a_home, "g_a", "Group A")
            self._make_group(b_home, "g_b", "Group B")

            node_a = Libp2pNode(home=a_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_b = Libp2pNode(home=b_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_a.start()
            node_b.start()
            try:
                with _HomeEnv(a_home):
                    http_registration = upsert_registration(
                        "g_a",
                        "http://127.0.0.1:8858",
                        transport="peer_cccc_http",
                        remote_group_id="g_b",
                        remote_peer_id=node_b.identity.peer_id,
                    )
                    store = pairing_kernel._load_store(a_home)  # type: ignore[attr-defined]
                    store["trusts"]["ptrust_http_b"] = {
                        "trust_id": "ptrust_http_b",
                        "request_id": "",
                        "registration_id": http_registration["registration_id"],
                        "group_id": "g_a",
                        "remote_group_id": "g_b",
                        "remote_group_title": "Group B",
                        "remote_endpoint": "http://127.0.0.1:8858",
                        "remote_peer_id": node_b.identity.peer_id,
                        "multiaddrs": [],
                        "transport": "peer_cccc_http",
                        "status": "active",
                        "created_at": "2026-06-17T00:00:00Z",
                        "updated_at": "2026-06-17T00:00:00Z",
                    }
                    pairing_kernel._save_store(store, a_home)  # type: ignore[attr-defined]

                with _HomeEnv(b_home):
                    reg = pairing_kernel._upsert_approved_libp2p_registration(  # type: ignore[attr-defined]
                        "g_b",
                        f"libp2p://{node_a.identity.peer_id}",
                        remote_group_id="g_a",
                        remote_peer_id=node_a.identity.peer_id,
                        multiaddrs=node_a.multiaddrs(),
                    )
                    resp = handle_remote_send(
                        {
                            "group_id": "g_b",
                            "registration_id": reg["registration_id"],
                            "idempotency_key": "http-trust-authorizes-libp2p",
                            "payload": {"text": "hello over trusted peer", "to": ["@foreman"]},
                        }
                    )

                self.assertTrue(resp.ok)
                receipt = resp.result["receipt"]
                self.assertEqual(receipt["status"], "sent")
                events = [event for event in self._ledger_events(a_home, "g_a") if event.get("kind") == "chat.message"]
                self.assertEqual(len(events), 1)
                self.assertEqual(events[0]["data"]["text"], "hello over trusted peer")
                self.assertEqual(events[0]["data"]["source_platform"], "libp2p_cccc")
                self.assertEqual(events[0]["data"]["source_user_id"], node_b.identity.peer_id)
            finally:
                node_a.stop()
                node_b.stop()

    def test_unapproved_address_announce_is_rejected(self) -> None:
        from cccc.daemon.federation.libp2p.sidecar import Libp2pNode
        from cccc.kernel.federation.peer_addresses import resolve_peer_multiaddrs

        with tempfile.TemporaryDirectory() as a_td, tempfile.TemporaryDirectory() as b_td:
            a_home = Path(a_td)
            b_home = Path(b_td)
            self._make_group(a_home, "g_a", "Group A")
            self._make_group(b_home, "g_b", "Group B")

            node_a = Libp2pNode(home=a_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_b = Libp2pNode(home=b_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_a.start()
            node_b.start()
            try:
                result = node_b.announce_addresses(
                    multiaddr=node_a.multiaddrs()[0],
                    request={
                        "src_group_id": "g_b",
                        "remote_group_id": "g_a",
                        "remote_peer_id": node_a.identity.peer_id,
                        "multiaddrs": node_b.multiaddrs(),
                    },
                )

                self.assertFalse(result.get("ok"))
                self.assertEqual(result.get("error", {}).get("code"), "unauthorized_peer")
                self.assertEqual(
                    resolve_peer_multiaddrs(node_b.identity.peer_id, remote_group_id="g_b", home=a_home),
                    (),
                )
            finally:
                node_a.stop()
                node_b.stop()

    def test_forged_socket_peer_id_without_private_key_is_rejected(self) -> None:
        from cccc.daemon.federation.libp2p.sidecar import PROTOCOL_ID, Libp2pNode
        from cccc.kernel.federation.pairing import approve_pairing_request, create_pairing_invite, create_pairing_request

        with tempfile.TemporaryDirectory() as a_td, tempfile.TemporaryDirectory() as b_td:
            a_home = Path(a_td)
            b_home = Path(b_td)
            self._make_group(a_home, "g_a", "Group A")
            self._make_group(b_home, "g_b", "Group B")

            node_a = Libp2pNode(home=a_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_b = Libp2pNode(home=b_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_a.start()
            node_b.start()
            try:
                with _HomeEnv(a_home):
                    invite = create_pairing_invite(
                        group_id="g_a",
                        remote_group_id="g_b",
                        remote_peer_id=node_b.identity.peer_id,
                        multiaddrs=node_b.multiaddrs(),
                    )
                    request = create_pairing_request(
                        invite["pairing_code"],
                        requester_group_id="g_b",
                        requester_peer_id=node_b.identity.peer_id,
                        requester_multiaddrs=node_b.multiaddrs(),
                    )
                    approve_pairing_request(request["request_id"], approver_user_id="test")

                host, port = _host_port(node_a.multiaddrs()[0])
                forged = {
                    "protocol": PROTOCOL_ID,
                    "from_peer_id": node_b.identity.peer_id,
                    "src_group_id": "g_b",
                    "target_group_id": "g_a",
                    "idempotency_key": "forged-k1",
                    "payload": {"text": "forged without private key", "to": ["@foreman"]},
                }
                with socket.create_connection((host, port), timeout=3.0) as sock:
                    sock.settimeout(3.0)
                    challenge = json.loads(sock.recv(8192).split(b"\n", 1)[0].decode("utf-8"))
                    self.assertEqual(challenge.get("protocol"), PROTOCOL_ID)
                    self.assertTrue(challenge.get("nonce"))
                    sock.sendall((json.dumps(forged, separators=(",", ":")) + "\n").encode("utf-8"))
                    response = json.loads(sock.recv(8192).split(b"\n", 1)[0].decode("utf-8"))

                self.assertFalse(response.get("ok"))
                self.assertEqual(response.get("error", {}).get("code"), "unauthorized_peer")
                self.assertEqual(
                    [event for event in self._ledger_events(a_home, "g_a") if event.get("kind") == "chat.message"],
                    [],
                )
            finally:
                node_a.stop()
                node_b.stop()

    def test_signed_socket_cannot_claim_a_different_approved_peer_id(self) -> None:
        from cccc.daemon.federation.libp2p.identity import canonical_payload_bytes
        from cccc.daemon.federation.libp2p.sidecar import PROTOCOL_ID, Libp2pNode
        from cccc.kernel.federation.pairing import approve_pairing_request, create_pairing_invite, create_pairing_request

        with tempfile.TemporaryDirectory() as a_td, tempfile.TemporaryDirectory() as b_td, tempfile.TemporaryDirectory() as c_td:
            a_home = Path(a_td)
            b_home = Path(b_td)
            c_home = Path(c_td)
            self._make_group(a_home, "g_a", "Group A")
            self._make_group(b_home, "g_b", "Group B")
            self._make_group(c_home, "g_c", "Group C")

            node_a = Libp2pNode(home=a_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_b = Libp2pNode(home=b_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_c = Libp2pNode(home=c_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_a.start()
            node_b.start()
            node_c.start()
            try:
                with _HomeEnv(a_home):
                    invite = create_pairing_invite(
                        group_id="g_a",
                        remote_group_id="g_b",
                        remote_peer_id=node_b.identity.peer_id,
                        multiaddrs=node_b.multiaddrs(),
                    )
                    request = create_pairing_request(
                        invite["pairing_code"],
                        requester_group_id="g_b",
                        requester_peer_id=node_b.identity.peer_id,
                        requester_multiaddrs=node_b.multiaddrs(),
                    )
                    approve_pairing_request(request["request_id"], approver_user_id="test")

                host, port = _host_port(node_a.multiaddrs()[0])
                payload = {"text": "signed by C but claims B", "to": ["@foreman"]}
                body = {
                    "protocol": PROTOCOL_ID,
                    "from_peer_id": node_b.identity.peer_id,
                    "src_group_id": "g_b",
                    "target_group_id": "g_a",
                    "idempotency_key": "signed-wrong-peer",
                    "payload": payload,
                }
                with socket.create_connection((host, port), timeout=3.0) as sock:
                    sock.settimeout(3.0)
                    challenge = json.loads(sock.recv(8192).split(b"\n", 1)[0].decode("utf-8"))
                    material = {
                        "nonce": challenge["nonce"],
                        "protocol": PROTOCOL_ID,
                        "src_group_id": "g_b",
                        "target_group_id": "g_a",
                        "idempotency_key": "signed-wrong-peer",
                        "payload": payload,
                    }
                    body["public_key"] = node_c.identity.public_key_b64
                    body["signature"] = node_c.identity.sign(canonical_payload_bytes(material))
                    sock.sendall((json.dumps(body, separators=(",", ":")) + "\n").encode("utf-8"))
                    response = json.loads(sock.recv(8192).split(b"\n", 1)[0].decode("utf-8"))

                self.assertFalse(response.get("ok"))
                self.assertEqual(response.get("error", {}).get("code"), "unauthorized_peer")
                self.assertEqual(
                    [event for event in self._ledger_events(a_home, "g_a") if event.get("kind") == "chat.message"],
                    [],
                )
            finally:
                node_a.stop()
                node_b.stop()
                node_c.stop()

    def test_legacy_signed_socket_from_approved_peer_is_accepted(self) -> None:
        from cccc.daemon.federation.libp2p.identity import canonical_payload_bytes
        from cccc.daemon.federation.libp2p.sidecar import PROTOCOL_ID, Libp2pNode
        from cccc.kernel.federation.pairing import approve_pairing_request, create_pairing_invite, create_pairing_request

        with tempfile.TemporaryDirectory() as a_td, tempfile.TemporaryDirectory() as b_td:
            a_home = Path(a_td)
            b_home = Path(b_td)
            self._make_group(a_home, "g_a", "Group A")
            self._make_group(b_home, "g_b", "Group B")

            node_a = Libp2pNode(home=a_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_b = Libp2pNode(home=b_home, listen_multiaddr="/ip4/127.0.0.1/tcp/0")
            node_a.start()
            node_b.start()
            try:
                with _HomeEnv(a_home):
                    invite = create_pairing_invite(
                        group_id="g_a",
                        remote_group_id="g_b",
                        remote_peer_id=node_b.identity.peer_id,
                        multiaddrs=node_b.multiaddrs(),
                    )
                    request = create_pairing_request(
                        invite["pairing_code"],
                        requester_group_id="g_b",
                        requester_peer_id=node_b.identity.peer_id,
                        requester_multiaddrs=node_b.multiaddrs(),
                    )
                    approve_pairing_request(request["request_id"], approver_user_id="test")

                host, port = _host_port(node_a.multiaddrs()[0])
                payload = {"text": "legacy signed hello", "to": ["@foreman"]}
                body = {
                    "protocol": PROTOCOL_ID,
                    "op": "remote_send",
                    "src_group_id": "g_b",
                    "target_group_id": "g_a",
                    "idempotency_key": "legacy-signed-approved",
                    "payload": payload,
                }
                with socket.create_connection((host, port), timeout=3.0) as sock:
                    sock.settimeout(3.0)
                    challenge = json.loads(sock.recv(8192).split(b"\n", 1)[0].decode("utf-8"))
                    material = {
                        "nonce": challenge["nonce"],
                        "protocol": PROTOCOL_ID,
                        "src_group_id": "g_b",
                        "target_group_id": "g_a",
                        "idempotency_key": "legacy-signed-approved",
                        "payload": payload,
                    }
                    body["public_key"] = node_b.identity.public_key_b64
                    body["signature"] = node_b.identity.sign(canonical_payload_bytes(material))
                    sock.sendall((json.dumps(body, separators=(",", ":")) + "\n").encode("utf-8"))
                    response = json.loads(sock.recv(8192).split(b"\n", 1)[0].decode("utf-8"))

                self.assertTrue(response.get("ok"))
                events = [event for event in self._ledger_events(a_home, "g_a") if event.get("kind") == "chat.message"]
                self.assertEqual(len(events), 1)
                self.assertEqual(events[0]["data"]["text"], "legacy signed hello")
                self.assertEqual(events[0]["data"]["source_user_id"], node_b.identity.peer_id)
            finally:
                node_a.stop()
                node_b.stop()


def _host_port(multiaddr: str) -> tuple[str, int]:
    parts = [part for part in str(multiaddr).split("/") if part]
    return parts[1], int(parts[3])


if __name__ == "__main__":
    unittest.main()
