import json
import os
import socket
import tempfile
import threading
import time
import unittest


class TestEventsStreamResumeFiltering(unittest.TestCase):
    def test_actor_view_resume_filters_visibility_and_echo(self) -> None:
        from cccc.daemon.streaming import stream_events_to_socket
        from cccc.kernel.actors import add_actor
        from cccc.kernel.group import create_group
        from cccc.kernel.ledger import append_event
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                reg = load_registry()
                group = create_group(reg, title="test")

                # First enabled actor is foreman; peer1 is a peer.
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
                    data={"text": "foreman_only", "to": ["@foreman"]},
                )
                append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="user",
                    data={"text": "to_peer1", "to": ["peer1"]},
                )
                append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="peer1",
                    data={"text": "peer1_echo", "to": ["user"]},
                )
                append_event(
                    group.ledger_path,
                    kind="system.notify",
                    group_id=group.group_id,
                    scope_key="",
                    by="system",
                    data={"kind": "info", "target_actor_id": "peer1", "title": "n_peer1", "message": "m_peer1"},
                )
                append_event(
                    group.ledger_path,
                    kind="system.notify",
                    group_id=group.group_id,
                    scope_key="",
                    by="system",
                    data={"kind": "info", "target_actor_id": "judge", "title": "n_judge", "message": "m_judge"},
                )
                append_event(
                    group.ledger_path,
                    kind="chat.ack",
                    group_id=group.group_id,
                    scope_key="",
                    by="peer1",
                    data={"actor_id": "peer1", "event_id": "01TEST"},
                )

                client, server = socket.socketpair()
                client.settimeout(2.0)

                th = threading.Thread(
                    target=stream_events_to_socket,
                    kwargs={
                        "sock": server,
                        "group_id": group.group_id,
                        "by": "peer1",
                        "since_ts": "1970-01-01T00:00:00Z",
                    },
                    daemon=True,
                )
                th.start()

                buf = b""
                got: list[tuple[str, str]] = []
                deadline = time.monotonic() + 2.0
                while time.monotonic() < deadline and len(got) < 3:
                    chunk = client.recv(65536)
                    if not chunk:
                        break
                    buf += chunk
                    while b"\n" in buf:
                        line, buf = buf.split(b"\n", 1)
                        if not line:
                            continue
                        obj = json.loads(line.decode("utf-8", errors="replace"))
                        if obj.get("t") != "event":
                            continue
                        ev = obj.get("event") or {}
                        if not isinstance(ev, dict):
                            continue
                        kind = str(ev.get("kind") or "")
                        data = ev.get("data") if isinstance(ev.get("data"), dict) else {}
                        label = str(data.get("text") or data.get("title") or "")
                        got.append((kind, label))

                try:
                    client.close()
                except Exception:
                    pass
                th.join(timeout=2.0)

                self.assertEqual(
                    got,
                    [
                        ("chat.message", "b1"),
                        ("chat.message", "to_peer1"),
                        ("system.notify", "n_peer1"),
                    ],
                )
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home

    def test_resume_falls_back_to_since_ts_when_event_id_missing(self) -> None:
        from cccc.daemon.streaming import _resume_candidates
        from cccc.kernel.group import create_group
        from cccc.kernel.ledger import append_event
        from cccc.kernel.registry import load_registry

        old_home = os.environ.get("CCCC_HOME")
        try:
            with tempfile.TemporaryDirectory() as td:
                os.environ["CCCC_HOME"] = td

                reg = load_registry()
                group = create_group(reg, title="resume-fallback")

                ev1 = append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="user",
                    data={"text": "m1", "to": []},
                )
                ev2 = append_event(
                    group.ledger_path,
                    kind="chat.message",
                    group_id=group.group_id,
                    scope_key="",
                    by="user",
                    data={"text": "m2", "to": []},
                )

                since_ts = str(ev1.get("ts") or "")
                out = _resume_candidates(
                    group.group_id,
                    since_event_id="01NONEXISTENT",
                    since_ts=since_ts,
                    kinds=None,
                )
                out_ids = [str(ev.get("id") or "") for ev in out if isinstance(ev, dict)]
                self.assertEqual(out_ids, [str(ev2.get("id") or "")])
        finally:
            if old_home is None:
                os.environ.pop("CCCC_HOME", None)
            else:
                os.environ["CCCC_HOME"] = old_home


if __name__ == "__main__":
    unittest.main()
