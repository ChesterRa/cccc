"""Check document navigation in a disposable system Chrome before building either CCCC binary."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time

here = Path(__file__).resolve().parent


def load_browser_inspector():
    helper = here / "browser_sample.py"
    import importlib.util

    spec = importlib.util.spec_from_file_location("private_browser_sample", helper)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run(mode, out, node, chrome):
    root = Path(tempfile.mkdtemp(prefix="cccc-isolated.", dir="/tmp")).resolve()
    report = {"mode": mode, "http": []}
    case = out / mode
    case.mkdir()
    loaded = set()

    class Http(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            match = re.fullmatch(r"/(document|loaded|observed)/([a-z-]+)", self.path)
            if not match:
                self.send_error(404)
                return
            route, name = match.groups()
            if route != "observed":
                report["http"].append([route, name])
            mime = "application/json"
            if route == "document":
                mime = "text/html"
                body = f"<title>{name}</title><script>fetch('/loaded/{name}')</script>".encode()
            else:
                if route == "loaded":
                    loaded.add(name)
                body = json.dumps({"script": name in loaded}).encode()
            self.send_response(200)
            self.send_header("Content-Type", mime)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    http = ThreadingHTTPServer(("127.0.0.1", 0), Http)
    http_thread = threading.Thread(target=http.serve_forever, daemon=True)
    http_thread.start()
    inspector = load_browser_inspector()
    proc = None
    try:
        for name in ("home", "tmp"):
            (root / name).mkdir()
        trial = Path(tempfile.mkdtemp(prefix="hold", dir=root / "tmp"))
        profile = (
            trial
            / "daemon/cccc/state/web_model_browser/_shared/chatgpt_web/chrome_profile"
        )
        profile.mkdir(parents=True)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        args = [
            str(chrome),
            f"--remote-debugging-port={port}",
            f"--user-data-dir={profile}",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-extensions",
            "--window-size=1366,900",
            "--window-position=-32000,-32000",
            "--force-device-scale-factor=1",
            "--headless=new",
            "--disable-background-networking",
        ]
        if mode == "mock":
            args.append("--use-mock-keychain")
        env = {
            "HOME": str(root / "home"),
            "TMPDIR": str(root / "tmp"),
            "PATH": "/usr/bin:/bin",
            "LANG": "C",
        }
        if mode == "runner-home":
            import pwd

            if (
                sys.platform != "darwin"
                or os.environ.get("GITHUB_ACTIONS") != "true"
                or os.environ["HOME"] != pwd.getpwuid(os.getuid()).pw_dir
            ):
                raise RuntimeError(
                    "runner-home control requires the ephemeral macOS runner's own home"
                )
            env["HOME"] = os.environ["HOME"]
        with (case / "chrome.log").open("wb") as log:
            proc = subprocess.Popen(
                args,
                env=env,
                stdin=subprocess.DEVNULL,
                stdout=log,
                stderr=log,
                start_new_session=True,
            )
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if proc.poll() is not None:
                raise RuntimeError(f"Chrome exited {proc.returncode}")
            try:
                owner = int(os.readlink(profile / "SingletonLock").rsplit("-", 1)[-1])
                if owner != proc.pid:
                    raise RuntimeError("private profile PID mismatch")
                if not inspector.owns_listener(proc.pid, port):
                    time.sleep(0.1)
                    continue
                report["listener_pid"] = proc.pid
                break
            except (FileNotFoundError, OSError):
                time.sleep(0.1)
        else:
            raise TimeoutError("no private browser endpoint")
        url = f"http://127.0.0.1:{http.server_port}"
        with (case / "driver.log").open("wb") as log:
            completed = subprocess.run(
                [
                    node,
                    str(here / "navigation.mjs"),
                    str(port),
                    str(proc.pid),
                    url,
                    str(case / "protocol.json"),
                ],
                env=env,
                stdout=log,
                stderr=log,
                timeout=90,
            )
        report["driver_exit"] = completed.returncode
        protocol = json.loads((case / "protocol.json").read_text())
        js = protocol["cases"]["js-background"]
        report["ready"] = (
            completed.returncode == 0
            and js.get("ready") is True
            and js["finalUrl"] == url + "/document/js-background"
            and js["viewport"]["width"] > 0
            and ["document", "js-background"] in report["http"]
            and "js-background" in loaded
        )
    except Exception as error:
        report["error"] = str(error)
    finally:
        if proc and proc.poll() is None:
            if mode == "standard" and not report.get("ready"):
                try:
                    report["samples"] = inspector.sample_processes(
                        proc.pid, profile, case
                    )
                except Exception as error:
                    report["sample_error"] = str(error)
            proc.terminate()
            try:
                proc.wait(timeout=4)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=3)
        http.shutdown()
        http.server_close()
        http_thread.join(timeout=2)
        shutil.rmtree(root, ignore_errors=True)
        (case / "result.json").write_text(json.dumps(report, indent=2) + "\n")
    return report.get("ready", False)


if __name__ == "__main__":
    out = Path(sys.argv[1]).resolve()
    out.mkdir(parents=True)
    if sys.platform == "darwin":
        chrome = Path("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
    elif sys.platform == "linux":
        chrome = Path("/opt/google/chrome/chrome")
    else:
        raise SystemExit("requires macOS or Linux")
    node = shutil.which("node")
    if not node or not chrome.is_file():
        raise SystemExit("requires Node 22+ and system Chrome")
    tested = {mode: run(mode, out, node, chrome) for mode in ("standard", "mock")}
    # Use Chromium's unattended macOS test setting when it has demonstrated real document navigation.
    # The reports retain the independent result of the ordinary configuration.
    if sys.platform == "darwin" and not tested["standard"]:
        tested["runner-home"] = run("runner-home", out, node, chrome)
    selected = next(
        (mode for mode in ("mock", "standard", "runner-home") if tested.get(mode)), None
    )
    (out / "selection.json").write_text(
        json.dumps({"ready": tested, "selected": selected}, indent=2) + "\n"
    )
    print(
        "Background JavaScript navigation and first-party HTTP:",
        tested,
        "selected:",
        selected,
        flush=True,
    )
    if selected is None:
        raise SystemExit(
            "No system Chrome configuration reached the fixture through CCCC's background navigation sequence."
        )
