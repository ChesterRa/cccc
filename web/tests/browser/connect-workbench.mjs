// Full native-workbench browser regression against isolated Homes and real routers.
// Build the debug CLI and cccc-web lib test binary from an isolated source copy.
// CCCC_CONNECT_WEB_TEST_BIN names that test binary; no existing service is used.
// CCCC_CONNECT_CLI may select an extracted release binary for the fixture daemons.
import assert from "node:assert/strict";
import http from "node:http";
import https from "node:https";
import net from "node:net";
import {
  mkdtempSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
  existsSync,
  rmSync,
  openSync,
  closeSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import { randomUUID } from "node:crypto";
import WebSocket from "ws";
import { connectGroupJourney } from "./connect-group-journey.mjs";
import { connectMeasurements } from "./connect-metrics.mjs";

const root = resolve(fileURLToPath(new URL("../../../", import.meta.url)));
const dir = mkdtempSync(join(tmpdir(), "cccc-connect-workbench-"));
const processes = [],
  servers = [],
  logFiles = [],
  sockets = new Set();
const groupProbe = process.env.CCCC_CONNECT_GROUPS_PROBE === "1";
const scaleProbe = process.env.CCCC_CONNECT_SCALE_PROBE === "1";
const traffic = Array.from({ length: 3 }, () => ({
  requestBytes: 0,
  responseBytes: 0,
  httpOpen: 0,
  wsOpen: 0,
}));
const daemonProcesses = [];
const requests = [],
  failures = [];
let cdp;
let diagnose;
let spawnError;
const delay = (ms) => new Promise((r) => setTimeout(r, ms));
async function eventually(check, label, timeout = 15000) {
  const until = Date.now() + timeout;
  while (Date.now() < until) {
    if (spawnError) throw spawnError;
    if (await check()) return;
    await delay(100);
  }
  throw new Error(`Timed out: ${label}`);
}
function start(command, args, env = {}) {
  const fd = openSync(join(dir, `process-${processes.length}.log`), "w", 0o600);
  logFiles.push(fd);
  const process = spawn(command, args, {
    cwd: dir,
    env: { ...globalThis.process.env, ...env },
    stdio: ["ignore", fd, fd],
  });
  process.on("error", (error) => {
    spawnError ||= error;
  });
  processes.push(process);
  return process;
}
async function listen(server, host = "127.0.0.1") {
  servers.push(server);
  server.on("connection", (socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
  });
  server.listen(0, host);
  await once(server, "listening");
  return server.address().port;
}
async function ipc(home, op, args = {}) {
  const address = JSON.parse(readFileSync(join(home, "daemon/ccccd.addr.json"), "utf8"));
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(address.path);
    let text = "";
    socket.setTimeout(15000, () => socket.destroy(new Error(`IPC timeout: ${op}`)));
    socket.on("error", reject);
    socket.on("connect", () => socket.write(JSON.stringify({ v: 1, op, args }) + "\n"));
    socket.on("data", (chunk) => {
      text += chunk;
      if (!text.includes("\n")) return;
      socket.end();
      const response = JSON.parse(text.split("\n")[0]);
      if (response.ok) resolve(response.result);
      else reject(new Error(`${op}: ${JSON.stringify(response.error)}`));
    });
  });
}
function proxy(index, tls, upstreams) {
  const server = (tls ? https : http).createServer(tls || {}, (request, response) => {
    const pathname = new URL(request.url, "http://fixture").pathname;
    requests.push({ index, method: request.method, path: pathname, url: request.url });
    traffic[index].httpOpen++;
    response.once("close", () => traffic[index].httpOpen--);
    request.on("data", (chunk) => (traffic[index].requestBytes += chunk.length));
    if (!upstreams[index]) {
      response.writeHead(503);
      response.end();
      return;
    }
    const upstream = http.request(
      new URL(request.url, upstreams[index]),
      {
        method: request.method,
        headers: {
          ...request.headers,
          "x-forwarded-host": request.headers.host,
          "x-forwarded-proto": tls ? "https" : "http",
        },
      },
      (incoming) => {
        response.writeHead(incoming.statusCode, incoming.headers);
        incoming.on("data", (chunk) => (traffic[index].responseBytes += chunk.length));
        incoming.pipe(response);
      },
    );
    upstream.on("error", () => {
      if (!response.headersSent) response.writeHead(502);
      response.end();
    });
    response.on("close", () => upstream.destroy());
    request.pipe(upstream);
  });
  server.on("upgrade", (request, socket, head) => {
    requests.push({
      index,
      method: "WS",
      path: new URL(request.url, "http://fixture").pathname,
      url: request.url,
    });
    traffic[index].wsOpen++;
    socket.once("close", () => traffic[index].wsOpen--);
    if (!upstreams[index]) {
      socket.destroy();
      return;
    }
    const target = new URL(upstreams[index]);
    const upstream = net.createConnection(Number(target.port), target.hostname);
    sockets.add(upstream);
    upstream.on("close", () => sockets.delete(upstream));
    upstream.on("error", () => socket.destroy());
    socket.on("error", () => upstream.destroy());
    socket.on("close", () => upstream.destroy());
    upstream.on("connect", () => {
      const headers = {
        ...request.headers,
        "x-forwarded-host": request.headers.host,
        "x-forwarded-proto": tls ? "https" : "http",
      };
      upstream.write(
        `${request.method} ${request.url} HTTP/1.1\r\n${Object.entries(headers)
          .map(([k, v]) => `${k}: ${v}`)
          .join("\r\n")}\r\n\r\n`,
      );
      if (head.length) upstream.write(head);
      socket.on("data", (chunk) => (traffic[index].requestBytes += chunk.length));
      upstream.on("data", (chunk) => (traffic[index].responseBytes += chunk.length));
      socket.pipe(upstream);
      upstream.pipe(socket);
    });
  });
  return server;
}

