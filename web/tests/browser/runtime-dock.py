#!/usr/bin/env python3
"""Browser regression for the real Runtime Dock with isolated synthetic SSE.

Requires Chrome, requests and websocket-client. Run an isolated Vite server first:
  CCCC_WEB_PORT=19999 npm -C web run dev -- --host 127.0.0.1 --port 15561
Then: python3 web/tests/browser/runtime-dock.py
Do not edit frontend files during this run (Vite hot reload replaces fixture state).
Optional env: CHROME_BIN, CCCC_RUNTIME_DOCK_BASE_URL, CCCC_RUNTIME_DOCK_OUTPUT_DIR.
The browser always uses a new temporary profile; it never accesses existing tabs.
"""

import base64, json, os, subprocess, tempfile, time
from pathlib import Path
import requests, websocket

OUT = Path(
    os.environ.get("CCCC_RUNTIME_DOCK_OUTPUT_DIR")
    or tempfile.mkdtemp(prefix="cccc-runtime-dock-evidence-")
)
OUT.mkdir(parents=True, exist_ok=True)
BASE_URL = os.environ.get(
    "CCCC_RUNTIME_DOCK_BASE_URL", "http://127.0.0.1:15561"
).rstrip("/")
with tempfile.TemporaryDirectory(
    prefix="cccc-runtime-dock-browser-", ignore_cleanup_errors=True
) as profile:
    browser = subprocess.Popen(
        [
            os.environ.get("CHROME_BIN", "/usr/bin/google-chrome"),
            "--headless=new",
            "--autoplay-policy=no-user-gesture-required",
            "--no-sandbox",
            "--remote-debugging-port=0",
            "--remote-allow-origins=*",
            "--user-data-dir=" + profile,
            "about:blank",
        ],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    sock = None
    try:
        portfile = Path(profile) / "DevToolsActivePort"
        for _ in range(100):
            if portfile.exists():
                break
            time.sleep(0.1)
        port = portfile.read_text().splitlines()[0]
        target = requests.put(
            f"http://127.0.0.1:{port}/json/new?about:blank", timeout=5
        ).json()
        sock = websocket.create_connection(target["webSocketDebuggerUrl"], timeout=40)
        seq = 0

        def cdp(method, params=None):
            global seq
            seq += 1
            sock.send(json.dumps({"id": seq, "method": method, "params": params or {}}))
            while True:
                msg = json.loads(sock.recv())
                if msg.get("method") == "Runtime.consoleAPICalled" and msg.get(
                    "params", {}
                ).get("type") in ["warning", "error"]:
                    print("CONSOLE", msg["params"], flush=True)
                if msg.get("id") != seq:
                    continue
                if "error" in msg:
                    raise RuntimeError(msg["error"])
                return msg["result"]

        def js(expr):
            r = cdp(
                "Runtime.evaluate",
                {"expression": expr, "awaitPromise": True, "returnByValue": True},
            )
            if "exceptionDetails" in r:
                raise RuntimeError(r["exceptionDetails"])
            return r.get("result", {}).get("value")

        def navigate(width=1440):
            cdp(
                "Emulation.setDeviceMetricsOverride",
                {
                    "width": width,
                    "height": 820,
                    "deviceScaleFactor": 1,
                    "mobile": False,
                },
            )
            cdp(
                "Page.navigate",
                {"url": BASE_URL + "/ui/tests/browser/runtime-dock.html"},
            )
            for _ in range(100):
                if js("typeof dockFixture !== 'undefined' && dockFixture.ready()"):
                    return
                time.sleep(0.1)
            raise RuntimeError("Fixture not ready")

        def texts():
            return js("dockFixture.texts()")

        def live(actor, turn, text):
            js(
                "dockFixture.live("
                + json.dumps(actor)
                + ","
                + json.dumps(turn)
                + ","
                + json.dumps(text)
                + ")"
            )
            time.sleep(0.12)

        navigate()
        js("dockFixture.restore()")
        time.sleep(0.15)
        assert texts() == [], texts()
        js("dockFixture.replay()")
        assert texts() == [], texts()
        live(
            "actor-0", "fresh", "Paragraph.\n" * 80 + "Latest result: 25 tests passed."
        )
        assert (
            len(texts()) == 1
            and "Latest result" in texts()[0]
            and "Paragraph" not in texts()[0]
        ), texts()
        live("actor-0", "fresh2", "Now checking the package.")
        assert len(texts()) == 1 and "Now checking" in texts()[0], texts()
        for i in range(1, 5):
            live("actor-" + str(i), "r" + str(i), "Runtime " + str(i) + " progress.")
            assert len(texts()) <= 2 and any(
                "Runtime " + str(i) in t for t in texts()
            ), texts()
        js("dockFixture.group('fixture-b')")
        time.sleep(0.15)
        assert texts() == [], texts()
        js("dockFixture.group('fixture-a')")
        time.sleep(0.15)
        assert len(texts()) == 2, texts()
        time.sleep(6.1)
        assert texts() == [], texts()
        navigate()
        js("dockFixture.restore(); dockFixture.replay()")
        assert texts() == [], texts()
        for width in [1440, 390, 320]:
            for dark in [False, True]:
                navigate(width)
                js("dockFixture.dark(" + json.dumps(dark) + ")")
                live(
                    "actor-0",
                    "long",
                    "正在验证本轮修改，检查每个协议运行时是否只展示最新进展。" * 6,
                )
                live(
                    "actor-1",
                    "second",
                    "Running focused checks; the complete output remains in the Actor panel.",
                )
                geometry = js("""(() => {
                  const input=document.querySelector('input'), dock=document.querySelector('.runtime-dock-ticker-entry');
                  const entries=[...document.querySelectorAll('.runtime-dock-ticker-entry')].map(e=>e.getBoundingClientRect().toJSON());
                  const clipped=[...document.querySelectorAll('.runtime-dock-ticker-entry')].some(node=>{
                    const rect=node.getBoundingClientRect();
                    for(let parent=node.parentElement;parent;parent=parent.parentElement){
                      const style=getComputedStyle(parent),box=parent.getBoundingClientRect();
                      if(['auto','scroll','hidden','clip'].includes(style.overflowY) && (rect.top<box.top-1 || rect.bottom>box.bottom+1))return true;
                      if(['auto','scroll','hidden','clip'].includes(style.overflowX) && (rect.left<box.left-1 || rect.right>box.right+1))return true;
                    }
                    return false;
                  });
                  return {clipped,width:innerWidth,scroll:document.documentElement.scrollWidth,input:input.getBoundingClientRect().toJSON(),entries,
                    buttons:[...document.querySelectorAll('button')].filter(b=>b.getBoundingClientRect().width>0).map(b=>b.getBoundingClientRect().toJSON())};
                })()""")
                assert not geometry["clipped"], geometry
                assert geometry["scroll"] <= width, geometry
                assert len(geometry["entries"]) == 2, geometry
                for e in geometry["entries"]:
                    assert (
                        e["left"] >= 0
                        and e["right"] <= width
                        and e["bottom"] < geometry["input"]["top"]
                    ), geometry
                time.sleep(0.6)
                shot = cdp("Page.captureScreenshot", {"format": "png"})
                (
                    OUT / (str(width) + ("-dark" if dark else "-light") + ".png")
                ).write_bytes(base64.b64decode(shot["data"]))
                js("document.querySelector('button').focus()")
                assert js("document.activeElement.tagName") == "BUTTON"
                cdp(
                    "Input.dispatchKeyEvent",
                    {
                        "type": "keyDown",
                        "key": "Tab",
                        "code": "Tab",
                        "windowsVirtualKeyCode": 9,
                    },
                )
                cdp(
                    "Input.dispatchKeyEvent",
                    {
                        "type": "keyUp",
                        "key": "Tab",
                        "code": "Tab",
                        "windowsVirtualKeyCode": 9,
                    },
                )
                assert js("document.activeElement.tagName") in ["BUTTON", "INPUT"]
                js("document.querySelector('input').focus()")
                assert js("document.activeElement.tagName") == "INPUT"
                print(
                    json.dumps(
                        {
                            "width": width,
                            "dark": dark,
                            "texts": texts(),
                            "geometry": geometry,
                        }
                    ),
                    flush=True,
                )
        print(
            "PASS: history, reconnect idempotence, five runtimes, latest excerpt, group isolation, expiry, responsive layout and focus",
            flush=True,
        )
    finally:
        if sock is not None:
            sock.close()
        browser.terminate()
        try:
            browser.wait(timeout=5)
        except subprocess.TimeoutExpired:
            browser.kill()
            browser.wait(timeout=5)
