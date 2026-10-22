"""Check anonymous RSS/latency with operator-supplied real groups.

Example (Linux, on a disposable profiling host):
  python scripts/check_inbox_memory.py --source-home /private/copied-home \
    --binary /path/to/cccc --max-rss-growth-mib 128 --output result.json

Only group documents, ledgers/segments and Mail cursors are copied. The source
home is read-only. Enabled actors keep their identity/order/generation but use
disposable /bin/cat PTYs; commands, credentials, scopes and automation rules
from the source are omitted. Only a fresh temporary working scope is attached.
No model or browser session is started. Use network isolation too
when profiling. Results contain timings/counts/RSS, never message bodies.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import statistics
import subprocess
import tempfile
import threading
import time

import yaml


def memory_sample(pid: int) -> dict:
    memory = {}
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        name, _, value = line.partition(":")
        if name in ("VmRSS", "RssAnon"):
            memory[{"VmRSS": "rss_kib", "RssAnon": "rss_anon_kib"}[name]] = int(value.split()[0])
    if len(memory) != 2:
        raise RuntimeError("daemon has no VmRSS/RssAnon")
    memory["memory_psi"] = Path("/proc/pressure/memory").read_text().strip()
    builds = []
    for directory in Path("/proc").iterdir():
        if not directory.name.isdigit():
            continue
        try:
            name = (directory / "comm").read_text().strip()
        except (FileNotFoundError, ProcessLookupError):
            continue
        if name in ("cargo", "rustc"):
            builds.append({"pid": int(directory.name), "comm": name})
    memory["concurrent_builds"] = builds
    return memory


def copy_groups(source: Path, target: Path) -> tuple[list[tuple[str, list[str]]], dict]:
    groups = []
    registry = {}
    mail = None
    send = None
    for directory in sorted((source / "groups").iterdir()):
        if not (directory / "group.yaml").is_file():
            continue
        doc = yaml.safe_load((directory / "group.yaml").read_text())
        actors = [actor["id"] for actor in doc["actors"] if actor.get("enabled")]
        if not actors:
            raise ValueError("real replay must preserve enabled actors in every group")
        workspace = target / "replay-workspace"
        workspace.mkdir(parents=True, exist_ok=True)
        scope = {"scope_key": "s_replay", "url": str(workspace), "label": "Replay", "git_remote": ""}
        doc.update(running=True, state="active", automation={"version": 1, "rules": []}, scopes=[scope], active_scope_key="s_replay")
        for key in ("hooks", "env", "profiles", "runtime_states", "scopes_context"):
            doc.pop(key, None)
        for actor in doc["actors"]:
            actor.update(runtime="custom", runtime_mode="default", runner="pty", command=["/bin/cat"], env={}, default_scope_key="s_replay")
            for key in ("profile_id", "runtime_profile_id", "runtime_mode", "workspace", "cwd"):
                actor.pop(key, None)
        destination = target / "groups" / directory.name
        destination.mkdir(parents=True)
        (destination / "group.yaml").write_text(yaml.safe_dump(doc, sort_keys=False))
        registry[directory.name] = {
            "group_id": directory.name, "title": doc["title"],
            "topic": doc.get("topic", ""), "path": str(destination),
            "default_scope_key": "", "created_at": doc["created_at"],
            "updated_at": doc["updated_at"],
        }
        for relative in ("ledger.jsonl", "state/read_cursors.json"):
            src = directory / relative
            if src.is_file():
                dst = destination / relative
                dst.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(src, dst)
        segments = directory / "state/ledger/segments"
        if segments.is_dir():
            shutil.copytree(segments, destination / "state/ledger/segments")
        groups.append((directory.name, actors))
        if mail is None or send is None:
            with (destination / "ledger.jsonl").open() as ledger:
                for line in ledger:
                    try:
                        event = json.loads(line)
                    except ValueError:
                        continue
                    data = event.get("data", {})
                    recipients = data.get("to", [])
                    targets = [actor for actor in actors if actor in recipients]
                    mode = data.get("message_mode")
                    if event.get("kind") == "chat.message" and data.get("text") and targets:
                        replay = {"group_id": directory.name, "by": "user", "to": targets[:1], "message_mode": mode, "text": data["text"]}
                        if mode == "mail" and mail is None:
                            mail = replay
                        elif mode == "send" and send is None:
                            send = replay
                        if mail is not None and send is not None:
                            break
    if not groups or mail is None or send is None:
        raise ValueError("provide real groups with addressed Mail and Send messages")
    # The daemon discovers automation groups through this registry. Merely
    # loading group.yaml by ID can pass actor_list while every tick skips it.
    target.joinpath("registry.json").write_text(json.dumps({
        "v": 1, "created_at": min(doc["created_at"] for doc in registry.values()),
        "updated_at": max(doc["updated_at"] for doc in registry.values()),
        "groups": registry, "defaults": {},
    }, default=str))
    mail["_send"] = send
    return groups, mail


def replay(args: argparse.Namespace) -> dict:
    with tempfile.TemporaryDirectory(prefix="cccc-real-inbox-") as temporary:
        home = Path(temporary) / "home"
        groups, mail = copy_groups(args.source_home, home)
        send = mail.pop("_send")
        (home / ".cccc-rust-v1").write_text("CCCC Rust home v1\n")
        calls, metrics, errors = [], [], []
        stop = threading.Event()
        guard = threading.Lock()
        start = time.monotonic()
        log = open(Path(temporary) / "daemon.log", "wb")
        daemon = subprocess.Popen([str(args.binary), "daemon", "run", "--host", "127.0.0.1", "--port", "0"], env=dict(os.environ, CCCC_HOME=str(home), CCCC_AUTOMATION_TICK_PROBE="1", CCCC_DELIVERY_QUERY_PROBE="1", CCCC_LEDGER_INDEX_PROBE="1", RUST_LOG="warn"), stdout=log, stderr=log)

        def call(op: str, **parameters):
            before = time.monotonic()
            with socket.socket(socket.AF_UNIX) as connection:
                connection.settimeout(120)
                connection.connect(str(home / "daemon/ccccd.sock"))
                connection.sendall(json.dumps({"v": 1, "op": op, "args": parameters}).encode() + b"\n")
                response = json.loads(connection.makefile("rb").readline())
            with guard:
                calls.append({"op": op, "ms": (time.monotonic() - before) * 1000})
            if not response.get("ok"):
                raise RuntimeError(f"{op} failed: {response.get('error')}")
            return response["result"]

        def measure():
            row = {"elapsed_s": time.monotonic() - start, **memory_sample(daemon.pid)}
            metrics.append(row)
            print(json.dumps(row), flush=True)
            growth = (row["rss_anon_kib"] - metrics[0]["rss_anon_kib"]) / 1024
            if growth > args.max_rss_growth_mib:
                # A baseline need not survive its own unbounded retention for
                # five minutes before the numeric budget can reject it.
                args.output.write_text(json.dumps({
                    "complete": False, "groups": len(groups),
                    "enabled_actors": sum(len(actors) for _, actors in groups),
                    "elapsed_s": row["elapsed_s"], "rss_anon_growth_mib": growth,
                    "rss_growth_mib": (row["rss_kib"] - metrics[0]["rss_kib"]) / 1024,
                    "acceptance_metric": "RssAnon",
                    "max_rss_growth_mib": args.max_rss_growth_mib,
                    "first_over_budget": {**row, "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())},
                    "metrics": metrics, "errors": errors,
                }, indent=2))
                raise AssertionError(f"RssAnon grew {growth:.2f} MiB; budget {args.max_rss_growth_mib} MiB")

        def append():
            while not stop.wait(1):
                try:
                    # Replay an actual copied Mail through the real send op.
                    call("message_send", **mail)
                except Exception as error:
                    errors.append(str(error))
                    stop.set()

        worker = None
        try:
            # Include startup recovery in the memory budget. The published
            # address appears only after that recovery completes.
            measure()
            deadline = time.monotonic() + 30
            while not (home / "daemon/ccccd.addr.json").exists():
                if daemon.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError("daemon failed to start")
                time.sleep(0.1)
                measure()
            measure()
            discovered = {group["group_id"] for group in call("group_list")["groups"]}
            if discovered != {group_id for group_id, _ in groups}:
                raise RuntimeError("automation registry does not discover every copied group")
            enabled = 0
            restore_deadline = time.monotonic() + 90
            for group_id, actors in groups:
                while True:
                    status = call("actor_list", group_id=group_id, include_unread=False)
                    running = {actor["id"]: actor.get("running") for actor in status["actors"]}
                    if all(running.get(actor) for actor in actors):
                        break
                    if daemon.poll() is not None or time.monotonic() > restore_deadline:
                        raise RuntimeError("unread tick would filter enabled actors out")
                    measure()
                    time.sleep(0.2)
                enabled += len(actors)
            # Exercise production dispatch -> pending query -> durable claim.
            # The Send body/mode also come from the frozen real history.
            call("message_send", **send)
            workload_start = time.monotonic()
            worker = threading.Thread(target=append)
            worker.start()
            while time.monotonic() - workload_start < args.seconds and not stop.is_set():
                for group_id, actors in groups:
                    call("inbox_peek", group_id=group_id, actor_id=actors[0], limit=5)
                    call("actor_list", group_id=group_id, include_unread=True)
                measure()
                stop.wait(5)
            stop.set()
            worker.join(timeout=125)
            measure()
            summary = {}
            for op in ("inbox_peek", "actor_list", "message_send"):
                values = sorted(row["ms"] for row in calls if row["op"] == op)
                summary[op] = {"n": len(values), "p50_ms": statistics.median(values), "p95_ms": values[min(len(values) - 1, int(0.95 * len(values)))], "max_ms": max(values)}
            growth = (max(row["rss_anon_kib"] for row in metrics) - metrics[0]["rss_anon_kib"]) / 1024
            rss_growth = (max(row["rss_kib"] for row in metrics) - metrics[0]["rss_kib"]) / 1024
            ticks = {group_id: 0 for group_id, _ in groups}
            deliveries = {group_id: set() for group_id, _ in groups}
            claims = {group_id: 0 for group_id, _ in groups}
            index_queries = []
            for line in (Path(temporary) / "daemon.log").read_text(errors="replace").splitlines():
                if line.startswith("CCCC_UNREAD_TICK "):
                    tick = json.loads(line.removeprefix("CCCC_UNREAD_TICK "))
                    if tick.get("include_unread") and tick.get("success") and tick.get("eligible_actors", 0) > 0 and tick.get("group_id") in ticks:
                        ticks[tick["group_id"]] += 1
                elif line.startswith("CCCC_DELIVERY_QUERY "):
                    query = json.loads(line.removeprefix("CCCC_DELIVERY_QUERY "))
                    if query.get("group_id") in deliveries:
                        deliveries[query["group_id"]].add(query["actor_index"])
                elif line.startswith("CCCC_DELIVERY_STATE_QUERY "):
                    query = json.loads(line.removeprefix("CCCC_DELIVERY_STATE_QUERY "))
                    if query.get("group_id") in claims and query.get("actors", 0) > 0:
                        claims[query["group_id"]] += 1
                elif line.startswith("CCCC_EVENT_INDEX_QUERY "):
                    index_queries.append(json.loads(line.removeprefix("CCCC_EVENT_INDEX_QUERY ")))
            result = {"complete": True, "acceptance_metric": "RssAnon", "groups": len(groups), "enabled_actors": enabled, "elapsed_s": time.monotonic() - start, "rss_anon_growth_mib": growth, "rss_growth_mib": rss_growth, "max_rss_growth_mib": args.max_rss_growth_mib, "unread_ticks_per_group": list(ticks.values()), "restore_delivery_actor_indices": [sorted(indices) for indices in deliveries.values()], "claim_queries_per_group": list(claims.values()), "full_event_index_queries": index_queries, "metrics": metrics, "latency": summary, "errors": errors}
            args.output.write_text(json.dumps(result, indent=2))
            if errors:
                raise AssertionError("replay had API failures")
            assert all(count >= 5 for count in ticks.values()), "fewer than five successful enabled-actor unread ticks per group"
            assert all(deliveries[group_id] == set(range(len(actors))) for group_id, actors in groups), "restore did not exercise every enabled actor's delivery query"
            assert sum(claims.values()) > 0, "replay did not exercise durable delivery claims"
            assert growth <= args.max_rss_growth_mib, f"RssAnon grew {growth:.2f} MiB; budget {args.max_rss_growth_mib} MiB"
            assert not index_queries, "replay still called the full Event index; see full_event_index_queries"
            return result
        finally:
            stop.set()
            daemon.send_signal(signal.SIGINT)
            try:
                daemon.wait(timeout=20)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait(timeout=10)
            if worker is not None:
                worker.join(timeout=125)
            log.close()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-home", type=Path, required=True)
    parser.add_argument("--binary", type=lambda value: Path(value).resolve(), required=True)
    parser.add_argument("--max-rss-growth-mib", type=float, default=128, help="per-sample RssAnon growth budget; VmRSS is context")
    parser.add_argument("--seconds", type=int, default=335)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not Path("/proc/self/status").exists():
        parser.error("Linux /proc is required for this RSS check")
    if args.seconds < 305:
        parser.error("run at least five 60-second unread intervals (>=305 seconds)")
    replay(args)


if __name__ == "__main__":
    main()
