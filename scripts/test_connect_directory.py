#!/usr/bin/env python3
"""Cross-repository, provider-free Connect directory and messaging regression on isolated Linux Homes.

Build target/debug/cccc first. The sibling homepage's dependencies must be installed.
This starts fixture daemons, native Web routers and a SQLite-backed account handler. It does
not start actors, modify an existing Home, or use website/browser credentials.
"""

import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
import uuid
from urllib.request import Request, urlopen


ROOT = Path(__file__).resolve().parents[1]
ACCOUNT = ROOT.parent / "cccc-homepage" / "account"


def eventually(check, timeout=75):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(0.2)
    raise AssertionError("Connect fixture did not converge before its deadline")


def ipc(home, op, **args):
    address = json.loads((home / "daemon/ccccd.addr.json").read_text())
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
        stream.settimeout(5)
        stream.connect(address["path"])
        stream.sendall(json.dumps({"v": 1, "op": op, "args": args}).encode() + b"\n")
        return json.loads(stream.makefile("rb").readline())


def result(home, op, **args):
    response = ipc(home, op, **args)
    assert response["ok"], f"{op}: {response.get('error')}"
    return response["result"]


def daemon_ready(home):
    try:
        return ipc(home, "ping")["ok"]
    except (OSError, ValueError):
        return False


def free_port():
    with socket.socket() as stream:
        stream.bind(("127.0.0.1", 0))
        return stream.getsockname()[1]


def listening(port):
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=0.2):
            return True
    except OSError:
        return False


def events(home, group):
    path = home / "groups" / group / "ledger.jsonl"
    return (
        [json.loads(line) for line in path.read_text().splitlines() if line]
        if path.exists()
        else []
    )


