#!/usr/bin/env python3
"""Browser regression using real AppShell, xterm and synthetic fixture transports.

Requires Chrome, requests and websocket-client. From web/, run an isolated Vite server:
  CCCC_WEB_PORT=19999 node --input-type=module -e 'import {createServer,loadConfigFromFile} from "vite-plus"; const c=await loadConfigFromFile({command:"serve",mode:"development"}); const s=await createServer({...c.config,configFile:false,server:{host:"127.0.0.1",port:15559,strictPort:true,hmr:{host:"127.0.0.1",clientPort:15559},forwardConsole:false,proxy:{}}}); await s.listen();'
The dev client uses this isolated server; all application transports are synthetic.
Then, from the repo root: python3 web/tests/browser/group-work.py
Keep frontend files unchanged while this run is in progress.
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
                'document.querySelector("[data-side-panel-resize]").getBoundingClientRect().toJSON()'
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

        # Ordinary stopped Groups use the existing launch controls. Setup and
        # history notices must never displace the independent side panel.
        click("[data-group-presentation-trigger]")
        wait('!!document.querySelector("#group-side-panel")')
        panel_top = rect("#group-side-panel")["top"]
        js("groupWorkProbe.setRunning(false)")
        wait('document.querySelector("header [data-group-run-control]").innerText === "Stopped"')
        assert not js('!!document.querySelector("[data-chat-notices]")')
        assert rect("#group-side-panel")["top"] == panel_top
        click('header [data-group-run-control]')
        js("[...document.querySelectorAll('[role=menuitem]')].find(b=>b.textContent.startsWith('Start Group')).click()")
        wait('document.querySelector("header [data-group-run-control]").innerText === "Running"')
        js("groupWorkProbe.setCount(0)")
        wait('!!document.querySelector("[data-chat-notices]")')
        assert rect("[data-chat-notices]")["right"] <= rect("#group-side-panel")["left"]
        assert rect("#group-side-panel")["top"] == panel_top
        js("groupWorkProbe.setCount(8); groupWorkProbe.setRunning(true)")
        js('void groupWorkProbe.group.getState().openChatWindow("g1", "g1-event-10")')
        wait('!!document.querySelector("[data-chat-notices]")')
        assert rect("#group-side-panel")["top"] == panel_top
        click('[data-group-presentation-trigger]')
        js('groupWorkProbe.group.getState().closeChatWindow("g1")')
        js("groupWorkProbe.setRunning(false); groupWorkProbe.actions.length=0")
        dimensions(390)
        click('header [data-group-run-control]')
        js("[...document.querySelectorAll('[role=menuitem]')].find(b=>b.textContent.startsWith('Start Group')).click()")
        wait('document.querySelector("header [data-group-run-control]").getAttribute("aria-label").endsWith("Running")')
        js("groupWorkProbe.setRunning(true); groupWorkProbe.actions.length=0")
        dimensions(1440)

        def live():
            return js(
                "groupWorkProbe.sockets.filter(s=>s.readyState===1 && s.group===groupWorkProbe.group.getState().selectedGroupId && [...document.querySelectorAll('[data-runtime-group-id]')].some(e=>e.dataset.runtimeGroupId===s.group && e.dataset.runtimeActorId===s.actor && !e.hasAttribute('inert'))).map(s=>s.actor).sort()"
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

        # Desktop preferences share controls with the narrow-header sheet.
        settings_trigger = "[data-app-settings-trigger]"
        settings_panel = "[data-app-settings-menu]"

        def open_settings_menu():
            # A real mouse click now opens Settings directly; keyboard opens the menu.
            js('document.querySelector("[data-app-settings-trigger]").focus()')
            click(settings_trigger)
            wait('!!document.querySelector("[data-app-settings-menu]")')

        def preference(index, value, container=settings_panel):
            name = ("theme", "textScale", "language")[index]
            trigger = container + f' [data-appearance-select="{name}"]'
            menu = f'[data-appearance-menu="{name}"][data-state="open"]'
            js(f"document.querySelector({json.dumps(trigger)}).scrollIntoView({{block: 'nearest'}})")
            point_click(trigger)
            wait(f"(() => {{ const e = document.querySelector({json.dumps(menu)}); return !!e && getComputedStyle(e).opacity === '1'; }})()")
            option = menu + f' [role="menuitemradio"][data-value="{value}"]'
            js(f"document.querySelector({json.dumps(option)}).scrollIntoView({{block: 'nearest'}})")
            point_click(option)
            wait(f"document.querySelector({json.dumps(trigger)}).dataset.value === {json.dumps(value)}")
            wait(f"!document.querySelector({json.dumps(menu)}) && document.activeElement === document.querySelector({json.dumps(trigger)})")
            assert js(f"!!document.querySelector({json.dumps(container)})")
            assert js(f"document.activeElement === document.querySelector({json.dumps(trigger)})")

        def menu_bounds(selector):
            bounds = rect(selector)
            assert bounds["x"] >= 0 and bounds["y"] >= 0, bounds
            assert bounds["right"] <= js("innerWidth") + 1, bounds
            assert bounds["bottom"] <= js("innerHeight") + 1, bounds
            assert js(f'''Array.from(document.querySelector({json.dumps(selector)}).querySelectorAll('[data-appearance-select]')).every(e => e.scrollWidth <= e.clientWidth + 1)''')

        point_click("[data-group-title-edit]")
        wait('groupWorkProbe.actions.includes("onOpenGroupEdit")')
        point_click('header [aria-label="Context Panel"]')
        assert js('groupWorkProbe.actions.includes("onOpenGroupEdit") && groupWorkProbe.actions.includes("onOpenContext")')
        open_settings_menu()
        wait('!!document.querySelector("[data-app-settings-menu]")')
        menu_bounds(settings_panel)
        assert js('document.activeElement === document.querySelector("[data-app-settings-menu] [data-appearance-select=theme]")')
        key("Tab")
        assert js('document.activeElement === document.querySelector("[data-app-settings-menu] [data-appearance-select=textScale]")')
        key("Tab", shift=True)
        key("Escape")
        wait('!document.querySelector("[data-app-settings-menu]")')
        assert js('document.activeElement.matches("[data-app-settings-trigger]")')

        for locale in ["en", "zh", "ja"]:
            js("groupWorkProbe.language(" + json.dumps(locale) + ")")
            for dark in [False, True]:
                js("groupWorkProbe.setDark(" + json.dumps(dark) + ")")
                open_settings_menu()
                wait('!!document.querySelector("[data-app-settings-menu]")')
                for scale in ["125", "70", "100"]:
                    preference(1, scale)
                    menu_bounds(settings_panel)
                    assert js('document.querySelector("[data-app-settings-menu] [data-appearance-select=textScale]").dataset.value') == scale
                shot("settings-" + locale + ("-dark" if dark else "-light"))
                key("Escape")
                wait('!document.querySelector("[data-app-settings-menu]")')
        js('groupWorkProbe.language("en")')
        js("groupWorkProbe.setDark(false)")
        open_settings_menu()
        preference(0, "dark")
        assert js('document.documentElement.classList.contains("dark")')
        assert js('localStorage.getItem("cccc-theme")') == "dark"
        preference(0, "light")
        preference(2, "ja")
        assert js('document.querySelector("[data-app-settings-menu] [data-appearance-select=language]").dataset.value') == "ja"
        assert js('localStorage.getItem("cccc-language")') == "ja"
        preference(2, "en")
        # The full production SettingsModal must own focus after the popover closes.
        point_click(settings_panel + " > button:last-child")
        wait('!!document.querySelector("[aria-modal=true]") && !document.querySelector("[data-app-settings-menu]")')
        time.sleep(0.3)
        assert js('document.activeElement.closest("[aria-modal=true]")!==null')
        key("Escape")
        wait('!document.querySelector("[aria-modal=true]")')
        assert js('document.activeElement.matches("[data-app-settings-trigger]")')
        open_settings_menu()
        point_click(settings_panel + " > button:first-of-type")
        wait('!!document.querySelector("[aria-modal=true]")')
        assert js('groupWorkProbe.actions.includes("onOpenAccount")')
        key("Escape")
        wait('!document.querySelector("[aria-modal=true]")')
        js("groupWorkProbe.setCanAccessAccount(false)")
        open_settings_menu()
        assert js('document.querySelectorAll("[data-app-settings-menu] > button").length') == 1
        key("Escape")
        js("groupWorkProbe.setCanAccessAccount(true)")
        open_settings_menu()
        js('groupWorkProbe.chooseGroup("g2")')
        wait('!document.querySelector("[data-app-settings-menu]")')
        js('groupWorkProbe.chooseGroup("g1")')
        open_settings_menu()
        dimensions(900)
        wait('!document.querySelector("[data-app-settings-menu]")')
        assert js('document.querySelector("[data-app-settings-trigger]").getClientRects().length') == 0
        point_click('header [aria-label="Menu"]')
        wait('!!document.querySelector(".mobile-menu-panel")')
        for width, height in [(900, 700), (390, 844), (320, 568), (390, 360)]:
            dimensions(width, height)
            preference(1, "125", ".mobile-menu-panel")
            menu_bounds(".mobile-menu-panel")
            js('document.querySelector(".mobile-menu-content").scrollTop=9999')
            time.sleep(0.1)
            assert js('(() => {const e=document.querySelector(".mobile-menu-panel button:last-child");const r=e.getBoundingClientRect();return r.bottom<=innerHeight && r.top>=0;})()')
            shot("settings-mobile-" + str(width) + "-" + str(height))
            preference(1, "100", ".mobile-menu-panel")
        key("Escape")
        wait('!document.querySelector(".mobile-menu-panel")')
        dimensions(1440)
        print("PASS settings choices/locales/scale, account scope, keyboard focus, dialog handoff, responsive dismissal and mobile reachability", flush=True)

        # Wide conversations share their reading bounds with filters and composer.
        dimensions(1920, 1000)
        time.sleep(0.3)
        assert js("""(() => {
          const filters=document.querySelector('[data-message-filters]');
          const log=document.querySelector('[data-group-message-view] [role=log]');
          const rows=log.querySelector('.chat-reading-width').getBoundingClientRect();
          const input=document.querySelector('footer .chat-reading-width').getBoundingClientRect();
          const controls=filters.querySelector('.chat-reading-width').getBoundingClientRect();
          return filters.getBoundingClientRect().bottom <= log.getBoundingClientRect().top + 1
            && Math.abs(rows.left-input.left)<2 && Math.abs(rows.width-input.width)<2
            && Math.abs(controls.left-input.left)<2 && rows.width<log.clientWidth-100;
        })()""")
        point_click('[data-group-presentation-trigger]')
        wait('!!document.querySelector("#group-side-panel")')
        assert js("""(() => {
          const footer=document.querySelector('footer').getBoundingClientRect();
          const work=document.querySelector('[data-chat-work-surface]').getBoundingClientRect();
          const panel=document.querySelector('#group-side-panel').getBoundingClientRect();
          return Math.abs(footer.right-work.right)<1 && footer.right<panel.left
            && Math.abs(footer.bottom-panel.bottom)<1 && work.bottom<=footer.top+1;
        })()""")
        point_click('[data-group-presentation-trigger]')
        dimensions(1440)
        time.sleep(0.3)
        print("PASS reading alignment, non-overlapping filters and full-height side panel", flush=True)

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
        wait('[...document.querySelectorAll("[data-runtime-group-id]")].filter(e=>!e.hasAttribute("inert")).length===4')
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
        # The modal schedules initial focus on the next animation frame.
        wait('document.activeElement.closest("[aria-modal=true]")!==null')
        key("Escape")
        wait('!document.querySelector("[aria-modal=true]")')
        assert js("groupWorkProbe.sockets.length") == opens
        assert js('document.activeElement.getAttribute("aria-label")') == "Controls for Foreman"
        messages()
        wait("[...document.querySelectorAll('[data-runtime-group-id]')].every(e=>e.hasAttribute('inert'))")
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
        assert js("groupWorkProbe.sockets.length") == opens
        tiled()
        wait('[...document.querySelectorAll("[data-runtime-group-id]")].filter(e=>!e.hasAttribute("inert")).length===4')
        # Retain scrollback, selection and connection while visiting another page/Group.
        js("""(async()=>{
          window.keptTerm=groupWorkProbe.terminals.find(t=>t.element?.closest('[data-runtime-actor-id=actor-1]'));
          window.keptSocket=groupWorkProbe.sockets.find(s=>s.readyState===1&&s.actor==='actor-1');
          await new Promise(done=>keptTerm.write(Array.from({length:200},(_,i)=>'cache line '+i+'\\r\\n').join(''),done));
          keptTerm.scrollToLine(30); keptTerm.select(0,32,8);
          window.keptScroll=keptTerm.buffer.active.viewportY;
          window.keptSelection=keptTerm.getSelection();
        })()""")
        click('[aria-label="Next page"]')
        wait('groupWorkProbe.sockets.filter(s=>s.readyState===1).length===8')
        assert visible_panes() == ["actor-5", "actor-6", "actor-7", "actor-8"]
        js('groupWorkProbe.chooseGroup("g2")')
        time.sleep(0.3)
        assert live() == [], live()
        assert js('keptSocket.readyState') == 1
        assert js('keptTerm._core._renderService._isPaused')
        frames = js('keptSocket.frames.filter(f=>f.type===48||f.type===50).length')
        js("keptTerm.input('must-not-reach-hidden',true); keptTerm.resize(31,9)")
        assert js('keptSocket.frames.filter(f=>f.type===48||f.type===50).length') == frames
        # Restore the local test dimensions; navigation fitting uses the visible container.
        js('keptTerm.resize(80,24)')
        tiled()
        wait('groupWorkProbe.sockets.filter(s=>s.readyState===1).length===12')
        assert js('document.querySelector("[data-runtime-group-id=g2][data-runtime-actor-id=actor-1] .xterm-screen")!==window.savedTerminal')
        point_click('[data-runtime-group-id=g2][data-runtime-actor-id=actor-1] .xterm-screen')
        typing('second-group-input')
        assert not js('keptSocket.frames.some(f=>f.type===48&&f.text.includes("second-group-input"))')
        js('groupWorkProbe.chooseGroup("g1")')
        time.sleep(0.3)
        assert visible_panes() == ["actor-5", "actor-6", "actor-7", "actor-8"]
        click('[aria-label="Previous page"]')
        assert js('document.querySelector("[data-runtime-group-id=g1][data-runtime-actor-id=actor-1] .xterm-screen")===window.savedTerminal')
        assert js('keptSocket.readyState') == 1
        # Capture after the deliberate resize, then verify ordinary navigation changes nothing.
        js("keptTerm.scrollToLine(30);keptTerm.select(0,32,8);window.keptScroll=keptTerm.buffer.active.viewportY;window.keptSelection=keptTerm.getSelection();window.retainedOpens=groupWorkProbe.sockets.length")
        click('[aria-label="Next page"]')
        js('groupWorkProbe.chooseGroup("g2")')
        time.sleep(0.2)
        js('groupWorkProbe.chooseGroup("g1")')
        time.sleep(0.2)
        click('[aria-label="Previous page"]')
        assert js('keptTerm.buffer.active.viewportY===keptScroll && keptTerm.getSelection()===keptSelection')
        assert js('groupWorkProbe.sockets.length===retainedOpens')
        # Ownership can return while hidden after another window changed the PTY
        # dimensions. An unchanged xterm fit must still resynchronize the writer.
        js('window.localSizeBeforeHide={cols:keptTerm.cols,rows:keptTerm.rows}')
        click('[aria-label="Next page"]')
        hidden_resizes = js('keptSocket.frames.filter(f=>f.type===50).length')
        for writable in [False, True]:
            js('keptSocket.onmessage({data:new TextEncoder().encode('+json.dumps('6'+json.dumps({'terminal_writable':writable}))+').buffer})')
            time.sleep(.1)
        assert js('keptSocket.frames.filter(f=>f.type===50).length') == hidden_resizes
        click('[aria-label="Previous page"]')
        wait(f'keptSocket.frames.filter(f=>f.type===50).length>{hidden_resizes}')
        time.sleep(.3)
        assert js('keptTerm.cols===localSizeBeforeHide.cols && keptTerm.rows===localSizeBeforeHide.rows')
        assert js('keptSocket.frames.filter(f=>f.type===50).length') == hidden_resizes + 1
        assert js('JSON.stringify(JSON.parse(keptSocket.frames.filter(f=>f.type===50).at(-1).text))===JSON.stringify(localSizeBeforeHide)')
        assert js('keptSocket.readyState===1 && groupWorkProbe.sockets.length===retainedOpens')
        print('PASS deferred writer resize after hidden ownership handoff, without reconnect or duplicate resize', flush=True)
        click('[aria-label="Next page"]')
        js("window.groupWorkReloadPending=true")
        cdp("Page.reload")
        wait(
            '!window.groupWorkReloadPending && !!window.groupWorkProbe && groupWorkProbe.sockets.filter(s=>s.readyState===1).map(s=>s.actor).sort().join(",")==="actor-5,actor-6,actor-7,actor-8"'
        )
        # Width changes keep the focused Actor, then the visible page anchor.
        point_click('[data-runtime-actor-id="actor-7"] .xterm-screen')
        dimensions(700)
        wait('[...document.querySelectorAll("[data-runtime-group-id]")].filter(e=>!e.hasAttribute("inert")).map(e=>e.dataset.runtimeActorId).join(",")==="actor-7"')
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
        # Closing a card leaves its side panel open. The header toggle closes the panel.
        assert js('document.querySelector("[data-group-presentation-trigger]").getAttribute("aria-expanded")') == "true"
        assert len(live()) == 1
        click("[data-group-presentation-trigger]")
        wait('[...document.querySelectorAll("[data-runtime-group-id]")].filter(e=>!e.hasAttribute("inert")).length===4')
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
        # Coarse pointers enlarge touch controls: notices must not displace actions.
        js('groupWorkProbe.patchActor("actor-4",{effective_working_state:"stuck"})')
        cdp("Emulation.setTouchEmulationEnabled", {"enabled": True, "maxTouchPoints": 1})
        dimensions(320, 568)
        for scale in [100, 125]:
            js(f'groupWorkProbe.setTextScale({scale})')
            for index, status in [(0, "Stopped"), (2, "Waiting"), (3, "Stuck")]:
                js(f'groupWorkProbe.ui.getState().setGroupTerminalPage("g1",{index})')
                time.sleep(0.3)
                shot("touch-status-" + status.lower() + "-" + str(scale))
                assert js(f'''(() => {{
                    const name = document.querySelector('#runtime-inspector-g1-actor-{index + 1}');
                    const status = Array.from(name.parentElement.children).find(e => e.textContent === {json.dumps(status)});
                    return name.getBoundingClientRect().width >= 60 && !!status && status.getBoundingClientRect().width >= 25;
                }})()'''), status
                assert js('''Array.from(document.querySelectorAll('[data-runtime-actor-id] button')).filter(e=>e.getClientRects().length).every(e=>{const r=e.getBoundingClientRect();return r.right<=innerWidth && r.left>=0;})'''), status
        js('groupWorkProbe.setTextScale(100)')
        cdp("Emulation.setTouchEmulationEnabled", {"enabled": False})
        dimensions(1440)
        js('groupWorkProbe.patchActor("actor-4",{effective_working_state:"working"})')
        js('groupWorkProbe.ui.getState().setGroupTerminalPage("g1",0)')
        time.sleep(0.3)
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
        def readonly_touch_history(actor):
            # Use xterm's public viewport state: rendered rows can be overscanned
            # and the fixture keeps producing live output during the gesture.
            pane = f'[data-runtime-actor-id={actor}]'
            js(f'void(window.touchTerminal=groupWorkProbe.terminals.find(t=>t.element?.isConnected&&t.element.closest({json.dumps(pane)})))')
            for mode in [1000, 1002, 1003]:
                js(f'''(async()=>{{touchTerminal.reset();
                    await new Promise(resolve=>touchTerminal.write(Array.from({{length:200}},(_,i)=>'audit-line '+i+'\\r\\n').join('')+'\\x1b[?1006h\\x1b[?'+{mode}+'h',resolve));
                    touchTerminal.scrollToBottom();}})()''')
                before = js('touchTerminal.buffer.active.viewportY')
                screen = rect(pane + ' .xterm-screen')
                cell_height = screen["height"] / js('touchTerminal.rows')
                start = {"x": screen["x"] + screen["width"] / 2, "y": screen["y"] + screen["height"] / 3}
                end = {"x": start["x"], "y": start["y"] + 3 * cell_height + 0.2}
                sent = js(f'groupWorkProbe.sockets.filter(s=>s.actor==={json.dumps(actor)}).flatMap(s=>s.frames).filter(f=>f.type===48).length')
                cdp("Emulation.setTouchEmulationEnabled", {"enabled": True})
                cdp("Input.dispatchTouchEvent", {"type": "touchStart", "touchPoints": [start]})
                cdp("Input.dispatchTouchEvent", {"type": "touchMove", "touchPoints": [end]})
                cdp("Input.dispatchTouchEvent", {"type": "touchEnd", "touchPoints": []})
                wait(f'touchTerminal.buffer.active.viewportY==={before-3}')
                assert js(f'groupWorkProbe.sockets.filter(s=>s.actor==={json.dumps(actor)}).flatMap(s=>s.frames).filter(f=>f.type===48).length') == sent
                cdp("Emulation.setTouchEmulationEnabled", {"enabled": False})
            print("PASS Actor read-only touch", actor, flush=True)

        # Read-only mode still shows output but cannot send raw terminal input.
        js("groupWorkProbe.setReadOnly(true)")
        time.sleep(0.3)
        point_click("[data-runtime-actor-id=actor-3] .xterm-screen")
        typing("must-not-send")
        assert not js(
            'groupWorkProbe.sockets.some(s=>s.frames.some(f=>f.type===48&&f.text.includes("must-not-send")))'
        )
        readonly_touch_history("actor-3")
        js("groupWorkProbe.setReadOnly(false)")
        js("groupWorkProbe.setCount(8)")
        time.sleep(0.3)
        # An existing writer stays in control until the user explicitly takes over.
        messages()
        js("""groupWorkProbe.externalWriters.add('actor-1'); groupWorkProbe.sockets.filter(s=>s.readyState===1&&s.actor==='actor-1').forEach(s=>{s.frames.length=0;s.onmessage({data:new TextEncoder().encode('6{"terminal_writable":false}').buffer})})""")
        tiled()
        wait('!!document.querySelector(`[data-runtime-actor-id=actor-1] [aria-label="Take control"]`)')
        point_click("[data-runtime-actor-id=actor-1] .xterm-screen")
        typing("read-only-attachment")
        assert not js('groupWorkProbe.sockets.filter(s=>s.readyState===1&&s.actor==="actor-1").some(s=>s.frames.some(f=>f.type===48||f.type===50))')
        readonly_touch_history("actor-1")
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
        dimensions(900)
        time.sleep(0.3)
        click('header [aria-label="Menu"]')
        wait('!!document.querySelector(".mobile-menu-panel")')
        assert rect('.mobile-menu-panel')["width"] > 300
        shot("compact-desktop-menu")
        key("Escape")
        wait('!document.querySelector(".mobile-menu-panel")')
        # Hiding virtualized messages must not replace their measured heights with zero.
        dimensions(1440, 1000)
        messages()
        js("""(() => {
          groupWorkProbe.ui.getState().setChatFilter('g1', 'all');
          const base=groupWorkProbe.group.getState().chatByGroup.g1.events;
          groupWorkProbe.group.getState().setEvents(Array.from({length:120},(_,i)=>({
            ...base[i%base.length],id:`history-${i}`,
            ts:new Date(Date.UTC(2026,8,17,0,0,i)).toISOString()
          })), 'g1');
        })()""")
        time.sleep(1)
        log = rect(logselector)
        cdp("Input.dispatchMouseEvent", {
            "type": "mouseWheel", "x": log["x"]+log["width"]/2,
            "y": log["y"]+100, "deltaY": -6000, "deltaX": 0,
        })
        time.sleep(0.5)
        anchor = js("""(() => {
          const log=document.querySelector('[data-group-message-view] [role=log]');
          const top=log.getBoundingClientRect().top;
          const row=Array.from(log.querySelectorAll('[data-message-row]'))
            .find(e=>e.getBoundingClientRect().bottom>top);
          return {id:row.dataset.messageId,offset:row.getBoundingClientRect().top-top,
            scroll:log.scrollTop,total:log.scrollHeight,height:log.clientHeight};
        })()""")
        assert 1000 < anchor["scroll"] < anchor["total"]-anchor["height"]-500, anchor
        tiled()
        time.sleep(0.4)
        messages()
        time.sleep(0.6)
        restored = js("""(() => {
          const log=document.querySelector('[data-group-message-view] [role=log]');
          const row=log.querySelector('[data-message-id="' + """ + json.dumps(anchor["id"]) + """ + '"]');
          return row ? row.getBoundingClientRect().top-log.getBoundingClientRect().top : null;
        })()""")
        assert restored is not None and abs(restored-anchor["offset"])<2, (anchor, restored)
        print("PASS virtual message identity and visible offset across Terminals/ Messages", flush=True)
        # Refreshed reading surfaces retain only the last successful version of
        # the same resource. These requests are local synthetic responses.
        js(r'''window.assetFixtureFetch=window.fetch;
          window.assetStatus=200; window.assetReads=0; window.assetKind='markdown';window.assetRevision=0;
          window.assetHold=false; window.assetRespond=null;
          window.savedPresentation=groupWorkProbe.group.getState().groupPresentation;
          window.fetch=async(input,init)=>{
            if(!String(input).includes('/presentation/slots/'))return assetFixtureFetch(input,init);
            assetReads++;
            if(assetHold)await new Promise(resolve=>{assetRespond=resolve;});
            return new Response(assetStatus!==200?'unavailable':assetKind==='markdown'
              ? '# Stable document\n\n'+Array.from({length:160},(_,i)=>'Paragraph '+i+' version '+assetRevision+'\n\n').join('')
              : '<svg xmlns="http://www.w3.org/2000/svg" width="2400" height="1800"><rect width="2400" height="1800" fill="#125665"/><text x="50" y="100" font-size="60" fill="white">Stable image</text></svg>',
              {status:assetStatus,headers:{'Content-Type':assetKind==='markdown'?'text/markdown':'image/svg+xml'}});
          };
          window.showAsset=(kind,surface,revision='one')=>{
            assetKind=kind;
            if(surface==='split')groupWorkProbe.ui.getState().setChatPresentationDockOpen('g1',true);
            groupWorkProbe.group.setState({groupPresentation:{v:1,slots:[{slot_id:'slot-1',index:1,card:{slot_id:'slot-1',title:'Reading continuity',card_type:kind,published_at:revision,published_by:'actor-1',content:{mode:'workspace_link',workspace_rel_path:kind==='markdown'?'sample.md':'sample.svg'}}}]}});
            groupWorkProbe.modals.getState().setPresentationViewer({groupId:'g1',slotId:'slot-1',surface,focusRef:kind==='markdown'?{kind:'presentation_ref',slot_id:'slot-1',card_type:'markdown',locator:{viewer_scroll_top:300}}:null});
          };''')
        for surface in ["modal", "split"]:
            js(f"assetStatus=200;showAsset('markdown','{surface}')")
            wait('[...document.querySelectorAll("h1")].some(e=>e.textContent==="Stable document" && e.getClientRects().length)')
            time.sleep(.3)
            js('''window.oldHeading=[...document.querySelectorAll('h1')].find(e=>e.textContent==='Stable document' && e.getClientRects().length);window.reader=oldHeading.parentElement;
              while(reader&&reader.scrollHeight<=reader.clientHeight+1)reader=reader.parentElement;
              window.quotedScroll=reader.scrollTop;reader.scrollTop=500;window.initialScroll=reader.scrollTop;''')
            assert js('quotedScroll') == 300
            assert js('initialScroll') == 500
            reads = js('assetReads')
            wait(f'assetReads>{reads}')
            assert js('oldHeading.isConnected && reader.scrollTop===initialScroll')
            js('assetStatus=503')
            click('[aria-label="Refresh"]')
            wait('document.body.textContent.includes("Update failed. Showing the last loaded version.")')
            assert js('oldHeading.isConnected && reader.scrollTop===initialScroll')
            shot('markdown-stale-' + surface)
            js('assetStatus=200')
            click('[aria-label="Refresh"]')
            wait('!document.body.textContent.includes("Update failed. Showing the last loaded version.")')
            assert js('oldHeading.isConnected && reader.scrollTop===initialScroll')
            js('assetRevision++')
            click('[aria-label="Refresh"]')
            wait('!oldHeading.isConnected')
            assert js('reader.scrollTop') == 500
            js('assetStatus=403')
            click('[aria-label="Refresh"]')
            wait('document.body.textContent.includes("HTTP 403")')
            js('groupWorkProbe.modals.getState().setPresentationViewer(null)')
            time.sleep(.2)

            js(f"assetStatus=200;showAsset('image','{surface}')")
            wait('!!document.querySelector("[data-graphic-viewer] img") && document.querySelector("[data-graphic-viewer] img").naturalWidth===2400')
            click('[aria-label="Actual size"]')
            js('''window.oldImage=document.querySelector('[data-graphic-viewer] img');
              window.graphicViewport=document.querySelector('[data-graphic-viewer] [role=region]');
              graphicViewport.scrollLeft=400;graphicViewport.scrollTop=500;
              window.displayedSrc=oldImage.src; assetHold=true;''')
            click('[aria-label="Refresh"]')
            wait('!!assetRespond')
            assert js('oldImage.isConnected && oldImage.src===displayedSrc && getComputedStyle(oldImage.parentElement).visibility==="visible"')
            js('assetStatus=503;assetHold=false;assetRespond();assetRespond=null')
            wait('document.body.textContent.includes("Update failed. Showing the last loaded version.")')
            assert js('oldImage.isConnected && oldImage.src===displayedSrc && graphicViewport.scrollLeft===400 && graphicViewport.scrollTop===500')
            shot('image-stale-' + surface)
            js('assetStatus=200')
            click('[aria-label="Refresh"]')
            wait('oldImage.src!==displayedSrc && oldImage.complete')
            assert js('oldImage.isConnected && getComputedStyle(oldImage.parentElement).visibility==="visible" && graphicViewport.scrollLeft===400 && graphicViewport.scrollTop===500')
            # A new publication is a different resource: do not show the old image
            # while its request is pending, or apply its zoom to the new resource.
            js(f"assetHold=true;showAsset('image','{surface}','two')")
            wait('!!assetRespond')
            assert not js('oldImage.isConnected')
            js('assetHold=false;assetRespond();assetRespond=null')
            wait('!!document.querySelector("[data-graphic-viewer] img") && document.querySelector("[data-graphic-viewer] img").complete')
            assert js('document.querySelector("[data-graphic-viewer] [role=region]").scrollTop===0')
            js('assetStatus=403')
            click('[aria-label="Refresh"]')
            wait('!document.querySelector("[data-graphic-viewer] img") && document.body.textContent.includes("HTTP 403")')
            js('groupWorkProbe.modals.getState().setPresentationViewer(null)')
            time.sleep(.2)
        js('window.fetch=assetFixtureFetch;groupWorkProbe.group.setState({groupPresentation:savedPresentation})')
        if js('document.querySelector("[data-group-presentation-trigger]")?.getAttribute("aria-expanded")==="true"'):
            click('[data-group-presentation-trigger]')
        print('PASS Markdown/image pending, failure, recovery, permission loss and publication isolation in modal/split', flush=True)

        js('groupWorkProbe.setCount(8);groupWorkProbe.patchActor("actor-1",{running:true,runner:"pty"})')
        tiled()
        js('groupWorkProbe.ui.getState().setGroupTerminalPage("g1",0)')
        wait('!!document.querySelector(\'[aria-label="Next page"]\')')
        pager = rect('[aria-label="Next page"]')
        assert pager['width'] >= 44 and pager['height'] >= 44, pager
        dimensions(900)
        time.sleep(.3)
        point_click('[aria-label="Next page"]')
        assert js('document.activeElement.getAttribute("aria-label")') == 'Next page'
        point_click('[aria-label="Previous page"]')
        assert not js('document.activeElement.closest(".xterm")')
        cdp("Emulation.setTouchEmulationEnabled", {"enabled": True, "maxTouchPoints": 2})
        dimensions(390, 844)
        time.sleep(.5)
        js('groupWorkProbe.ui.getState().setGroupTerminalPage("g1",0)')
        time.sleep(.3)
        def swipe(selector, dx, dy=0):
            r = rect(selector)
            x = r['x'] + r['width']*.7
            y = r['y'] + min(16, r['height']/2)
            cdp('Input.dispatchTouchEvent', {'type':'touchStart','touchPoints':[{'x':x,'y':y}]})
            for part in range(1, 6):
                cdp('Input.dispatchTouchEvent', {'type':'touchMove','touchPoints':[{'x':x+dx*part/5,'y':y+dy*part/5}]})
                time.sleep(.025)
            cdp('Input.dispatchTouchEvent', {'type':'touchEnd','touchPoints':[]})
            time.sleep(.2)
        title = '[data-runtime-actor-id="actor-1"] [data-terminal-title-bar]'
        wait(f'!!document.querySelector({json.dumps(title)})')
        swipe(title, -110)
        wait('groupWorkProbe.ui.getState().chatSessions.g1.terminalPage===1')
        swipe('[data-runtime-actor-id="actor-2"] [data-terminal-title-bar]', 90)
        wait('groupWorkProbe.ui.getState().chatSessions.g1.terminalPage===0')
        swipe(title, -110, 70)
        assert js('groupWorkProbe.ui.getState().chatSessions.g1.terminalPage') == 0
        swipe('[aria-label="Next page"]', -90)
        assert js('groupWorkProbe.ui.getState().chatSessions.g1.terminalPage') == 0
        swipe('[data-runtime-actor-id="actor-1"] .xterm-screen', -110)
        assert js('groupWorkProbe.ui.getState().chatSessions.g1.terminalPage') == 0
        shot('touch-terminal-paging')
        cdp("Emulation.setTouchEmulationEnabled", {"enabled": False})
        print('PASS 44px pagination targets, keyboard focus and title-only touch paging without body/button/vertical interception', flush=True)

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