try {
  assert(process.env.CCCC_CONNECT_WEB_TEST_BIN, "build and specify the isolated Web test binary");
  execFileSync(
    "openssl",
    [
      "req",
      "-x509",
      "-newkey",
      "rsa:2048",
      "-nodes",
      "-days",
      "1",
      "-keyout",
      join(dir, "ca-key.pem"),
      "-out",
      join(dir, "ca.pem"),
      "-subj",
      "/CN=CCCC isolated fixture CA",
    ],
    { stdio: "ignore" },
  );
  execFileSync(
    "openssl",
    [
      "req",
      "-new",
      "-newkey",
      "rsa:2048",
      "-nodes",
      "-keyout",
      join(dir, "key.pem"),
      "-out",
      join(dir, "server.csr"),
      "-subj",
      "/CN=127.0.0.2",
    ],
    { stdio: "ignore" },
  );
  writeFileSync(
    join(dir, "server.ext"),
    "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=IP:127.0.0.2,IP:127.0.0.3\n",
  );
  execFileSync(
    "openssl",
    [
      "x509",
      "-req",
      "-in",
      join(dir, "server.csr"),
      "-CA",
      join(dir, "ca.pem"),
      "-CAkey",
      join(dir, "ca-key.pem"),
      "-CAcreateserial",
      "-days",
      "1",
      "-extfile",
      join(dir, "server.ext"),
      "-out",
      join(dir, "cert.pem"),
    ],
    { stdio: "ignore" },
  );
  execFileSync(
    "openssl",
    ["verify", "-CAfile", join(dir, "ca.pem"), "-verify_ip", "127.0.0.2", join(dir, "cert.pem")],
    { stdio: "ignore" },
  );
  const tls = {
    key: readFileSync(join(dir, "key.pem")),
    cert: readFileSync(join(dir, "cert.pem")),
  };
  const upstreams = [];
  const origins = [];
  for (let index = 0; index < 3; index++) {
    const secure = index > 0;
    const host = `127.0.0.${index + 1}`;
    const port = await listen(proxy(index, secure ? tls : null, upstreams), host);
    origins.push(`${secure ? "https" : "http"}://${host}:${port}`);
  }
  const accountPath = join(dir, "account.json");
  start(
    "node",
    [
      "--experimental-transform-types",
      join(root, "../cccc-homepage/account/scripts/connect-fixture.mjs"),
      accountPath,
      ...(groupProbe ? ["--groups"] : []),
    ],
    groupProbe ? { NODE_EXTRA_CA_CERTS: join(dir, "ca.pem") } : {},
  );
  await eventually(() => existsSync(accountPath), "account fixture");
  const account = JSON.parse(readFileSync(accountPath, "utf8"));
  const homes = [],
    tokens = [],
    groups = [];
  for (let index = 0; index < 3; index++) {
    const home = join(dir, `home-${index}`);
    homes.push(home);
    mkdirSync(join(home, "secrets"), { recursive: true });
    writeFileSync(join(home, ".cccc-rust-v1"), "CCCC Rust home v1\n");
    writeFileSync(
      join(home, "secrets/membership.json"),
      JSON.stringify({
        ...account.devices[index],
        logged_in: true,
        account_origin: account.origin,
      }),
      { mode: 0o600 },
    );
    writeFileSync(
      join(home, "settings.yaml"),
      JSON.stringify({
        remote_access: { provider: "manual", enabled: true, web_public_url: origins[index] },
      }),
    );
    const token = randomUUID();
    tokens.push(token);
    const now = new Date().toISOString();
    writeFileSync(
      join(home, "access_tokens.yaml"),
      JSON.stringify({
        tokens: {
          [randomUUID()]: {
            user_id: `backup-admin-${index}`,
            is_admin: true,
            allowed_groups: [],
            created_at: now,
            updated_at: now,
          },
          [token]: {
            user_id: `admin-${index}`,
            is_admin: true,
            allowed_groups: [],
            created_at: now,
            updated_at: now,
          },
        },
      }),
      { mode: 0o600 },
    );
    daemonProcesses.push(
      start(process.env.CCCC_CONNECT_CLI || join(root, "target/debug/cccc"), ["daemon", "run"], {
        CCCC_HOME: home,
        CCCC_ACCOUNT_ORIGIN: account.origin,
      }),
    );
    await eventually(() => existsSync(join(home, "daemon/ccccd.addr.json")), "daemon address");
    await eventually(async () => {
      try {
        return Boolean(await ipc(home, "ping"));
      } catch {
        return false;
      }
    }, "daemon readiness");
    if (scaleProbe)
      for (let group = 1; group < 10; group++) {
        await ipc(home, "group_create", { title: `Extra ${index}-${group}` });
      }
    const { group_id: groupId } = await ipc(home, "group_create", {
      title: `Workspace ${index === 0 ? "A" : index === 1 ? "B" : "C"}`,
    });
    groups.push(groupId);
    const scope = join(dir, `workspace-${index}`);
    mkdirSync(scope);
    await ipc(home, "attach", { group_id: groupId, path: scope, by: "user" });
    await ipc(home, "actor_add", {
      group_id: groupId,
      actor_id: "fixture-terminal",
      title: "Fixture Terminal",
      runtime: "custom",
      command: [
        "python3",
        "-u",
        "-c",
        "import sys; print('CONNECT_TERMINAL_READY', flush=True)\nfor line in sys.stdin: print('ECHO:'+line.strip(), flush=True)",
      ],
      by: "user",
    });
    await ipc(home, "actor_start", { group_id: groupId, actor_id: "fixture-terminal", by: "user" });
    if (scaleProbe && index === 1)
      for (let actor = 2; actor <= 4; actor++) {
        const actorId = `fixture-terminal-${actor}`;
        await ipc(home, "actor_add", {
          group_id: groupId,
          actor_id: actorId,
          title: `Fixture ${actor}`,
          runtime: "custom",
          command: [
            "python3",
            "-u",
            "-c",
            "import sys; print('CONNECT_TERMINAL_READY', flush=True)\nfor line in sys.stdin: print('ECHO:'+line.strip(), flush=True)",
          ],
          by: "user",
        });
        await ipc(home, "actor_start", { group_id: groupId, actor_id: actorId, by: "user" });
      }
    writeFileSync(join(scope, "fixture.txt"), `attachment-${index}`);
    await ipc(home, "send_files", {
      group_id: groupId,
      paths: [join(scope, "fixture.txt")],
      text: `Fixture attachment ${index}`,
      by: "fixture-terminal",
      to: ["user"],
      message_mode: "send",
    });
    await ipc(home, "presentation_publish", {
      group_id: groupId,
      by: "user",
      slot: "slot-1",
      card_type: "web_preview",
      title: "Nested fixture",
      content: "<!doctype html><html><body><h1>CONNECT_PRESENTATION_READY</h1></body></html>",
    });
  }
  const configFile = join(dir, "web-config.json"),
    readyFile = join(dir, "web-ready.json");
  writeFileSync(
    configFile,
    JSON.stringify({ homes, ca_certificate: join(dir, "ca.pem"), ready_file: readyFile }),
    { mode: 0o600 },
  );
  const webProcess = start(
    process.env.CCCC_CONNECT_WEB_TEST_BIN,
    ["connect_browser_fixture_when_enabled", "--ignored", "--nocapture"],
    { CCCC_CONNECT_BROWSER_FIXTURE: configFile, CCCC_WEB_TRUST_PROXY_HEADERS: "1" },
  );
  await eventually(() => existsSync(readyFile), "native Web fixtures");
  upstreams.push(...JSON.parse(readFileSync(readyFile, "utf8")));
  await eventually(
    () =>
      homes.every((home, index) => {
        const path = join(home, "secrets/connect.json");
        return (
          existsSync(path) &&
          JSON.parse(readFileSync(path, "utf8")).directory?.instances.length ===
            (groupProbe ? (index === 1 ? 1 : 2) : 3)
        );
      }),
    "three-instance directory",
    75000,
  );

  const browser = start(process.env.CHROME_BIN || "/usr/bin/google-chrome", [
    "--headless=new",
    "--no-sandbox",
    "--ignore-certificate-errors",
    "--window-size=1360,900",
    "--remote-debugging-port=0",
    `--user-data-dir=${join(dir, "profile")}`,
    "about:blank",
  ]);
  const metrics = scaleProbe
    ? connectMeasurements({
        roots: () => ({
          daemonA: daemonProcesses[0].pid,
          daemonB: daemonProcesses[1].pid,
          daemonC: daemonProcesses[2].pid,
          webRouters: webProcess.pid,
          browser: browser.pid,
        }),
        traffic,
        requests,
        accountOrigin: account.origin,
        file: process.env.CCCC_CONNECT_METRICS_FILE || "/tmp/cccc-connect-performance.json",
      })
    : null;
  const portFile = join(dir, "profile/DevToolsActivePort");
  await eventually(() => existsSync(portFile) || browser.exitCode !== null, "Chrome");
  const port = readFileSync(portFile, "utf8").split("\n")[0];
  const tab = await (
    await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })
  ).json();
  cdp = new WebSocket(tab.webSocketDebuggerUrl);
  await once(cdp, "open");
  let sequence = 0;
  const pending = new Map();
  const realtimeSockets = new Map();
  cdp.on("message", (raw) => {
    const message = JSON.parse(raw);
    if (!message.id) {
      if (message.method === "Runtime.exceptionThrown")
        failures.push(message.params.exceptionDetails.text);
      const key = `${message.sessionId || "main"}:${message.params?.requestId}`;
      if (message.method === "Network.webSocketCreated") {
        const url = new URL(message.params.url);
        if (url.pathname === "/api/v1/events/ws")
          realtimeSockets.set(key, { host: url.host, ready: new Set(), events: new Set() });
      }
      if (message.method === "Network.webSocketFrameReceived" && realtimeSockets.has(key)) {
        const frame = message.params.response;
        if (frame.opcode === 1) {
          const packet = JSON.parse(frame.payloadData);
          const socket = realtimeSockets.get(key);
          if (packet.type === "ready") socket.ready.add(packet.channel);
          if (packet.type === "event") socket.events.add(packet.message.event);
        }
      }
      return;
    }
    const callback = pending.get(message.id);
    pending.delete(message.id);
    if (!callback) return;
    if (message.error) callback.reject(new Error(JSON.stringify(message.error)));
    else callback.resolve(message.result);
  });
  const call = (method, params = {}, sessionId) =>
    new Promise((resolve, reject) => {
      const id = ++sequence;
      pending.set(id, { resolve, reject });
      cdp.send(JSON.stringify({ id, method, params, sessionId }));
    });
  const evaluate = async (expression, sessionId) => {
    const result = await call(
      "Runtime.evaluate",
      { expression, awaitPromise: true, returnByValue: true },
      sessionId,
    );
    if (result.exceptionDetails)
      throw new Error(
        result.exceptionDetails.exception?.description || result.exceptionDetails.text,
      );
    return result.result.value;
  };
  diagnose = async () => {
    const text = await evaluate("(document.body?.innerText || '').slice(0,6000)");
    writeFileSync(join(dir, "entry-state.txt"), text || "empty");
    const screenshot = await call("Page.captureScreenshot", { format: "png" });
    writeFileSync(join(dir, "entry.png"), Buffer.from(screenshot.data, "base64"));
    writeFileSync(join(dir, "request-paths.json"), JSON.stringify(requests));
  };
  const cookieControls = {
    enableThirdPartyCookieRestriction: true,
    disableThirdPartyCookieMetadata: true,
    disableThirdPartyCookieHeuristics: true,
  };
  await call("Page.enable");
  await call("Runtime.enable");
  await call("Network.enable");
  await call("Network.setCookieControls", cookieControls);
  await call("Browser.setDownloadBehavior", { behavior: "allow", downloadPath: dir });
  if (groupProbe) {
    await connectGroupJourney({
      call,
      evaluate,
      eventually,
      ipc,
      homes,
      groups,
      origins,
      tokens,
      account,
      dir,
      failures,
    });
  } else {
    process.stdout.write("Three daemons and native Web routers ready; opening the entry.\n");
    await call("Page.navigate", { url: `${origins[0]}/ui/` });
    await eventually(
      () =>
        evaluate(
          "!!document.querySelector('input[name=cccc-access-token]') || (document.body?.innerText || '').includes('Workspace A')",
        ),
      "entry login or workbench",
    );
    if (await evaluate("!!document.querySelector('input[name=cccc-access-token]')")) {
      await evaluate(
        `(()=>{const input=document.querySelector('input[name=cccc-access-token]'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,${JSON.stringify(tokens[0])}); input.dispatchEvent(new Event('input',{bubbles:true}));})()`,
      );
      await delay(50);
      await evaluate("document.querySelector('form').requestSubmit()");
    }
    await eventually(
      () => evaluate("(document.body?.innerText || '').includes('Workspace A')"),
      "entry workbench",
    );
    await evaluate(
      `fetch('/api/v1/web_access/session',{headers:{Authorization:'Bearer '+${JSON.stringify(tokens[0])}}}).then(r=>r.json()).then(r=>r.ok)`,
    );
    await eventually(
      () => evaluate("(document.body?.innerText || '').includes('fixture-b')"),
      "Connect sidebar",
    );
    if (metrics) await metrics.sample("local_entry_idle", delay);
    await evaluate(
      "[...document.querySelectorAll('aside button')].find(b=>b.getAttribute('aria-label')?.startsWith('fixture-b ·')).click()",
    );
    process.stdout.write("Entry administrator and Connect sidebar ready; opening B.\n");
    let frameInfo;
    await eventually(async () => {
      frameInfo = (await call("Target.getTargets")).targetInfos.find(
        (target) => target.type === "iframe" && target.url.startsWith(origins[1] + "/ui/connect/"),
      );
      return Boolean(frameInfo);
    }, "target frame");
    let { sessionId: world } = await call("Target.attachToTarget", {
      targetId: frameInfo.targetId,
      flatten: true,
    });
    await call("Runtime.enable", {}, world);
    await call("Network.enable", {}, world);
    await call("Page.enable", {}, world);
    await call("Network.setCookieControls", cookieControls, world);
    await eventually(
      () => evaluate("!!document.querySelector('input[name=cccc-access-token]')", world),
      "target administrator login",
    );
    const login = async (token) => {
      await evaluate(
        `(()=>{const input=document.querySelector('input[name=cccc-access-token]'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,${JSON.stringify(token)}); input.dispatchEvent(new Event('input',{bubbles:true}));})()`,
        world,
      );
      await delay(50);
      await evaluate("document.querySelector('form').requestSubmit()", world);
    };
    await login(tokens[0]);
    await delay(500);
    assert(
      await evaluate("!!document.querySelector('input[name=cccc-access-token]')", world),
      "A token must not unlock B",
    );
    await login(tokens[1]);
    if (scaleProbe) {
      await eventually(
        () =>
          evaluate(
            "!![...document.querySelectorAll('aside button')].find(b=>b.textContent.trim()==='Workspace B')",
          ),
        "B main Group listed",
      );
      await evaluate(
        "[...document.querySelectorAll('aside button')].find(b=>b.textContent.trim()==='Workspace B').click()",
      );
    }
    await eventually(
      () =>
        evaluate(
          "(document.body?.innerText || '').includes('Workspace B') && !document.querySelector('input[name=cccc-access-token]')",
          world,
        ),
      "target workbench",
    );
    await eventually(
      () => evaluate("document.querySelector('aside').innerText.includes('Workspace B')"),
      "target Group metadata",
    );
    await eventually(
      () =>
        [...realtimeSockets.values()].some(
          (socket) =>
            socket.host === new URL(origins[1]).host &&
            ["global", "ledger", "headless"].every((channel) => socket.ready.has(channel)) &&
            socket.events.has("headless.snapshot"),
        ),
      "native target multiplexed subscriptions and snapshot",
    );
    if (scaleProbe)
      await evaluate(
        "[...document.querySelectorAll('aside button')].find(b=>b.textContent.trim()==='Workspace B').click()",
      );
    const terminalStart = performance.now();
    await evaluate(
      "[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Terminals').click()",
      world,
    );
    await eventually(
      () => requests.some((r) => r.index === 1 && r.method === "WS" && r.path.endsWith("/term")),
      "native target terminal",
    );
    await eventually(
      () => evaluate("!!document.querySelector('.xterm-helper-textarea')", world),
      "interactive terminal",
    );
    if (metrics) {
      await eventually(
        () => evaluate("document.querySelectorAll('.xterm-helper-textarea').length===4", world),
        "four visible terminal panes",
      );
      metrics.action("open_four_terminals", performance.now() - terminalStart);
      await metrics.sample("remote_four_terminals", delay);
      const hiddenTab = await call("Target.createTarget", { url: "about:blank" });
      await call("Target.activateTarget", { targetId: hiddenTab.targetId });
      await eventually(
        () => evaluate("document.visibilityState==='hidden'"),
        "entry hidden by another tab",
      );
      await metrics.sample("hidden_remote_entry", delay);
      await call("Target.closeTarget", { targetId: hiddenTab.targetId });
      await call("Page.bringToFront");
      await eventually(
        () => evaluate("document.visibilityState==='visible'"),
        "entry visible again",
      );
      const second = await call("Target.createTarget", { url: `${origins[0]}/ui/` });
      await delay(1500);
      const multiple = await metrics.sample("two_entries", delay);
      assert.equal(
        multiple.account.providerCalls,
        0,
        "browser entry count does not query the tunnel provider",
      );
      await call("Target.closeTarget", { targetId: second.targetId });
      await call("Page.bringToFront");
    }
    await evaluate("document.querySelector('.xterm-helper-textarea').focus()", world);
    await call("Input.insertText", { text: "CONNECT_TYPED" }, world);
    await call(
      "Input.dispatchKeyEvent",
      { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, text: "\r" },
      world,
    );
    await eventually(
      () =>
        evaluate(
          "[...document.querySelectorAll('.xterm-rows')].some(e=>e.textContent.includes('ECHO:CONNECT_TYPED'))",
          world,
        ),
      "terminal typed input",
    );
    await call("Emulation.setDeviceMetricsOverride", {
      width: 1050,
      height: 750,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await delay(400);
    assert(
      await evaluate("document.documentElement.scrollWidth<=window.innerWidth", world),
      "target does not overflow after resize",
    );
    await evaluate(
      "[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Messages').click()",
      world,
    );
    await eventually(
      () => evaluate("!!document.querySelector('a[download=\"fixture.txt\"]')", world),
      "attachment download",
    );
    await evaluate("document.querySelector('a[download=\"fixture.txt\"]').click()", world);
    await eventually(() => existsSync(join(dir, "fixture.txt")), "authenticated browser download");
    assert.equal(readFileSync(join(dir, "fixture.txt"), "utf8"), "attachment-1");
    // Use the real file picker and composer; verify storage and dispatch stay in B.
    const uploadPath = join(dir, "uploaded-from-browser.txt");
    writeFileSync(uploadPath, "B-only upload");
    const dom = await call("DOM.getDocument", {}, world);
    const input = await call(
      "DOM.querySelector",
      { nodeId: dom.root.nodeId, selector: 'input[type="file"]' },
      world,
    );
    await call("DOM.setFileInputFiles", { nodeId: input.nodeId, files: [uploadPath] }, world);
    await eventually(
      () =>
        evaluate(
          "!!document.querySelector('button[aria-label=\"Send message\"]:not(:disabled)')",
          world,
        ),
      "file composer readiness",
    );
    await evaluate("document.querySelector('button[aria-label=\"Send message\"]').click()", world);
    await eventually(
      () =>
        requests.some(
          (r) => r.index === 1 && r.method === "POST" && r.path.endsWith("/send_upload"),
        ),
      "target file upload",
    );
    await eventually(
      () =>
        readFileSync(join(homes[1], "groups", groups[1], "ledger.jsonl"), "utf8").includes(
          "uploaded-from-browser.txt",
        ),
      "target upload ledger",
    );
    await eventually(
      () =>
        [...realtimeSockets.values()].some(
          (socket) => socket.host === new URL(origins[1]).host && socket.events.has("ledger"),
        ),
      "target live ledger event over WebSocket",
    );
    assert(
      !readFileSync(join(homes[0], "groups", groups[0], "ledger.jsonl"), "utf8").includes(
        "uploaded-from-browser.txt",
      ),
      "entry ledger is untouched by target upload",
    );
    process.stdout.write(
      "Target login, multiplexed events, terminal input/resize and file upload/download passed.\n",
    );
    const cookies = (await call("Storage.getCookies")).cookies.filter((cookie) =>
      cookie.name.startsWith("__Host-cccc_access_"),
    );
    assert(
      cookies.some(
        (cookie) =>
          cookie.value === tokens[1] && cookie.partitionKey && cookie.httpOnly && cookie.secure,
      ),
      "target uses its own partitioned HttpOnly cookie",
    );
    assert.equal(
      await evaluate(
        "(()=>{try{return document.querySelector('iframe').contentWindow.document.body.innerText}catch{return 'isolated'}})()",
      ),
      "isolated",
    );
    // Exercise the nested, sandboxed Presentation in the authenticated target.
    await evaluate("document.querySelector('[data-group-presentation-trigger]').click()", world);
    await eventually(
      () =>
        evaluate(
          "!!document.querySelector('button[aria-label=\"Open presentation slot 1: Nested fixture\"]')",
          world,
        ),
      "presentation slot",
    );
    await evaluate(
      "document.querySelector('button[aria-label=\"Open presentation slot 1: Nested fixture\"]').click()",
      world,
    );
    await eventually(
      () => evaluate("!!document.querySelector('iframe[title=\"Nested fixture\"]')", world),
      "nested presentation",
    );
    let preview;
    await eventually(async () => {
      preview = (await call("Target.getTargets")).targetInfos.find(
        (target) =>
          target.type === "iframe" && target.url.startsWith(origins[1] + "/api/v1/groups/"),
      );
      return Boolean(preview);
    }, "sandboxed presentation target");
    const { sessionId: previewWorld } = await call("Target.attachToTarget", {
      targetId: preview.targetId,
      flatten: true,
    });
    await eventually(
      () =>
        evaluate(
          "(document.body?.innerText || '').includes('CONNECT_PRESENTATION_READY')",
          previewWorld,
        ),
      "authenticated nested document rendered",
    );
    await evaluate("document.querySelector('[data-group-presentation-trigger]').click()", world);
    const openInstance = async (index, name) => {
      await evaluate(
        `[...document.querySelectorAll('aside button')].find(b=>b.getAttribute('aria-label')?.startsWith(${JSON.stringify(name + " ·")})).click()`,
      );
      let target;
      await eventually(async () => {
        target = (await call("Target.getTargets")).targetInfos.find(
          (item) => item.type === "iframe" && item.url.startsWith(origins[index] + "/ui/connect/"),
        );
        return Boolean(target);
      }, `${name} frame`);
      ({ sessionId: world } = await call("Target.attachToTarget", {
        targetId: target.targetId,
        flatten: true,
      }));
      await call("Runtime.enable", {}, world);
      await call("Network.enable", {}, world);
      await call("Page.enable", {}, world);
      await call("Network.setCookieControls", cookieControls, world);
    };
    await evaluate("document.querySelector('[data-workspace-files-toggle]').click()", world);
    await eventually(
      () =>
        evaluate(
          "[...document.querySelectorAll('[role=treeitem]')].some(e=>e.textContent.includes('fixture.txt'))",
          world,
        ),
      "remote workspace file tree",
    );
    await evaluate(
      "[...document.querySelectorAll('[role=treeitem]')].find(e=>e.textContent.includes('fixture.txt')).click()",
      world,
    );
    await eventually(
      () =>
        evaluate(
          "[...document.querySelectorAll('textarea')].some(e=>e.value==='attachment-1')",
          world,
        ),
      "remote workspace editor",
    );
    await evaluate(
      "(()=>{const e=[...document.querySelectorAll('textarea')].find(e=>e.value==='attachment-1');e.focus();e.select();})()",
      world,
    );
    await call("Input.insertText", { text: "UNSAVED REMOTE EDIT" }, world);
    await eventually(
      () =>
        evaluate(
          "(()=>{const e=new Event('beforeunload',{cancelable:true});window.dispatchEvent(e);return e.defaultPrevented;})()",
        ),
      "entry learns remote dirty state",
    );
    await evaluate(
      "[...document.querySelectorAll('aside [role=button]')].find(b=>b.textContent.trim()==='Workspace A').click()",
    );
    await eventually(
      () =>
        evaluate(
          "[...document.querySelectorAll('[role=dialog]')].some(e=>e.textContent.includes('Leave unsaved file edits?'))",
        ),
      "entry protects remote drafts",
    );
    await evaluate(
      "[...document.querySelectorAll('[role=dialog] button')].find(e=>e.textContent==='Stay here').click()",
    );
    assert(
      await evaluate(
        "[...document.querySelectorAll('textarea')].some(e=>e.value==='UNSAVED REMOTE EDIT')",
        world,
      ),
      "cancel preserves remote edits and frame",
    );
    await evaluate(
      "[...document.querySelectorAll('aside [role=button]')].find(b=>b.textContent.trim()==='Workspace A').click()",
    );
    await eventually(
      () =>
        evaluate(
          "[...document.querySelectorAll('[role=dialog] button')].some(e=>e.textContent==='Discard and continue')",
        ),
      "explicit discard choice",
    );
    await evaluate(
      "[...document.querySelectorAll('[role=dialog] button')].find(e=>e.textContent==='Discard and continue').click()",
    );
    assert.equal(
      readFileSync(join(dir, "workspace-1/fixture.txt"), "utf8"),
      "attachment-1",
      "navigation never saves files implicitly",
    );
    await eventually(
      () => evaluate("document.querySelectorAll('iframe').length===0"),
      "local navigation releases the remote iframe",
    );
    assert(
      await evaluate("document.querySelector('aside').innerText.includes('Workspace B')"),
      "local selection preserves B navigation",
    );
    await evaluate(
      "document.querySelector('button[aria-label=\"Collapse groups in fixture-b\"]').click()",
    );
    assert(
      !(await evaluate("document.querySelector('aside').innerText.includes('Workspace B')")),
      "explicit collapse hides B groups",
    );
    await evaluate(
      "document.querySelector('button[aria-label=\"Expand groups in fixture-b\"]').click()",
    );
    assert(
      await evaluate("document.querySelector('aside').innerText.includes('Workspace B')"),
      "explicit expansion restores B groups",
    );
    if (process.env.CCCC_CONNECT_SCREENSHOT_DIR) {
      mkdirSync(process.env.CCCC_CONNECT_SCREENSHOT_DIR, { recursive: true });
      const screenshot = await call("Page.captureScreenshot", { format: "png" });
      writeFileSync(
        join(process.env.CCCC_CONNECT_SCREENSHOT_DIR, "local-with-remote-navigation.png"),
        Buffer.from(screenshot.data, "base64"),
      );
    }
    process.stdout.write(
      "Local selection preserves remote navigation, explicit collapse/expand works, and the inactive iframe closes.\n",
    );
    await openInstance(2, "fixture-c");
    await eventually(
      () => evaluate("!!document.querySelector('input[name=cccc-access-token]')", world),
      "C remains locked",
    );
    await login(tokens[1]);
    await delay(500);
    assert(
      await evaluate("!!document.querySelector('input[name=cccc-access-token]')", world),
      "B token must not unlock C",
    );
    await login(tokens[2]);
    if (scaleProbe) {
      await eventually(
        () =>
          evaluate(
            "!![...document.querySelectorAll('aside button')].find(b=>b.textContent.trim()==='Workspace C')",
          ),
        "C main Group listed",
      );
      await evaluate(
        "[...document.querySelectorAll('aside button')].find(b=>b.textContent.trim()==='Workspace C').click()",
      );
    }
    await eventually(
      () =>
        evaluate(
          "(document.body?.innerText || '').includes('Workspace C') && !document.querySelector('input[name=cccc-access-token]')",
          world,
        ),
      "C administrator workbench",
    );
    await openInstance(1, "fixture-b");
    await eventually(
      () =>
        evaluate(
          "(document.body?.innerText || '').includes('Workspace B') && !document.querySelector('input[name=cccc-access-token]')",
          world,
        ),
      "B login reused after C",
    );
    assert.equal(
      await evaluate("document.querySelectorAll('iframe').length"),
      1,
      "only the active target is mounted",
    );
    process.stdout.write("Nested Presentation, separate C login and B session reuse passed.\n");
    // A real settings logout ends the single-use frame instead of reloading its proof.
    await call("Page.bringToFront");
    await evaluate("document.querySelector('iframe').focus()");
    await evaluate("document.querySelector('[data-app-settings-trigger]').focus()", world);
    const focusEvidence = await evaluate(
      "({focused:document.hasFocus(),active:document.activeElement?.tagName,activeTrigger:document.activeElement?.hasAttribute('data-app-settings-trigger'),triggers:[...document.querySelectorAll('[data-app-settings-trigger]')].map(b=>({visible:!!b.getClientRects().length,disabled:b.disabled,rect:b.getBoundingClientRect().toJSON()}))})",
      world,
    );
    assert(
      focusEvidence.focused && focusEvidence.activeTrigger,
      `target settings owns keyboard focus: ${JSON.stringify(focusEvidence)}`,
    );
    await call("Input.dispatchKeyEvent", {
      type: "keyDown",
      key: "Enter",
      code: "Enter",
      windowsVirtualKeyCode: 13,
      text: "\r",
    });
    await call("Input.dispatchKeyEvent", {
      type: "keyUp",
      key: "Enter",
      code: "Enter",
      windowsVirtualKeyCode: 13,
    });
    await eventually(
      () => evaluate("!!document.querySelector('[data-app-settings-menu]')", world),
      "keyboard opens target settings menu",
    );
    await evaluate(
      "[...document.querySelectorAll('[data-app-settings-menu] button')].find(b=>b.textContent.trim()==='Settings').click()",
      world,
    );
    await eventually(
      () =>
        evaluate(
          "!![...document.querySelectorAll('[role=dialog] button')].find(b=>b.getClientRects().length && b.textContent.trim().startsWith('This instance'))",
          world,
        ),
      "target settings scope",
    );
    await evaluate(
      "[...document.querySelectorAll('[role=dialog] button')].find(b=>b.getClientRects().length && b.textContent.trim().startsWith('This instance')).click()",
      world,
    );
    await eventually(
      () =>
        evaluate(
          "!![...document.querySelectorAll('[role=dialog] button')].find(b=>b.getClientRects().length && b.textContent.trim()==='Account')",
          world,
        ),
      "target Account tab",
    );
    await evaluate(
      "[...document.querySelectorAll('[role=dialog] button')].find(b=>b.getClientRects().length && b.textContent.trim()==='Account').click()",
      world,
    );
    await eventually(
      () =>
        evaluate(
          "document.querySelector('[data-testid=connect-status]')?.textContent.includes('Background collaboration is enabled')",
          world,
        ),
      "target account confirmation uses target daemon state",
    );
    assert(
      await evaluate(
        "document.querySelector('[data-testid=connect-status]')?.parentElement.textContent.includes('each instance')",
        world,
      ),
      "account panel explains separate Workbench authorization",
    );
    await evaluate(
      "document.querySelector('[data-testid=connect-status]').scrollIntoView({block:'center'})",
      world,
    );
    for (const name of ["Browser workstation B", "fixture-b"]) {
      await eventually(
        () => evaluate("!!document.querySelector('input[name=connect-instance-name]')", world),
        "instance name input",
      );
      await evaluate(
        `(()=>{const input=document.querySelector('input[name=connect-instance-name]'); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(input,${JSON.stringify(name)}); input.dispatchEvent(new Event('input',{bubbles:true}));})()`,
        world,
      );
      await delay(50);
      await evaluate(
        "document.querySelector('input[name=connect-instance-name]').closest('form').requestSubmit()",
        world,
      );
      await eventually(
        () =>
          evaluate(
            `document.querySelector('input[name=connect-instance-name]')?.value===${JSON.stringify(name)} && !document.querySelector('input[name=connect-instance-name]').disabled && document.querySelector('input[name=connect-instance-name]').closest('form').querySelector('button').disabled && !document.querySelector('[data-testid=connect-status] [role=alert]')`,
            world,
          ),
        "account name saved",
      );
      assert.equal(
        JSON.parse(
          readFileSync(join(homes[1], "secrets/connect.json"), "utf8"),
        ).directory.instances.find((entry) => entry.device_id === account.devices[1].device_id)
          .display_name,
        name,
        "native daemon refreshed the account-owned name",
      );
    }
    process.stdout.write(
      "Native account naming port and settings editor preserve the active workbench.\n",
    );
    const accountPanel = await call("Page.captureScreenshot", { format: "png" });
    writeFileSync("/tmp/cccc-connect-account-panel.png", Buffer.from(accountPanel.data, "base64"));
    await call("Emulation.setDeviceMetricsOverride", {
      width: 390,
      height: 844,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await eventually(() => evaluate("innerWidth <= 390", world), "narrow target account panel");
    await evaluate("document.querySelector('button[aria-label=\"Close sidebar\"]').click()");
    await eventually(
      () => evaluate("document.querySelector('aside').getBoundingClientRect().right <= 1"),
      "parent navigation closes so account controls are visible",
    );
    await evaluate(
      "document.querySelector('[data-testid=connect-status]').scrollIntoView({block:'center'})",
      world,
    );
    assert(
      await evaluate("document.documentElement.scrollWidth <= innerWidth + 1", world),
      "account settings does not overflow on a narrow entry",
    );
    const narrowAccountPanel = await call("Page.captureScreenshot", { format: "png" });
    writeFileSync(
      "/tmp/cccc-connect-account-panel-mobile.png",
      Buffer.from(narrowAccountPanel.data, "base64"),
    );
    await call("Emulation.setDeviceMetricsOverride", {
      width: 1050,
      height: 750,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await eventually(
      () =>
        evaluate(
          "!![...document.querySelectorAll('[role=dialog] button')].find(b=>b.getClientRects().length && b.textContent.trim()==='Web Access')",
          world,
        ),
      "target Web Access tab",
    );
    await evaluate(
      "[...document.querySelectorAll('[role=dialog] button')].find(b=>b.getClientRects().length && b.textContent.trim()==='Web Access').click()",
      world,
    );
    await eventually(
      () =>
        evaluate(
          "!![...document.querySelectorAll('[role=dialog] button')].find(b=>b.textContent.trim()==='Sign out')",
          world,
        ),
      "target sign out action",
    );
    await evaluate(
      "[...document.querySelectorAll('[role=dialog] button')].find(b=>b.textContent.trim()==='Sign out').click()",
      world,
    );
    await eventually(
      () =>
        evaluate(
          "!document.querySelector('iframe') && !!document.querySelector('[data-testid=connect-remote-panel] [role=alert]')",
        ),
      "target logout returns to entry",
    );
    await evaluate(
      "[...document.querySelectorAll('[data-testid=connect-remote-panel] button')].find(b=>b.textContent.trim()==='Retry').click()",
    );
    await openInstance(1, "fixture-b");
    await eventually(
      () => evaluate("!!document.querySelector('input[name=cccc-access-token]')", world),
      "reopened target requires login after logout",
    );
    await login(tokens[1]);
    if (scaleProbe) {
      await eventually(
        () =>
          evaluate(
            "!![...document.querySelectorAll('aside button')].find(b=>b.textContent.trim()==='Workspace B')",
          ),
        "B main Group listed",
      );
      await evaluate(
        "[...document.querySelectorAll('aside button')].find(b=>b.textContent.trim()==='Workspace B').click()",
      );
    }
    await eventually(
      () =>
        evaluate(
          "(document.body?.innerText || '').includes('Workspace B') && !document.querySelector('input[name=cccc-access-token]')",
          world,
        ),
      "target login after fresh frame",
    );
    await call("Emulation.setDeviceMetricsOverride", {
      width: 390,
      height: 844,
      deviceScaleFactor: 1,
      mobile: false,
    });
    await eventually(() => evaluate("innerWidth===390"), "narrow entry viewport");
    assert(
      await evaluate("document.documentElement.scrollWidth <= innerWidth + 1"),
      "entry has no horizontal overflow",
    );
    assert(
      await evaluate("document.documentElement.scrollWidth <= innerWidth + 1", world),
      "target has no horizontal overflow",
    );
    const groupsButton =
      "[...document.querySelectorAll('[data-testid=connect-remote-panel] button')].find(b=>b.textContent.trim()==='Working Groups')";
    assert(
      await evaluate(`!!${groupsButton}?.getClientRects().length`),
      "mobile Group navigation remains reachable",
    );
    await evaluate(`${groupsButton}.click()`);
    await eventually(
      () =>
        evaluate(
          "!![...document.querySelectorAll('aside button')].find(b=>b.getClientRects().length && b.getAttribute('aria-label')?.startsWith('fixture-c ·'))",
        ),
      "mobile sidebar exposes other instances",
    );
    writeFileSync(
      "/tmp/cccc-connect-workbench-mobile.png",
      Buffer.from((await call("Page.captureScreenshot", { format: "png" })).data, "base64"),
    );
    await call("Emulation.clearDeviceMetricsOverride");
    process.stdout.write(
      "Target settings keyboard, logout/reopen and narrow viewport navigation passed.\n",
    );
    // Keep an auxiliary read-only socket outside React to prove server revocation.
    const terminalPath = `/api/v1/groups/${groups[1]}/actors/fixture-terminal/term`;
    const activeFrame = await evaluate(
      "JSON.parse(new TextDecoder().decode(Uint8Array.from(atob(new URLSearchParams(location.search).get('proof').replace(/-/g,'+').replace(/_/g,'/')),c=>c.charCodeAt(0)))).frame_id",
      world,
    );
    await evaluate(
      "[...document.querySelectorAll('button')].find(b=>b.textContent.trim()==='Terminals').click()",
      world,
    );
    await eventually(
      () =>
        requests.some(
          (request) =>
            request.index === 1 &&
            request.path === terminalPath &&
            new URL(request.url, origins[1]).searchParams.get("connect_frame") === activeFrame,
        ),
      "current frame terminal URL",
    );
    for (const path of [terminalPath, "/api/v1/events/ws", new URL(preview.url).pathname]) {
      const actual = requests.filter((request) => request.index === 1 && request.path === path);
      assert(actual.length > 0, `native workbench opened ${path}`);
      assert(
        actual.every((request) =>
          new URL(request.url, origins[1]).searchParams.has("connect_frame"),
        ),
        `native workbench binds ${path} to its frame`,
      );
    }
    const socketRequest = requests.findLast(
      (request) =>
        request.index === 1 &&
        request.path === terminalPath &&
        new URL(request.url, origins[1]).searchParams.get("connect_frame") === activeFrame,
    );
    assert(socketRequest, "current frame opened its native terminal connection");
    await evaluate(
      `(()=>{ const url=new URL(${JSON.stringify(socketRequest.url)},location.origin);url.protocol='wss:';url.searchParams.set('mode','viewer');url.searchParams.delete('takeover'); window.fixtureSocket=new WebSocket(url);window.fixtureSocket.onclose=()=>window.fixtureSocketClosed=true;})()`,
      world,
    );
    await eventually(
      () => evaluate("window.fixtureSocket?.readyState===WebSocket.OPEN", world),
      "independent target socket",
    );
    const realtimeRequest = requests.findLast(
      (request) =>
        request.index === 1 &&
        request.path === "/api/v1/events/ws" &&
        new URL(request.url, origins[1]).searchParams.get("connect_frame") === activeFrame,
    );
    assert(realtimeRequest, "current frame opened its event socket");
    await evaluate(
      `(()=>{const url=new URL(${JSON.stringify(realtimeRequest.url)},location.origin);url.protocol='wss:';const socket=window.fixtureRealtime=new WebSocket(url);socket.onopen=()=>socket.send(JSON.stringify({type:'subscribe',channel:'global',id:1}));socket.onmessage=e=>{if(JSON.parse(e.data).type==='ready')window.fixtureRealtimeReady=true;};socket.onclose=()=>window.fixtureRealtimeClosed=true;})()`,
      world,
    );
    await eventually(
      () => evaluate("window.fixtureRealtimeReady===true", world),
      "independent target event subscription",
    );
    const editCurrentToken = async (index, method, body, session) => {
      const result = await evaluate(
        `(async()=>{const list=await fetch('/api/v1/access-tokens').then(r=>r.json());const current=list.result.access_tokens.find(t=>t.user_id===${JSON.stringify(`admin-${index}`)});return fetch('/api/v1/access-tokens/'+current.token_id,{method:${JSON.stringify(method)},headers:{'Content-Type':'application/json'},body:${body ? `JSON.stringify(${JSON.stringify(body)})` : "undefined"}}).then(r=>r.json())})()`,
        session,
      );
      assert(result.ok, `${method} current fixture token`);
    };
    await editCurrentToken(1, "DELETE", null, world);
    await eventually(
      () =>
        evaluate("window.fixtureSocketClosed===true && window.fixtureRealtimeClosed===true", world),
      "server closes already-open revoked terminal and event sockets",
      20000,
    );
    await eventually(
      () => evaluate("!!document.querySelector('input[name=cccc-access-token]')", world),
      "B re-locks after revocation",
      20000,
    );
    assert(
      (await ipc(homes[1], "group_show", { group_id: groups[1] })).group.actors.some(
        (actor) => actor.id === "fixture-terminal" && actor.running,
      ),
      "revoking Web access must not stop the Actor",
    );
    await editCurrentToken(0, "PATCH", { is_admin: false, allowed_groups: [groups[0]] });
    await eventually(
      () =>
        evaluate(
          "!document.querySelector('iframe') && !(document.querySelector('aside')?.innerText || '').includes('fixture-b')",
        ),
      "restricted entry becomes single-instance",
      20000,
    );
    assert(
      await evaluate("(document.body?.innerText || '').includes('Workspace A')"),
      "entry retains its permitted local Group",
    );
    process.stdout.write(
      "Target revocation and entry downgrade converged without stopping Actors.\n",
    );
    assert.equal(failures.length, 0, `browser exceptions: ${failures.join(", ")}`);
    process.stdout.write(
      JSON.stringify({
        nativeWorkbenches: true,
        targetAdminLogin: true,
        entryTokenIsolation: true,
        thirdPartyCookiesBlocked: true,
        nativeRealtimeWebSocket: true,
        terminalInputAndResize: true,
        authenticatedDownload: true,
        targetUpload: true,
        nestedPresentation: true,
        targetSessionReuse: true,
        remoteDraftNavigation: true,
        targetLogoutReopen: true,
        keyboardSettings: true,
        accountConnectionStatus: true,
        narrowViewportNavigation: true,
        liveRevocation: true,
        restrictedEntry: true,
        exceptions: failures.length,
      }) + "\n",
    );
  }
} catch (error) {
  if (diagnose) await Promise.race([diagnose().catch(() => {}), delay(3000)]);
  process.stderr.write(`${error.stack || error}\nFixture evidence retained at ${dir}\n`);
  process.exitCode = 1;
} finally {
  cdp?.close();
  for (const socket of sockets) socket.destroy();
  for (const server of servers) server.close();
  for (const process of processes.reverse())
    if (process.exitCode === null) {
      process.kill("SIGTERM");
      await Promise.race([once(process, "exit"), delay(3000)]);
      if (process.exitCode === null) process.kill("SIGKILL");
    }
  for (const fd of logFiles) closeSync(fd);
  if (!process.exitCode) rmSync(dir, { recursive: true, force: true });
}
