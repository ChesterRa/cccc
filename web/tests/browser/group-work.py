#!/usr/bin/env python3
"""Browser regression using real AppShell, xterm and synthetic fixture transports.

Requires Chrome, requests and websocket-client. Run an isolated Vite server first:
  CCCC_WEB_PORT=19999 npm -C web run dev -- --host 127.0.0.1 --port 15559
Then: python3 web/tests/browser/group-work.py
Do not edit frontend files during this run (Vite hot reload replaces fixture state).
Optional env: CHROME_BIN, CCCC_GROUP_WORK_BASE_URL, CCCC_GROUP_WORK_OUTPUT_DIR.
The browser always uses a new temporary profile; it never accesses existing tabs.
"""

import base64, json, os, subprocess, tempfile, time
from pathlib import Path
import requests, websocket

OUT = Path(
    os.environ.get("CCCC_GROUP_WORK_OUTPUT_DIR")
    or tempfile.mkdtemp(prefix="cccc-group-work-evidence-")
)
OUT.mkdir(parents=True, exist_ok=True)
BASE_URL = os.environ.get("CCCC_GROUP_WORK_BASE_URL", "http://127.0.0.1:15559").rstrip(
    "/"
)
with tempfile.TemporaryDirectory(
    prefix="cccc-group-work-browser-", ignore_cleanup_errors=True
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
            shot("failure")
            print(
                "RESOURCES",
                js(
                    'performance.getEntriesByType("resource").map(e=>e.name).filter(n=>n.includes("fixture")||n.includes("main"))'
                ),
                flush=True,
            )
            print(
                "FAILURE",
                js(
                    "({body:document.body.innerText.slice(0,2500),errors:groupWorkProbe.errors,state:groupWorkProbe.ui.getState().chatSessions,sockets:groupWorkProbe.sockets.map(s=>({actor:s.actor,ready:s.readyState,frames:s.frames})),requests:groupWorkProbe.requests})"
                ),
                flush=True,
            )
            raise AssertionError(expr)

        def dimensions(width, height=900):
            cdp(
                "Emulation.setDeviceMetricsOverride",
                {
                    "width": width,
                    "height": height,
                    "deviceScaleFactor": 1,
                    "mobile": width < 600,
                },
            )
            time.sleep(0.15)

        def rect(selector):
            return js(
                f"document.querySelector({json.dumps(selector)}).getBoundingClientRect().toJSON()"
            )

        def drag_to(x):
            r = js(
                'document.querySelector("[data-group-work-area]").parentElement.nextElementSibling.getBoundingClientRect().toJSON()'
            )
            start = r["x"] + r["width"] / 2
            y = r["y"] + r["height"] / 2
            cdp("Input.dispatchMouseEvent", {"type": "mouseMoved", "x": start, "y": y})
            cdp(
                "Input.dispatchMouseEvent",
                {
                    "type": "mousePressed",
                    "x": start,
                    "y": y,
                    "button": "left",
                    "buttons": 1,
                    "clickCount": 1,
                },
            )
            for i in range(1, 11):
                cdp(
                    "Input.dispatchMouseEvent",
                    {
                        "type": "mouseMoved",
                        "x": start + (x - start) * i / 10,
                        "y": y,
                        "buttons": 1,
                        "button": "left",
                    },
                )
            cdp(
                "Input.dispatchMouseEvent",
                {
                    "type": "mouseReleased",
                    "x": x,
                    "y": y,
                    "button": "left",
                    "clickCount": 1,
                },
            )
            time.sleep(0.15)

        cdp("Runtime.enable")
        cdp("Emulation.setFocusEmulationEnabled", {"enabled": True})
        dimensions(1440)
        cdp("Page.navigate", {"url": BASE_URL + "/ui/tests/browser/group-work.html"})
        wait(
            '!!window.groupWorkProbe && !!document.querySelector("[data-group-work-area]")'
        )
        time.sleep(0.5)

        def live():
            return js(
                "groupWorkProbe.sockets.filter(s=>s.readyState===1).map(s=>s.actor)"
            )

        def tiled():
            js(
                "document.querySelector('[data-group-view-switch] button:last-child').click()"
            )

        def messages():
            js(
                "document.querySelector('[data-group-view-switch] button:first-child').click()"
            )

        def point_click(selector):
            r = rect(selector)
            x = r["x"] + min(20, r["width"] / 2)
            y = r["y"] + min(20, r["height"] / 2)
            cdp(
                "Input.dispatchMouseEvent",
                {
                    "type": "mousePressed",
                    "x": x,
                    "y": y,
                    "button": "left",
                    "clickCount": 1,
                },
            )
            cdp(
                "Input.dispatchMouseEvent",
                {
                    "type": "mouseReleased",
                    "x": x,
                    "y": y,
                    "button": "left",
                    "clickCount": 1,
                },
            )
            time.sleep(0.1)

        def typing(text):
            cdp("Input.insertText", {"text": text})
            time.sleep(0.1)

        def visible_panes():
            return js(
                'Array.from(document.querySelectorAll("[data-runtime-actor-id]")).filter(e=>e.getBoundingClientRect().width>0).map(e=>e.dataset.runtimeActorId)'
            )

        composer_selector = "textarea:not(.xterm-helper-textarea)"
        composer = rect(composer_selector)
        js("document.querySelector(" + json.dumps(composer_selector) + ").focus()")
        typing("Keep this Group draft")
        logselector = "[data-group-message-view] [role=log]"
        print(
            "SCROLL_ELEMENTS",
            js(
                'Array.from(document.querySelectorAll("[data-group-message-view] *")).filter(e=>e.scrollHeight>e.clientHeight+200&&e.clientHeight>200).map(e=>({tag:e.tagName,role:e.getAttribute("role"),cls:e.className}))'
            ),
            flush=True,
        )
        if js("!!document.querySelector(" + json.dumps(logselector) + ")"):
            js("document.querySelector(" + json.dumps(logselector) + ").scrollTop=800")
            time.sleep(0.3)
            beforeScroll = js(
                "document.querySelector(" + json.dumps(logselector) + ").scrollTop"
            )
        else:
            beforeScroll = None
        tiled()
        wait("groupWorkProbe.sockets.filter(s=>s.readyState===1).length===4")
        time.sleep(0.5)
        assert live() == ["actor-1", "actor-2", "actor-3", "actor-4"], live()
        assert rect(composer_selector) == composer, (rect(composer_selector), composer)
        assert (
            js("document.querySelector(" + json.dumps(composer_selector) + ").value")
            == "Keep this Group draft"
        )
        assert js(
            'Array.from(document.querySelectorAll("[data-group-presentation-trigger]")).some(e=>{let r=e.getBoundingClientRect();return document.elementFromPoint(r.x+r.width/2,r.y+r.height/2)===e||e.contains(document.elementFromPoint(r.x+r.width/2,r.y+r.height/2))})'
        )
        for id, text in [("actor-1", "first-window"), ("actor-3", "third-window")]:
            point_click('[data-runtime-actor-id="' + id + '"] .xterm-screen')
            typing(text)
            assert js(
                "groupWorkProbe.sockets.filter(s=>s.readyState===1&&s.actor==="
                + json.dumps(id)
                + ").some(s=>s.frames.some(f=>f.type===48&&f.text.includes("
                + json.dumps(text)
                + ")))"
            )
            assert not js(
                "groupWorkProbe.sockets.filter(s=>s.readyState===1&&s.actor!=="
                + json.dumps(id)
                + ").some(s=>s.frames.some(f=>f.type===48&&f.text.includes("
                + json.dumps(text)
                + ")))"
            )
        js("window.focusedTerminal=document.activeElement")
        time.sleep(1.7)
        assert js("document.activeElement===window.focusedTerminal")
        js(
            'window.savedTerminal=document.querySelector("[data-runtime-actor-id=actor-1] .xterm-screen")'
        )
        opens = js("groupWorkProbe.sockets.length")
        click('[aria-label="Maximize Foreman"]')
        wait('!!document.querySelector("[aria-modal=true]")')
        time.sleep(0.4)
        assert js(
            'document.querySelector("[data-runtime-actor-id=actor-1] .xterm-screen")===window.savedTerminal'
        )
        assert js("groupWorkProbe.sockets.length") == opens
        point_click('[data-runtime-actor-id="actor-1"] .xterm-screen')
        key("Escape")
        key("Tab")
        assert js('!!document.querySelector("[aria-modal=true]")')
        assert js(
            'groupWorkProbe.sockets.find(s=>s.actor==="actor-1").frames.some(f=>f.type===48&&f.text.includes(String.fromCharCode(27)))'
        )
        assert js(
            'document.querySelector("[data-runtime-actor-id=actor-1] .xterm-screen").getBoundingClientRect().height>600'
        )
        shot("maximized-desktop")
        click('[aria-label="Close expanded Actor view"]')
        time.sleep(0.3)
        assert js(
            'document.querySelector("[data-runtime-actor-id=actor-1] .xterm-screen")===window.savedTerminal'
        )
        assert js("groupWorkProbe.sockets.length") == opens
        assert visible_panes() == ["actor-1", "actor-2", "actor-3", "actor-4"], (
            visible_panes()
        )
        assert (
            js(
                'document.activeElement.closest("[data-runtime-actor-id]")?.dataset.runtimeActorId'
            )
            == "actor-1"
        )
        # Tile controls route to the intended Actor without reconnecting its terminal.
        click('[data-runtime-actor-id=actor-1] [aria-label="Send interrupt signal"]')
        assert js('groupWorkProbe.sockets.filter(s=>s.readyState===1&&s.actor==="actor-1").some(s=>s.frames.some(f=>f.type===48&&f.text.includes(String.fromCharCode(3))))')
        click('[data-runtime-actor-id=actor-1] [aria-label="Controls for Foreman"]')
        wait('!!document.querySelector("[data-radix-popper-content-wrapper]")')
        shot("actor-controls")
        key("Escape")
        wait('!document.querySelector("[data-radix-popper-content-wrapper]")')
        assert js('document.activeElement.getAttribute("aria-label")') == "Controls for Foreman"
        click('[data-runtime-actor-id=actor-1] [aria-label="Controls for Foreman"]')
        js('Array.from(document.querySelectorAll("[data-radix-popper-content-wrapper] button")).find(e=>e.textContent.includes("Terminal history")).click()')
        wait('!!document.querySelector("[aria-modal=true]")')
        assert js('document.activeElement.closest("[aria-modal=true]")!==null')
        key("Escape")
        wait('!document.querySelector("[aria-modal=true]")')
        assert js("groupWorkProbe.sockets.length") == opens
        assert js('document.activeElement.getAttribute("aria-label")') == "Controls for Foreman"
        messages()
        wait("groupWorkProbe.sockets.every(s=>s.readyState===3)")
        time.sleep(0.3)
        if beforeScroll is not None:
            afterScroll = js(
                "document.querySelector(" + json.dumps(logselector) + ").scrollTop"
            )
            assert abs(afterScroll - beforeScroll) < 2, (beforeScroll, afterScroll)
        assert (
            js("document.querySelector(" + json.dumps(composer_selector) + ").value")
            == "Keep this Group draft"
        )
        tiled()
        wait("groupWorkProbe.sockets.filter(s=>s.readyState===1).length===4")
        click('[aria-label="Next page"]')
        wait(
            'groupWorkProbe.sockets.filter(s=>s.readyState===1).map(s=>s.actor).join(",")==="actor-5,actor-6,actor-7,actor-8"'
        )
        js('groupWorkProbe.chooseGroup("g2")')
        time.sleep(0.3)
        assert live() == [], live()
        assert (
            js(
                'document.querySelector("[data-group-terminal-view]").dataset.groupTerminalView'
            )
            == "hidden"
        )
        js('groupWorkProbe.chooseGroup("g1")')
        wait(
            'groupWorkProbe.sockets.filter(s=>s.readyState===1).map(s=>s.actor).join(",")==="actor-5,actor-6,actor-7,actor-8"'
        )
        js("window.groupWorkReloadPending=true")
        cdp("Page.reload")
        wait(
            '!window.groupWorkReloadPending && !!window.groupWorkProbe && groupWorkProbe.sockets.filter(s=>s.readyState===1).map(s=>s.actor).join(",")==="actor-5,actor-6,actor-7,actor-8"'
        )
        # Width changes keep the focused Actor, then the visible page anchor.
        point_click('[data-runtime-actor-id="actor-7"] .xterm-screen')
        dimensions(700)
        wait('groupWorkProbe.sockets.filter(s=>s.readyState===1).map(s=>s.actor).join(",")==="actor-7"')
        assert js('groupWorkProbe.ui.getState().chatSessions.g1.terminalPage') == 6
        dimensions(1440)
        wait('groupWorkProbe.sockets.filter(s=>s.readyState===1).length===4')
        assert visible_panes() == ["actor-5", "actor-6", "actor-7", "actor-8"]
        for count in [2, 3, 4, 8]:
            js("groupWorkProbe.setCount(" + str(count) + ")")
            time.sleep(0.35)
            assert len(live()) == min(count, 4), (count, live())
            shot("actors-" + str(count))
        for locale in ["en", "zh", "ja"]:
            js("groupWorkProbe.language(" + json.dumps(locale) + ")")
            time.sleep(0.3)
            for w, h in [(1440, 900), (1024, 900), (390, 844), (320, 568)]:
                dimensions(w, h)
                time.sleep(0.35)
                assert len(live()) == (4 if w >= 1024 else 1), (locale, w, live())
                assert js('Array.from(document.querySelectorAll("header button")).filter(e=>e.getBoundingClientRect().width>0).every(e=>{let r=e.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth+1})'), (locale, w, "header overflow")
                assert not js("document.documentElement.scrollWidth>innerWidth"), (
                    locale,
                    w,
                )
                assert js(
                    'Array.from(document.querySelectorAll("[data-runtime-actor-id]")).filter(e=>e.getBoundingClientRect().width>0).every(e=>{let r=e.getBoundingClientRect();let a=document.querySelector("[data-group-work-area]").getBoundingClientRect();return r.top>=a.top&&r.bottom<=a.bottom+1&&r.width>250&&r.height>150})'
                ), (locale, w)
                assert js(
                    'Array.from(document.querySelectorAll("[data-group-work-area] button, [data-group-work-area] select")).filter(e=>e.getBoundingClientRect().width>0).every(e=>{let r=e.getBoundingClientRect();return r.left>=0&&r.right<=innerWidth+1})'
                ), (locale, w)
                if w < 480:
                    assert js('Array.from(document.querySelectorAll("[data-actor-quick-controls]")).filter(e=>e.getBoundingClientRect().width>0).every(e=>Array.from(e.children).filter(b=>b.tagName==="BUTTON"&&b.getBoundingClientRect().width>0).length===2)'), (locale, w, "compact controls")
                assert js(
                    'document.querySelector("[data-group-work-area]").getBoundingClientRect().top>=document.querySelector("header").getBoundingClientRect().bottom-1'
                ), (locale, w)
                assert js(
                    'Array.from(document.querySelectorAll("[data-runtime-actor-id] .xterm-screen")).filter(e=>e.getBoundingClientRect().width>0).every(e=>{let parent=e.closest(".xterm").parentElement;return Math.abs(e.getBoundingClientRect().height-parent.clientHeight)<25})'
                ), (locale, w)
                shot("layout-" + locale + "-" + str(w))
            js("groupWorkProbe.setDark(true)")
            shot("dark-" + locale + "-320")
            js("groupWorkProbe.setDark(false)")

        dimensions(1440)
        js('groupWorkProbe.language("en")')
        js("groupWorkProbe.setCount(8)")
        time.sleep(0.3)
        # Presentation must be operable while tiles are visible, including its split viewer.
        click("[data-group-presentation-trigger]")
        time.sleep(0.2)
        shot("presentation-dock")
        js('groupWorkProbe.ui.getState().setChatPresentationDisplayMode("g1","split")')
        js(
            'groupWorkProbe.modals.getState().setPresentationViewer({groupId:"g1",slotId:"slot-1",surface:"split"})'
        )
        time.sleep(0.5)
        shot("presentation-split")
        assert js('!!document.querySelector("[role=separator]")')
        drag_to(850)
        time.sleep(0.4)
        assert len(live()) == 1, live()
        js("groupWorkProbe.modals.getState().setPresentationViewer(null)")
        time.sleep(0.5)
        assert len(live()) == 4
        js('groupWorkProbe.ui.getState().setChatPresentationDockOpen("g1",false)')
        # Stopped/headless Actors remain useful without creating a PTY connection.
        js('groupWorkProbe.ui.getState().setGroupTerminalPage("g1",0)')
        time.sleep(0.3)
        js(
            'groupWorkProbe.patchActor("actor-1",{running:false,enabled:false,effective_working_state:"idle"})'
        )
        js('groupWorkProbe.patchActor("actor-2",{runner:"headless"})')
        js('groupWorkProbe.patchActor("actor-3",{effective_working_state:"waiting"})')
        time.sleep(0.5)
        shot("mixed-runtime-states")
        assert len(live()) == 2, live()
        assert js(
            'document.querySelector("[data-runtime-actor-id=actor-1]").innerText.includes("Stopped")'
        )
        assert js(
            'document.querySelector("[data-runtime-actor-id=actor-3]").innerText.includes("Waiting")'
        )
        # No hidden messages may acquire Voice viewed observations.
        beforeViewed = js(
            'groupWorkProbe.requests.filter(r=>r.path.endsWith("/messages/viewed")).length'
        )
        time.sleep(2.6)
        assert (
            js(
                'groupWorkProbe.requests.filter(r=>r.path.endsWith("/messages/viewed")).length'
            )
            == beforeViewed
        )
        messages()
        time.sleep(2.6)
        assert (
            js(
                'groupWorkProbe.requests.filter(r=>r.path.endsWith("/messages/viewed")).length'
            )
            > beforeViewed
        )
        tiled()
        time.sleep(0.3)
        # Read-only mode still shows output but cannot send raw terminal input.
        js("groupWorkProbe.setReadOnly(true)")
        time.sleep(0.3)
        point_click("[data-runtime-actor-id=actor-3] .xterm-screen")
        typing("must-not-send")
        assert not js(
            'groupWorkProbe.sockets.some(s=>s.frames.some(f=>f.type===48&&f.text.includes("must-not-send")))'
        )
        js("groupWorkProbe.setReadOnly(false)")
        js("groupWorkProbe.setCount(8)")
        time.sleep(0.3)
        # An existing writer stays in control until the user explicitly takes over.
        messages()
        wait("groupWorkProbe.sockets.every(s=>s.readyState===3)")
        js('groupWorkProbe.externalWriters.add("actor-1")')
        tiled()
        wait('!!document.querySelector(`[data-runtime-actor-id=actor-1] [aria-label="Take control"]`)')
        point_click("[data-runtime-actor-id=actor-1] .xterm-screen")
        typing("read-only-attachment")
        assert not js('groupWorkProbe.sockets.filter(s=>s.readyState===1&&s.actor==="actor-1").some(s=>s.frames.some(f=>f.type===48||f.type===50))')
        shot("writer-preserved")
        click('[data-runtime-actor-id=actor-1] [aria-label="Take control"]')
        wait('!groupWorkProbe.externalWriters.has("actor-1")')
        time.sleep(0.3)
        point_click("[data-runtime-actor-id=actor-1] .xterm-screen")
        typing("explicit-takeover")
        assert js('groupWorkProbe.sockets.filter(s=>s.readyState===1&&s.actor==="actor-1").some(s=>s.frames.some(f=>f.type===48&&f.text.includes("explicit-takeover")))')
        # A source jump explicitly returns to message history.
        js('void groupWorkProbe.group.getState().openChatWindow("g1","g1-event-10")')
        wait("!!groupWorkProbe.group.getState().chatByGroup.g1.chatWindow")
        assert js("groupWorkProbe.ui.getState().chatSessions.g1.workView") == "messages"
        js('groupWorkProbe.group.getState().closeChatWindow("g1")')
        tiled()
        time.sleep(0.2)
        js("groupWorkProbe.setCount(0)")
        time.sleep(0.2)
        assert live() == []
        shot("empty-group")
        js("groupWorkProbe.setCount(8)")
        dimensions(390, 844)
        time.sleep(0.4)
        click("[data-mobile-presentation-trigger]")
        time.sleep(0.3)
        assert js('!!document.querySelector("[data-mobile-presentation-surface]")')
        assert live() == []
        key("Escape")
        time.sleep(0.4)
        assert (
            js("groupWorkProbe.ui.getState().chatSessions.g1.workView") == "terminals"
        )
        assert len(live()) == 1
        # The shared menu remains accessible when desktop header controls collapse.
        dimensions(1024)
        time.sleep(0.3)
        click('header [aria-label="Menu"]')
        wait('!!document.querySelector(".mobile-menu-panel")')
        assert rect('.mobile-menu-panel')["width"] > 300
        shot("compact-desktop-menu")
        key("Escape")
        wait('!document.querySelector(".mobile-menu-panel")')
        print("EVIDENCE", str(OUT), flush=True)
        print(
            "PASS input isolation, no focus theft, maximize same xterm/socket, terminal Escape/Tab, four-pane pagination, per-group persistence, reload, responsive/locales, Presentation split/mobile, stopped/headless, read-only input, Voice viewed gating and source navigation",
            flush=True,
        )
        print("ERRORS", js("groupWorkProbe.errors"), flush=True)
        assert not js("groupWorkProbe.errors")
    finally:
        if sock:
            sock.close()
        browser.terminate()
        try:
            browser.wait(timeout=5)
        except subprocess.TimeoutExpired:
            browser.kill()
            browser.wait(timeout=5)