def snapshot(home):
    path = home / "secrets/connect.json"
    return json.loads(path.read_text()) if path.exists() else {}


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=15)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def main():
    with tempfile.TemporaryDirectory(prefix="cccc-connect-directory-") as root:
        root = Path(root)
        processes = []
        logs = []
        try:
            descriptor = root / "account.json"
            account_log = (root / "account.log").open("w+")
            logs.append(account_log)
            account = subprocess.Popen(
                [
                    "node",
                    "--experimental-transform-types",
                    str(ACCOUNT / "scripts/connect-fixture.mjs"),
                    str(descriptor),
                ],
                cwd=ACCOUNT,
                stdout=account_log,
                stderr=account_log,
            )
            processes.append(account)
            eventually(lambda: descriptor.exists() or account.poll() is not None, 10)
            assert descriptor.exists(), "account fixture failed to start"
            config = json.loads(descriptor.read_text())
            homes = []

            def start(home, command=None):
                log = (root / f"{home.name}-{len(processes)}.log").open("w+")
                logs.append(log)
                process = subprocess.Popen(
                    [str(ROOT / "target/debug/cccc"), *(command or ["daemon", "run"])],
                    cwd=root,
                    env={
                        **os.environ,
                        "CCCC_HOME": str(home),
                        "CCCC_ACCOUNT_ORIGIN": config["origin"],
                    },
                    stdout=log,
                    stderr=log,
                )
                processes.append(process)
                return process

            daemons, webs, ports = [], [], []
            for device in config["devices"]:
                home = root / device["device_id"]
                (home / "secrets").mkdir(parents=True)
                (home / ".cccc-rust-v1").write_text("CCCC Rust home v1\n")
                state = home / "secrets/membership.json"
                state.write_text(
                    json.dumps(
                        {
                            **device,
                            "logged_in": True,
                            "account_origin": config["origin"],
                        }
                    )
                )
                state.chmod(0o600)
                port = free_port()
                ports.append(port)
                (home / "settings.yaml").write_text(
                    json.dumps(
                        {
                            "remote_access": {
                                "provider": "manual",
                                "enabled": True,
                                "web_public_url": f"http://127.0.0.1:{port}",
                            }
                        }
                    )
                )
                homes.append(home)
                daemons.append(start(home))
                eventually(lambda: (home / "daemon/ccccd.addr.json").exists(), 10)
                webs.append(
                    start(home, ["web", "--host", "127.0.0.1", "--port", str(port)])
                )
                eventually(lambda: listening(port), 15)
            eventually(
                lambda: all(snapshot(home).get("directory") for home in homes), 15
            )
            ids = [snapshot(home)["instance_id"] for home in homes]
            assert len(set(ids)) == 3
            print(
                "Three real daemons registered independently; Rust signatures verified by the account handler.",
                flush=True,
            )
            eventually(
                lambda: all(
                    len(snapshot(home).get("directory", {}).get("instances", [])) == 3
                    for home in homes
                )
            )
            groups = [
                result(home, "group_create", title=f"Fixture {index}")["group_id"]
                for index, home in enumerate(homes)
            ]

            def catalog_ready(index, remote):
                response = result(
                    homes[index],
                    "connect_catalog",
                    group_id=groups[index],
                    instance_id=ids[remote],
                )
                return any(
                    group["group_id"] == groups[remote]
                    for group in (response.get("catalog") or {}).get("groups", [])
                )

            eventually(
                lambda: catalog_ready(1, 2) and catalog_ready(2, 1) and catalog_ready(2, 0)
            )
            before = [snapshot(home)["checked_at"] for home in homes]
            stop(webs[0])
            stop(daemons[0])
            offline = result(
                homes[2],
                "connect_send",
                group_id=groups[2],
                instance_id=ids[0],
                target_group_id=groups[0],
                by="user",
                to=["user"],
                text="A remains offline while B and C communicate",
                message_mode="send",
                client_id=str(uuid.uuid4()),
            )
            offline_record = (
                homes[2] / "state/connect/outbox" / ids[0] / f"{offline['delivery_id']}.json"
            )
            eventually(
                lambda: json.loads(offline_record.read_text())["progress"]["attempts"] > 0,
                15,
            )
            stop(webs[1])
            stop(daemons[1])
            # C accepts known-target work while B is down. A is not a routing hub.
            pending = result(
                homes[2],
                "connect_send",
                group_id=groups[2],
                instance_id=ids[1],
                target_group_id=groups[1],
                by="user",
                to=["user"],
                text="C to offline B",
                message_mode="request_reply",
                client_id=str(uuid.uuid4()),
            )
            assert pending["accepted"] and pending["queued"]
            daemons[1] = start(homes[1])
            eventually(lambda: daemon_ready(homes[1]), 15)
            webs[1] = start(
                homes[1], ["web", "--host", "127.0.0.1", "--port", str(ports[1])]
            )
            eventually(lambda: listening(ports[1]), 15)
            eventually(lambda: snapshot(homes[1])["checked_at"] > before[1])
            assert snapshot(homes[1])["instance_id"] == ids[1]
            eventually(lambda: snapshot(homes[2])["checked_at"] > before[2])
            assert snapshot(homes[0])["checked_at"] == before[0]
            assert ipc(homes[1], "connect_status")["ok"]
            assert ipc(homes[2], "connect_status")["ok"]
            print(
                "A stopped; B restarted with the same identity; B and C refreshed without an entry Web or human token.",
                flush=True,
            )

            def received(index, delivery_id):
                return next(
                    (
                        event
                        for event in events(homes[index], groups[index])
                        if event.get("data", {})
                        .get("connect_message", {})
                        .get("delivery_id")
                        == delivery_id
                    ),
                    None,
                )

            eventually(lambda: received(1, pending["delivery_id"]))
            incoming = received(1, pending["delivery_id"])
            answer = result(
                homes[1],
                "reply",
                group_id=groups[1],
                reply_to=incoming["id"],
                by="user",
                text="B recovered and answered C",
                message_mode="send",
                client_id=str(uuid.uuid4()),
            )
            eventually(lambda: received(2, answer["delivery_id"]))
            healthy_start = time.monotonic()
            healthy = result(
                homes[2],
                "connect_send",
                group_id=groups[2],
                instance_id=ids[1],
                target_group_id=groups[1],
                by="user",
                to=["user"],
                text="Healthy B is not blocked by A's retrying work",
                message_mode="send",
                client_id=str(uuid.uuid4()),
            )
            eventually(lambda: received(1, healthy["delivery_id"]), 10)
            assert offline_record.exists(), "A's pending work must not be silently discarded"
            print(
                f"Healthy-peer delivery with A retrying: {time.monotonic() - healthy_start:.2f}s.",
                flush=True,
            )
            # A second C request exercises durable cancellation between the surviving peers.
            question = result(
                homes[2],
                "connect_send",
                group_id=groups[2],
                instance_id=ids[1],
                target_group_id=groups[1],
                by="user",
                to=["user"],
                text="Cancel this reply obligation",
                message_mode="request_reply",
                client_id=str(uuid.uuid4()),
            )
            result(
                homes[2],
                "reply_request_cancel",
                group_id=groups[2],
                source_event_id=question["source_event"]["id"],
                by="user",
            )
            eventually(lambda: received(1, question["delivery_id"]))
            event_id = received(1, question["delivery_id"])["id"]
            eventually(
                lambda: result(
                    homes[1],
                    "ledger_statuses",
                    group_id=groups[1],
                    event_ids=[event_id],
                )["statuses"][event_id]["obligation_status"]["user"]["cancelled"]
            )
            for index, delivery_id in [
                (1, pending["delivery_id"]),
                (2, answer["delivery_id"]),
                (1, healthy["delivery_id"]),
                (1, question["delivery_id"]),
            ]:
                assert (
                    len(
                        [
                            e
                            for e in events(homes[index], groups[index])
                            if e.get("data", {})
                            .get("connect_message", {})
                            .get("delivery_id")
                            == delivery_id
                        ]
                    )
                    == 1
                )
            print(
                "A stayed offline; B recovered C's accepted request, replied directly, and both peers converged cancellation without duplicate chats.",
                flush=True,
            )
            c = config["devices"][2]
            request = Request(
                config["origin"] + "/v1/device/disable",
                data=b"{}",
                headers={
                    "Authorization": "Bearer " + c["device_token"],
                    "CCCC-Membership-Version": "1",
                    "Content-Type": "application/json",
                },
            )
            with urlopen(request, timeout=5) as response:
                assert response.status == 200
            eventually(
                lambda: snapshot(homes[2]).get("error_code") == "membership_disabled"
            )
            eventually(
                lambda: all(
                    row["device_id"] != c["device_id"]
                    for row in snapshot(homes[1])["directory"]["instances"]
                )
            )
            assert (
                ipc(homes[2], "connect_status")["result"]["connect"]["directory"]
                is None
            )
            print(
                "Cloud retirement cleared C's grant and removed C from B's next directory; no tunnel was created.",
                flush=True,
            )
        except Exception:
            for log in logs:
                log.flush()
                log.seek(0)
                for line in log.readlines()[-12:]:
                    if any(
                        marker in line.lower()
                        for marker in [
                            "address already",
                            "connection refused",
                            "failed to",
                            "error:",
                            "cannot",
                            "usage:",
                        ]
                    ):
                        print(f"{Path(log.name).name}: {line.strip()}", flush=True)
            raise
        finally:
            for process in reversed(processes):
                stop(process)
            for log in logs:
                log.close()


if __name__ == "__main__":
    main()
