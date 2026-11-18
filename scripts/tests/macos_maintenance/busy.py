"""An in-flight local Web Model turn across the real maintenance entry points."""

import hashlib
import http.server
import importlib.util
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import traceback
import time
import urllib.request

here = Path(__file__).resolve().parent
src = here / "update_keep.py"
spec = importlib.util.spec_from_file_location("update_fixture", src)
u = importlib.util.module_from_spec(spec)
spec.loader.exec_module(u)
context_spec = importlib.util.spec_from_file_location(
    "browser_control", here / "coverage/wave/browser_context.py"
)
control = importlib.util.module_from_spec(context_spec)
context_spec.loader.exec_module(control)

actor = "browser-test"
one, two = "W2-ROUND-1", "W2-ROUND-2"


def exercise(kind, root, out, old, new, before, after, archive_server):
    assert kind in ("daemon", "local", "release")
    case = out / kind
    case.mkdir()
    root = root / kind
    root.mkdir()
    home = root / "cccc"
    project = root / "work"
    project.mkdir()
    install = root / "bin"
    install.mkdir()
    program = install / "cccc"
    shutil.copy2(old, program)
    if kind == "release":
        (install / ".cccc-standalone").write_text("standalone-v1\n")
    env = dict(
        os.environ,
        CCCC_HOME=str(home),
        CCCC_RELEASE_BASE_URL=archive_server,
        CCCC_RELEASE_INDEX_URL="https://127.0.0.1:1/releases",
        PATH=str(install) + ":/usr/bin:/bin",
    )
    assert Path(env["HOME"]).parent.name.startswith("cccc-isolated.")
    if sys.platform == "darwin" and os.environ.get("CCCC_TEST_BROWSER_ENV_FILE"):
        # Only the CCCC child uses the OS context the positive Chrome control identified.
        overrides = json.loads(
            Path(os.environ["CCCC_TEST_BROWSER_ENV_FILE"]).read_text()
        )
        assert set(overrides) <= {
            "HOME",
            "TMPDIR",
            "XPC_FLAGS",
            "XPC_SERVICE_NAME",
            "__CF_USER_TEXT_ENCODING",
            "COMMAND_MODE",
        }
        env.update(overrides)

    import socket

    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]

    def cli(*args):
        return json.loads(
            subprocess.check_output(
                [program, *args, "--port", str(port)], env=env, timeout=60
            )
        )

    def expect(fn, name, seconds=65):
        try:
            return u.wait_for(fn, name, seconds=seconds)
        except TimeoutError:
            print("timeout:", name, "events:", events, flush=True)
            raise

    def trace(text):
        print(kind, text, flush=True)

    events = []
    http_trace = []
    finished = set()
    page = here.joinpath("chat.html").read_bytes()

    class Model(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def respond(self, body, mime):
            self.send_response(200)
            self.send_header("Content-Type", mime)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            if self.path != "/state":
                http_trace.append(["GET", self.path])
            if self.path == "/state":
                self.respond(
                    json.dumps({"finished": list(finished)}).encode(),
                    "application/json",
                )
            elif self.path == "/ci-navigation-check":
                self.respond(b"<title>private navigation check</title><script>fetch('/ci-navigation-loaded')</script>", "text/html")
            elif self.path == "/ci-navigation-loaded":
                self.respond(b"ok", "text/plain")
            elif self.path == "/":
                self.respond(page, "text/html; charset=utf-8")
            else:
                self.send_error(404)

        def do_POST(self):
            if self.path == "/fixture-script":
                payload = self.rfile.read(int(self.headers["Content-Length"]))
                http_trace.append(["script", json.loads(payload)])
                self.respond(b"{}", "application/json")
                return
            if self.path != "/event":
                self.send_error(404)
                return
            v = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            events.append(v)
            self.respond(b"{}", "application/json")

    model = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Model)
    thread = threading.Thread(target=model.serve_forever, daemon=True)
    thread.start()
    url = f"http://127.0.0.1:{model.server_port}/"

    def served_assets(dist):
        index = (dist / "index.html").read_bytes()
        entry = re.search(rb'<script type="module"[^>]* src="/ui/(assets/[^"]+\.js)"', index)
        assert entry, "built Web index has no module entry"
        script = entry[1].decode()
        hashes = {}
        for route, asset in [("", "index.html"), (script, script)]:
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/ui/{route}", timeout=3) as response:
                actual = hashlib.sha256(response.read()).hexdigest()
            assert actual == u.digest(dist / asset), f"served {asset} differs from its build"
            hashes[asset] = actual
        return hashes

    def browser():
        rows = u.daemon_request(home, "browser_resources", by="user")["resources"]
        for r in rows:
            for p in r.get("sessions", []):
                if (p.get("actor") or {}).get("actor_id") == actor:
                    pid = r["metadata"]["pid"]
                    profile = Path(r["profile"]).resolve(strict=True)
                    assert profile.is_relative_to(home.resolve()), profile
                    locked = control.profile_owner(profile)
                    assert locked and locked[0] == pid, (profile, pid)
                    return {
                        "profile": str(profile),
                        "instance": r["instance"],
                        "owner": r["owner_id"],
                        "target": p["target_id"],
                        "identity": u.process_identity(pid),
                        "pid": pid,
                    }

    def ev(name, token):
        return [v for v in events if v["event"] == name and v["token"] == token]

    logs = (case / "host.log").open("wb")
    host = None

    def start_host():
        return subprocess.Popen(
            [program, "web", "--port", str(port)],
            stdin=subprocess.DEVNULL,
            stdout=logs,
            stderr=subprocess.STDOUT,
            env=env,
        )

    try:
        first_daemon = subprocess.Popen(
            [program, "daemon", "run"],
            stdin=subprocess.DEVNULL,
            stdout=logs,
            stderr=subprocess.STDOUT,
            env=env,
        )
        try:
            expect(lambda: u.daemon_ping(home), "seed daemon")
            gid = cli("attach", str(project))["group_id"]
            row = cli(
                "actor",
                "add",
                actor,
                "--runtime",
                "web_model",
                "--env",
                "CCCC_WEB_MODEL_DELIVERY_MODE=browser",
                "--group",
                gid,
            )["actor"]
        finally:
            cli("daemon", "stop")
            assert first_daemon.wait(timeout=30) == 0
        # Seed a synthetic paired browser while the fixture home is offline, as
        # in the in-process recovery regression. No login or real provider is used.
        groupfile = home / "groups" / gid / "group.yaml"
        generation = row.get("generation") or "legacy:" + row["created_at"]
        group = json.loads(
            subprocess.check_output(
                [
                    "/usr/bin/ruby",
                    "-rjson",
                    "-ryaml",
                    "-e",
                    "print JSON.generate(YAML.safe_load(File.read(ARGV.fetch(0))))",
                    str(groupfile),
                ]
            )
        )
        assert group["group_id"] == gid
        group["running"] = True
        group["web_model_browser_targets"] = {
            actor: {
                "kind": "existing_chat",
                "url": url,
                "last_delivery_status": "bound",
            }
        }
        groupfile.write_text(json.dumps(group))
        key = json.dumps([gid, actor], separators=(",", ":"))
        store = {
            "version": 3,
            "connectors": {
                "chatgpt_web": {
                    "provider": "chatgpt_web",
                    "connector_id": "fixture-connector",
                    "revoked": False,
                    "bindings": {
                        key: {
                            "group_id": gid,
                            "actor_id": actor,
                            "generation": generation,
                            "revision": "fixture",
                            "state": "bound",
                            "url": url,
                        }
                    },
                }
            },
        }
        f = home / "web_model_connectors.yaml"
        f.write_text(json.dumps(store))
        f.chmod(0o600)
        host = start_host()
        source = expect(lambda: u.daemon_ping(home), "first Web")
        assert source["build"]["source_id"] == before["source_id"]
        source_dist = Path(os.environ.get("CCCC_TEST_PREVIOUS_DIST", Path.cwd() / "web/dist"))
        source_assets = expect(lambda: served_assets(source_dist), "previous program serves its built Web assets")
        original = expect(browser, "open first held window")
        if sys.platform == "linux":
            assert b"--headless=new" in Path(
                "/proc", str(original["pid"]), "cmdline"
            ).read_bytes().split(b"\0")
        elif sys.platform == "darwin":
            argv = subprocess.check_output(
                ["/bin/ps", "-ww", "-p", str(original["pid"]), "-o", "command="],
                text=True,
            )
            assert "--headless=new" in argv.split() and "--user-data-dir=" in argv
            assert ("--use-mock-keychain" in argv.split()) == (os.environ["CCCC_TEST_CHROME_MODE"] == "mock")

        else:
            raise RuntimeError(
                "the fixture needs a native process identity on this platform"
            )

        def send(text):
            return cli("send", "--group", gid, "--to", actor, text)

        # Each record here must be available even when the first submission never happens.
        (case / "before-send.json").write_text(json.dumps({
            "browser": original, "fixture_url": url, "web_assets": source_assets,
            "source": source["build"], "headless_argv_verified": True,
        }, indent=2) + "\n")
        receipt = send(one)
        try:
            expect(lambda: ev("submit", one), "first prompt on page")
        except TimeoutError:
            evidence = {"http_requests": list(http_trace), "page_events": list(events)}
            try:
                rows = u.daemon_request(home, "browser_resources", by="user")["resources"]
                resource = next(r for r in rows if r["instance"] == original["instance"])
                owned = control.profile_owner(Path(original["profile"]))
                assert owned and owned[0] == original["pid"] == resource["metadata"]["pid"]
                cdp_port = int(resource["metadata"]["cdp_port"])
                assert f"--remote-debugging-port={cdp_port}" in owned[1].split()
                spec = importlib.util.spec_from_file_location("private_browser_sample", here.parent / "macos_launch/browser_sample.py")
                sample = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(sample)
                assert sample.owns_listener(owned[0], cdp_port), "private Chrome no longer owns the CDP port"
                devtools = f"http://127.0.0.1:{cdp_port}/json/list"
                with urllib.request.urlopen(devtools, timeout=3) as response:
                    targets = json.load(response)
                evidence["target"] = next(
                    ({"url": t.get("url"), "type": t.get("type"), "title": t.get("title")}
                     for t in targets if t["id"] == original["target"]), None
                )
                if sys.platform == "darwin":
                    evidence["samples"] = sample.sample_processes(owned[0], Path(original["profile"]), case)
                if evidence["target"] and os.environ.get("CCCC_TEST_NODE"):
                    document = subprocess.check_output(
                        [os.environ["CCCC_TEST_NODE"], here.parent / "macos_launch/page-state.mjs",
                         str(cdp_port), original["target"], url + "ci-navigation-check"], text=True, timeout=12,
                    )
                    evidence["post_verdict_probe"] = json.loads(document)
                    deadline = time.monotonic() + 2
                    while ["GET", "/ci-navigation-loaded"] not in http_trace and time.monotonic() < deadline:
                        time.sleep(0.05)
                    evidence["post_verdict_http"] = list(http_trace[len(evidence["http_requests"]):])
            except Exception as error:
                evidence["target_read_error"] = str(error)
            try:
                source_event = receipt["event"]["id"]
                rows = u.read_calls(home / "groups" / gid / "ledger.jsonl")
                evidence["delivery"] = [r["data"] for r in rows if r["kind"] == "runtime.delivery"
                                        and r["data"].get("source_event_id") == source_event]
            except Exception as error:
                evidence["delivery_read_error"] = str(error)
            (case / "failed-first-prompt.json").write_text(json.dumps(evidence, indent=2) + "\n")
            raise
        (case / "first-submitted.json").write_text(json.dumps(ev("submit", one)) + "\n")
        trace("first turn started")
        if kind == "daemon":
            cli("daemon", "stop", "--keep-agents")
            assert host.wait(timeout=45) == 0
            assert u.process_identity(original["pid"]) == original["identity"]
            candidate = install / "cccc.next"
            shutil.copy2(new, candidate)
            candidate.replace(program)
            assert u.digest(program) == u.digest(new)
            host = start_host()
            target = expect(lambda: u.daemon_ping(home), "replacement Web")
            assert (
                source["pid"] != target["pid"]
                and target["build"]["source_id"]
                == after["source_id"]
                != source["build"]["source_id"]
            )
            receipt = None
        else:
            if kind == "local":
                next_image = install / "cccc.next"
                shutil.copy2(new, next_image)
                next_image.replace(program)
                check = u.post(port, "/api/v1/update/check", {})["check"]["local"]
                assert check["state"] == "rebuilt" and check["keep"]["allowed"], check
                selection = {
                    "source": kind,
                    "token": check["token"],
                    "source_id": after["source_id"],
                }
            else:
                selection = {"source": kind, "version": after["version"]}
            accepted = u.post(
                port, "/api/v1/update/start", {**selection, "mode": "keep"}
            )
            assert accepted["accepted"], accepted

            def completed():
                val = json.loads((home / "update/last.json").read_text())
                return (
                    val
                    if val.get("transaction") == accepted["transaction"]
                    and val.get("agents", {}).get("state") != "observing"
                    else None
                )

            receipt = expect(completed, "completed maintenance", 120)
            assert receipt["result"] == (
                "installed" if kind == "release" else "restarted"
            ), receipt
            target = expect(lambda: u.daemon_ping(home), "new build")
            assert (
                target["build"]["source_id"] == after["source_id"]
                and target["pid"] == source["pid"]
            )
        target_assets = expect(lambda: served_assets(Path.cwd() / "web/dist"), "replacement serves its built Web assets")
        current = expect(browser, "same browser after restart")
        assert current == original, (current, original)
        send(two)
        time.sleep(2)
        assert (
            len(ev("submit", one)) == 1
            and not ev("finish", one)
            and not ev("submit", two)
        )
        finished.add(one)
        expect(lambda: ev("finish", one), "original answer finished")
        expect(lambda: ev("submit", two), "next prompt delivered", 100)
        finished.add(two)
        expect(lambda: ev("finish", two), "second answer finished")
        filtered = [(v["event"], v["token"]) for v in events if v["event"] != "open"]
        assert filtered == [
            ("submit", one),
            ("finish", one),
            ("submit", two),
            ("finish", two),
        ], filtered
        assert len({v["doc"] for v in events if v["event"] != "open"}) == 1
        assert len([v for v in events if v["event"] == "open"]) == 1, events
        if receipt:
            actors = [
                (v["group_id"], v["actor_id"]) for v in receipt["agents"]["reattached"]
            ]
            assert actors == [(gid, actor)], receipt
        cli("daemon", "stop")
        assert host.wait(timeout=45) == 0
        expect(
            lambda: u.process_gone(original["pid"]),
            "normal stop closes original browser",
        )
        result = dict(
            kind=kind,
            source=source["build"],
            target=target["build"],
            browser=original,
            events=events,
            agents=receipt["agents"] if receipt else target.get("retained_agents"),
            clean_stop=True,
            launcher="fixture_direct_headless",
            web_assets_sha256={"previous": source_assets, "current": target_assets},
        )
        (case / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        trace("passed")
    finally:
        if host and host.poll() is None:
            try:
                cli("daemon", "stop")
            except Exception:
                pass
            try:
                host.wait(timeout=10)
            except subprocess.TimeoutExpired:
                host.kill()
                host.wait()
        model.shutdown()
        model.server_close()
        logs.close()
        (case / "http-trace.json").write_text(json.dumps(http_trace, indent=2) + "\n")
        (case / "events.json").write_text(json.dumps(events, indent=2) + "\n")
        diagnostics = [(home / "daemon/cccc-web.log", "web-diagnostics.log"),
                       (home / "daemon/ccccd.log", "daemon-diagnostics.log")]
        diagnostics.extend((log, "chrome-stderr.log") for log in home.glob("state/web_model_browser/_shared/*/fixture-chrome.log"))
        for source, filename in diagnostics:
            try:
                with source.open("rb") as reader:
                    reader.seek(max(0, os.fstat(reader.fileno()).st_size - 65536))
                    (case / filename).write_bytes(reader.read())
            except FileNotFoundError:
                pass
        for lock in home.rglob("SingletonLock"):
            control.stop_profile(control.profile_owner(lock.parent.resolve()))


def run_requested(_kind, *args):
    failed = []
    for kind in requested:
        try:
            exercise(kind, *args)
        except Exception:
            failed.append(kind)
            traceback.print_exc()
    if failed:
        raise RuntimeError("Busy browser checks failed: " + ", ".join(failed))


if __name__ == "__main__":
    if len(sys.argv) not in (4, 5):
        raise SystemExit(
            "usage: busy.py OLD_BINARY NEW_BINARY OUTPUT [daemon|local|release]"
        )
    # Build the mock release once, then use a separate home for each maintenance action.
    requested = (
        (sys.argv.pop(),) if len(sys.argv) == 5 else ("daemon", "local", "release")
    )
    if any(kind not in ("daemon", "local", "release") for kind in requested):
        raise SystemExit("unknown browser maintenance mode")
    sys.argv.append("local")
    u.run_case = run_requested
    u.main()
