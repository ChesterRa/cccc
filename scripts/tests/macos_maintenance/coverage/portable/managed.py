"""Hold a structured Agent turn through three maintenance entry points with a protocol stand-in."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import traceback
import uuid

HERE = Path(__file__).resolve().parent
shared = HERE / "update_keep.py"
standin = HERE / "standin.py"
spec = importlib.util.spec_from_file_location("update_keep", shared)
u = importlib.util.module_from_spec(spec)
spec.loader.exec_module(u)
ACTORS = ("structured", "deepseek")


def seed_deepseek(home):
    """Only package metadata is consumed by the stand-in; production creates its own profile."""
    version = "0.1.0-rc.6"
    packages = {
        f"@deepseek-ai/{name}": version
        for name in ("dsh-acp", "dsh-acp-demo", "dsh-mcp-client", "dsh-llm-deepseek")
    }
    folder = home / "runtimes/deepseek" / version
    folder.mkdir(parents=True)
    package_bin = folder / "node_modules/.bin"
    package_bin.mkdir(parents=True)
    (package_bin / "dsh-acp-demo").symlink_to(standin)
    (folder / "package.json").write_text(
        json.dumps(
            {
                "ccccManaged": True,
                "name": "cccc-deepseek-runtime",
                "private": True,
                "dependencies": packages,
            }
        )
    )
    lock = {"": {"dependencies": packages}}
    for name, ver in packages.items():
        key = "node_modules/" + name
        entry = folder / key
        entry.mkdir(parents=True)
        lock[key] = {"version": ver}
        (entry / "package.json").write_text(json.dumps(lock[key]))
    (folder / "package-lock.json").write_text(
        json.dumps({"lockfileVersion": 3, "packages": lock})
    )


def exercise(kind, root, out, old, new, before, after, archive_server):
    case = out / kind
    case.mkdir()
    root = root / kind
    root.mkdir()
    install, project, home = root / "bin", root / "work", root / "cccc"
    install.mkdir()
    project.mkdir()
    gate = root / "finish"
    gate.mkdir()
    logs = case / "provider"
    logs.mkdir()
    program = install / "cccc"
    shutil.copy2(old, program)
    if kind == "release":
        (install / ".cccc-standalone").write_text("standalone-v1\n")
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        port = str(sock.getsockname()[1])
    env = dict(
        os.environ,
        CCCC_HOME=str(home),
        CCCC_RELEASE_BASE_URL=archive_server,
        CCCC_RELEASE_INDEX_URL="https://127.0.0.1:1/releases",
        npm_config_registry="http://127.0.0.1:1/",
        npm_config_ignore_scripts="true",
        PATH=str(install) + os.pathsep + os.environ["PATH"],
    )
    assert Path(env["HOME"]).parent.name.startswith("cccc-isolated.")

    def cli(*args):
        return u.run_json([program, *args, "--port", port], env)

    def rows(actor):
        return [
            r
            for path in sorted(logs.glob(actor + ".*.jsonl"))
            for r in u.read_calls(path)
        ]

    def token_events(actor, token, event):
        return [r for r in rows(actor) if r["ev"] == event and r.get("token") == token]

    def send_round():
        data = {}
        for actor in ACTORS:
            token = "TOKEN-" + uuid.uuid4().hex[:16]
            v = cli("send", "--group", gid, "--to", actor, "maintenance " + token)
            event_id = v.get("id") or v["event"]["id"]
            assert event_id
            data[actor] = (token, event_id)
        return data

    def complete(data):
        for token, _ in data.values():
            (gate / token).touch()
        for actor, (token, _) in data.items():
            u.wait_for(
                lambda: token_events(actor, token, "end"), "prompt completion: " + actor
            )
            assert [r["reason"] for r in token_events(actor, token, "end")] == [
                "end_turn"
            ]
            assert [r["k"] for r in token_events(actor, token, "chunk")] == [1, 2, 3, 4]
            assert [r["copies"] for r in token_events(actor, token, "input")] == [1]

    log = (case / "host.log").open("wb")
    host = subprocess.Popen(
        [
            program,
            *(["daemon", "run"] if kind == "daemon" else ["web", "--port", port]),
        ],
        stdin=subprocess.DEVNULL,
        stdout=log,
        stderr=subprocess.STDOUT,
        env=env,
    )
    try:
        first = u.wait_for(lambda: u.daemon_ping(home), "old daemon")
        assert first["build"]["source_id"] == before["source_id"]
        seed_deepseek(home)
        gid = cli("attach", str(project))["group_id"]
        for actor in ACTORS:
            provider = root / (actor + "-home")
            provider.mkdir()
            name = "kimi" if actor == "structured" else "fake-dsh"
            fake = install / name
            fake.symlink_to(standin)
            flags = (
                [
                    "--runtime",
                    "kimi",
                    "--runtime-mode",
                    "acp",
                    "--env",
                    "KIMI_CODE_HOME=" + str(provider / "kimi"),
                ]
                if actor == "structured"
                else ["--runtime", "deepseek", "--env", "DEEPSEEK_API_KEY=fixture"]
            )
            cli(
                "actor",
                "add",
                actor,
                "--group",
                gid,
                "--command",
                str(fake),
                *flags,
                "--env",
                "HOME=" + str(provider),
                "--env",
                "STANDIN_LOG_DIR=" + str(logs),
                "--env",
                "STANDIN_RELEASE_DIR=" + str(gate),
                "--env",
                "STANDIN_CHUNKS=4",
                "--env",
                "STANDIN_INTERVAL=0.12",
            )
            cli("actor", "start", actor, "--group", gid)
        round_one = send_round()
        ids = {}
        for actor, (token, _) in round_one.items():
            u.wait_for(
                lambda: token_events(actor, token, "chunk"),
                "first pre-update chunk: " + actor,
            )
            starts = [r for r in rows(actor) if r["ev"] == "start"]
            assert len(starts) == 1, starts
            ids[actor] = u.process_identity(starts[0]["pid"])
        stopped_rows = None
        if kind == "daemon":
            receipt = cli("daemon", "stop", "--keep-agents")["receipt"]
            assert receipt["mode"] == "preserve_agents" and not receipt["unconfirmed"]
            stopped_rows = receipt["retained"]
            assert host.wait(timeout=40) == 0
            for identity in ids.values():
                assert u.process_identity(identity[0]) == identity
            candidate = install / "cccc.next"
            shutil.copy2(new, candidate)
            candidate.replace(program)
            subprocess.run(
                [program, "daemon", "start"],
                env=env,
                check=True,
                stdout=log,
                stderr=log,
                timeout=50,
            )
            target = u.wait_for(lambda: u.daemon_ping(home), "restarted daemon")
            assert first["pid"] != target["pid"]
            rows_after = target["retained_agents"]
        else:
            if kind == "local":
                candidate = install / "cccc.next"
                shutil.copy2(new, candidate)
                candidate.replace(program)
                checked = u.post(port, "/api/v1/update/check", {})["check"]["local"]
                assert checked["state"] == "rebuilt" and checked["keep"]["allowed"], (
                    checked
                )
                selection = {
                    "source": "local",
                    "source_id": after["source_id"],
                    "token": checked["token"],
                }
            else:
                selection = {"source": "release", "version": after["version"]}
            accepted = u.post(
                port, "/api/v1/update/start", {**selection, "mode": "keep"}
            )
            assert accepted["accepted"] and accepted["mode"] == "keep"

            def completed():
                record = json.loads((home / "update/last.json").read_text())
                return (
                    record
                    if record["transaction"] == accepted["transaction"]
                    and record["agents"]["state"] != "observing"
                    else None
                )

            last = u.wait_for(completed, "maintenance result", 120)
            assert (
                last["result"] == ("installed" if kind == "release" else "restarted")
                and last["agents"]["state"] == "observed"
            ), last
            assert not any(
                last["agents"][key] for key in ("unconfirmed", "not_back", "restarted")
            )
            rows_after = last["agents"]["reattached"]
            target = u.daemon_ping(home)
            assert first["pid"] == target["pid"]
        assert target["build"]["source_id"] == after["source_id"] and u.digest(
            program
        ) == u.digest(new)

        def owner_keys(records):
            return sorted(
                (
                    r["group_id"],
                    r["actor_id"],
                    r["actor_created_at"],
                    r["owner_id"],
                    r["generation"],
                )
                for r in records
            )

        after_keys = owner_keys(rows_after)
        assert [(r[0], r[1]) for r in after_keys] == sorted((gid, a) for a in ACTORS), (
            after_keys
        )
        assert all(all(row) for row in after_keys)
        if stopped_rows is not None:
            assert owner_keys(stopped_rows) == after_keys
        for actor, (token, _) in round_one.items():
            assert [r["k"] for r in token_events(actor, token, "chunk")] == [1]
            assert u.process_identity(ids[actor][0]) == ids[actor]
        complete(round_one)
        round_two = send_round()
        complete(round_two)
        for actor in ACTORS:
            assert len([r for r in rows(actor) if r["ev"] == "start"]) == 1
            assert u.process_identity(ids[actor][0]) == ids[actor]
        final = cli("daemon", "stop")["receipt"]
        assert (
            final["mode"] == "stop"
            and owner_keys(final["confirmed_stopped"]) == after_keys
        )
        if host.poll() is None:
            assert host.wait(timeout=40) == 0
        for identity in ids.values():
            u.wait_for(
                lambda: u.process_gone(identity[0]),
                "normal Stop retires original process",
            )
        result = dict(
            mode=kind,
            before=first["build"],
            after=target["build"],
            identities=ids,
            owners=after_keys,
            first_turn=round_one,
            followup=round_two,
            unique_turns=2,
            chunks_each_turn=4,
            clean_stop=True,
        )
        (case / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        print(kind, "passed:", *ids, flush=True)
    finally:
        try:
            if (home / "daemon/ccccd.sock").exists():
                cli("daemon", "stop")
        except Exception:
            pass
        if host.poll() is None:
            host.terminate()
            try:
                host.wait(timeout=8)
            except subprocess.TimeoutExpired:
                host.kill()
                host.wait()
        log.close()


def all_cases(_kind, *args):
    failed = []
    for kind in ("daemon", "local", "release"):
        try:
            exercise(kind, *args)
        except Exception:
            failed.append(kind)
            traceback.print_exc()
    if failed:
        raise RuntimeError("failed maintenance entries: " + ", ".join(failed))


if __name__ == "__main__":
    if len(sys.argv) != 4:
        raise SystemExit("usage: managed.py OLD NEW OUTPUT")
    u.run_case = all_cases
    sys.argv.append("local")
    u.main()
