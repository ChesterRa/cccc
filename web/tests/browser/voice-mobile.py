#!/usr/bin/env python3
"""Browser regression using production voice controls and xterm fixtures.

Requires Chrome, requests and websocket-client. Start an isolated Vite server:
  CCCC_WEB_PORT=19999 npm -C web run dev -- --host 127.0.0.1 --port 15559
Then: python3 web/tests/browser/voice-mobile.py
Optional: CHROME_BIN, CCCC_VOICE_MOBILE_BASE_URL, CCCC_VOICE_MOBILE_OUTPUT_DIR.
All HTTP is synthetic. No microphone, provider, daemon, or existing browser tabs.
"""

import base64, json, os, subprocess, tempfile, time
from pathlib import Path
import requests, websocket

OUT = Path(
    os.environ.get("CCCC_VOICE_MOBILE_OUTPUT_DIR")
    or tempfile.mkdtemp(prefix="cccc-voice-mobile-evidence-")
)
OUT.mkdir(parents=True, exist_ok=True)
BASE_URL = os.environ.get(
    "CCCC_VOICE_MOBILE_BASE_URL", "http://127.0.0.1:15559"
).rstrip("/")
with tempfile.TemporaryDirectory(
    prefix="cccc-voice-mobile-browser-", ignore_cleanup_errors=True
) as profile:
    browser = subprocess.Popen(
        [
            os.environ.get("CHROME_BIN", "/usr/bin/google-chrome"),
            "--headless=new",
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
        sock = websocket.create_connection(target["webSocketDebuggerUrl"], timeout=20)
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

        def click(selector):
            js(f"document.querySelector({json.dumps(selector)}).click()")
            time.sleep(0.15)

        def shot(name):
            time.sleep(0.2)
            r = cdp("Page.captureScreenshot", {"format": "png"})
            (OUT / (name + ".png")).write_bytes(base64.b64decode(r["data"]))

        def key(name, shift=False):
            cdp(
                "Input.dispatchKeyEvent",
                {
                    "type": "keyDown",
                    "key": name,
                    "code": name,
                    "windowsVirtualKeyCode": 9 if name == "Tab" else 27,
                    "modifiers": 8 if shift else 0,
                },
            )
            cdp(
                "Input.dispatchKeyEvent",
                {
                    "type": "keyUp",
                    "key": name,
                    "code": name,
                    "windowsVirtualKeyCode": 9 if name == "Tab" else 27,
                    "modifiers": 8 if shift else 0,
                },
            )
            time.sleep(0.03)

        def wait(expr):
            for _ in range(150):
                if js(expr):
                    return
                time.sleep(0.1)
            raise AssertionError(expr)

        def size(w, h=844):
            cdp(
                "Emulation.setDeviceMetricsOverride",
                {"width": w, "height": h, "deviceScaleFactor": 1, "mobile": True},
            )
            time.sleep(0.3)

        size(390)
        cdp("Page.navigate", {"url": BASE_URL + "/ui/tests/browser/voice-mobile.html"})
        wait('!!document.querySelector(".voice-mobile-only button")')
        click(".voice-mobile-only button")
        wait('!!document.querySelector(".voice-mobile-menu")')
        size(844, 390)
        assert not js('!!document.querySelector(".voice-mobile-menu")'), (
            "menu persists after rotation"
        )
        shot("menu-after-rotation")
        size(390)
        if not js('!!document.querySelector(".voice-mobile-menu")'):
            click(".voice-mobile-only button")
        js('voiceMobileProbe.setGroup("g2")')
        time.sleep(0.3)
        assert not js('!!document.querySelector(".voice-mobile-menu")'), (
            "menu persists after group switch"
        )
        click(".voice-mobile-only button")
        js("voiceMobileProbe.setDisabled(true)")
        time.sleep(0.3)
        assert not js('!!document.querySelector(".voice-mobile-menu")')
        js("voiceMobileProbe.setDisabled(false)")
        time.sleep(0.3)
        assert not js('!!document.querySelector(".voice-mobile-menu")')
        click(".voice-mobile-only button")
        click(".voice-mobile-menu button:has(svg.lucide-maximize)")
        wait('!!document.querySelector("[aria-modal=true]")')
        time.sleep(0.5)
        assert js(
            'document.querySelector("[aria-modal=true]").contains(document.activeElement)'
        ), "workspace focus outside modal"
        key("Escape")
        time.sleep(0.3)
        assert js(
            'document.activeElement === document.querySelector(".voice-mobile-only button")'
        ), "workspace lost return focus"
        print("PASS menu rotation, group scope, disabled, workspace focus", flush=True)
        click(".voice-mobile-only button")
        assert (
            js('document.querySelectorAll(".voice-mobile-menu fieldset").length') == 2
        )
        key("Escape")
        size(844, 390)
        click('.voice-desktop-controls button[aria-haspopup="dialog"]')
        size(390)
        assert not js('!!document.querySelector("[role=dialog]")'), (
            "desktop menu persists after rotation"
        )
        for index in [0, 1]:
            size(844, 390)
            js(
                f'document.querySelectorAll(".voice-desktop-controls button[aria-haspopup=dialog]")[{index}].click()'
            )
            time.sleep(0.2)
            assert js('!!document.querySelector("[role=dialog]")')
            size(390)
            assert not js('!!document.querySelector("[role=dialog]")')
        for lang in ["en", "zh", "ja"]:
            js(f"voiceMobileProbe.language({json.dumps(lang)})")
            for width, height in [(390, 844), (844, 390), (1280, 900)]:
                size(width, height)
                assert js("document.documentElement.scrollWidth") <= width
                if width < 640:
                    click(".voice-mobile-only button")
                    js(
                        'Promise.all(document.querySelector(".voice-mobile-menu").getAnimations({subtree:true}).map(a=>a.finished))'
                    )
                    bounds = js(
                        'document.querySelector(".voice-mobile-menu").getBoundingClientRect().toJSON()'
                    )
                    assert (
                        bounds["left"] >= 0
                        and bounds["right"] <= width
                        and bounds["top"] >= 0
                        and bounds["bottom"] <= height
                    )
                    assert js(
                        '[...document.querySelectorAll(".voice-mobile-menu button")].every(b=>b.getBoundingClientRect().height>=44)'
                    )
                    shot(f"menu-{lang}-{width}")
                    key("Escape")
                    assert js(
                        'document.activeElement===document.querySelector(".voice-mobile-only button")'
                    )
                else:
                    assert (
                        js(
                            'getComputedStyle(document.querySelector(".voice-mobile-only")).display'
                        )
                        == "none"
                    )
        print(
            "PASS desktop/mobile menu lifecycle, locales, bounds, keyboard focus",
            flush=True,
        )

        size(390)
        cdp("Emulation.setTouchEmulationEnabled", {"enabled": True})
        cdp(
            "Page.navigate",
            {"url": BASE_URL + "/ui/tests/browser/terminal-touch-scroll.html"},
        )
        wait('!!document.querySelector("#terminal[data-ready=true]")')
        for mode, alternate in [
            ("none", False),
            ("x10", False),
            ("vt200", False),
            ("drag", False),
            ("any", False),
            ("none", True),
            ("x10", True),
        ]:
            js(
                f"touchScrollFixture.configure({json.dumps(mode)},{json.dumps(alternate)})"
            )
            before = js("touchScrollFixture.snapshot()")
            assert before["mode"] == mode
            start = before["point"]
            end = {"x": start["x"], "y": start["y"] + before["cellHeight"] * 3}
            cdp(
                "Input.dispatchTouchEvent",
                {"type": "touchStart", "touchPoints": [start]},
            )
            cdp("Input.dispatchTouchEvent", {"type": "touchMove", "touchPoints": [end]})
            cdp("Input.dispatchTouchEvent", {"type": "touchEnd", "touchPoints": []})
            time.sleep(0.15)
            after = js("touchScrollFixture.snapshot()")
            if not alternate and mode in ["none", "x10"]:
                assert after["viewport"] == before["viewport"] - 3
                assert after["input"] == [] and after["wheels"] == []
            else:
                assert after["viewport"] == before["viewport"]
                assert after["wheels"] == [-1, -1, -1]
                if alternate:
                    assert "".join(after["input"]) == "\x1bOA" * 3
                else:
                    import re

                    assert len(after["input"]) == 3
                    assert all(
                        re.fullmatch(r"\x1b\[<64;\d+;\d+M", v) for v in after["input"]
                    )
            print("PASS touch", mode, "alternate=" + str(alternate), flush=True)
        for mode in ["vt200", "drag", "any"]:
            for can_control in [False, True]:
                js(f"touchScrollFixture.configure({json.dumps(mode)},false)")
                js(f"touchScrollFixture.setAccess({json.dumps(can_control)},false)")
                before = js("touchScrollFixture.snapshot()")
                start = before["point"]
                cdp(
                    "Input.dispatchTouchEvent",
                    {"type": "touchStart", "touchPoints": [start]},
                )
                # Same gesture: viewer -> writer -> viewer. No xterm remount.
                for step, (controls, writer) in enumerate(
                    [(can_control, False), (True, True), (True, False)], 1
                ):
                    js(
                        f"touchScrollFixture.setAccess({json.dumps(controls)},{json.dumps(writer)})"
                    )
                    previous = js("touchScrollFixture.snapshot()")
                    point = {
                        "x": start["x"],
                        "y": start["y"] + before["cellHeight"] * step * 2 + 0.2,
                    }
                    cdp(
                        "Input.dispatchTouchEvent",
                        {"type": "touchMove", "touchPoints": [point]},
                    )
                    time.sleep(0.1)
                    after = js("touchScrollFixture.snapshot()")
                    # xterm returns to live output when application input resumes.
                    assert after["viewport"] == (
                        after["base"] if writer else previous["viewport"] - 2
                    )
                    assert len(after["wheels"]) == (0 if step == 1 else 2)
                    assert len(after["input"]) == (0 if step == 1 else 2)
                cdp("Input.dispatchTouchEvent", {"type": "touchEnd", "touchPoints": []})
            print("PASS read-only + writer handoffs", mode, flush=True)
        print("EVIDENCE", str(OUT), flush=True)

    finally:
        if sock:
            sock.close()
        browser.terminate()
        try:
            browser.wait(timeout=5)
        except subprocess.TimeoutExpired:
            browser.kill()
            browser.wait(timeout=5)
