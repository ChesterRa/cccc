// Transport feasibility probe, not the full Connect workbench acceptance test.
// Uses an isolated Chrome profile, loopback servers, generated test TLS and fake data.
import http from "node:http";
import https from "node:https";
import { mkdtempSync, readFileSync, existsSync, rmSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { execFileSync, spawn } from "node:child_process";
import { once } from "node:events";
import assert from "node:assert/strict";
import WebSocket, { WebSocketServer } from "ws";

const dir = mkdtempSync(join(tmpdir(), "cccc-connect-browser-"));
const servers = [];
const sockets = new Set();
const downloadRequests = [];
const downloadEvents = [];
let sharedHostCookieVisibility = false;
let browser;
let cdp;
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
async function listen(server, host) {
  servers.push(server);
  server.on("connection", (socket) => {
    sockets.add(socket);
    socket.on("close", () => sockets.delete(socket));
  });
  server.listen(0, host);
  await once(server, "listening");
  return server.address().port;
}

try {
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
      join(dir, "key.pem"),
      "-out",
      join(dir, "cert.pem"),
      "-subj",
      "/CN=127.0.0.2",
      "-addext",
      "subjectAltName=IP:127.0.0.2",
    ],
    { stdio: "ignore" },
  );
  const tls = {
    key: readFileSync(join(dir, "key.pem")),
    cert: readFileSync(join(dir, "cert.pem")),
  };
  const targets = [];
  const entry = http.createServer((_req, res) => {
    res.setHeader("Content-Type", "text/html");
    res.end(
      `<html><body>${targets.map((url) => `<iframe src="${url}" width="450" height="500"></iframe>`).join("")}</body></html>`,
    );
  });
  const entryOrigin = `http://127.0.0.1:${await listen(entry, "127.0.0.1")}`;
  const evidence = {
    entry: "HTTP loopback",
    targets: "HTTPS, same host and different ports",
    thirdPartyCookiesBlocked: true,
  };
  for (const id of ["b", "c"]) {
    const cookieName = `cccc_access_fixture_${id}`;
    const authorized = (req) =>
      (req.headers.cookie || "").split("; ").includes(`${cookieName}=fixture-${id}`);
    const server = https.createServer(tls, async (req, res) => {
      if (id === "c" && (req.headers.cookie || "").includes("cccc_access_fixture_b=fixture-b"))
        sharedHostCookieVisibility = true;
      if (req.url === "/download") downloadRequests.push({ id, authorized: authorized(req) });
      res.setHeader("Content-Security-Policy", `frame-ancestors ${entryOrigin}`);
      res.setHeader("Referrer-Policy", "no-referrer");
      if (req.url === "/") {
        res.setHeader("Content-Type", "text/html");
        res.end(`<html><body><h1>Instance ${id}</h1><input id="token"><button id="login">Login</button><pre id="result"></pre><script>
          window.result = {};
          setInterval(() => document.querySelector('#result').textContent = JSON.stringify(result), 50);
          document.querySelector('#login').onclick = async () => {
            const login = await fetch('/login', {method:'POST', body:document.querySelector('#token').value});
            result.login = login.status;
            result.read = await (await fetch('/read')).text();
            const es = new EventSource('/events');
            es.onmessage = e => { result.sse = e.data; es.close(); };
            es.onerror = () => { result.sse = 'failed'; es.close(); };
            const ws = new WebSocket(${JSON.stringify(`wss://127.0.0.2:PORT/term`)}.replace('PORT', location.port));
            ws.onopen = () => ws.send('typed-input');
            ws.onmessage = e => { result.ws = e.data; ws.close(); };
            ws.onerror = () => result.ws = 'failed';
            const upload = new FormData(); upload.append('file', new Blob(['fixture-upload']), 'fixture.txt');
            result.upload = await (await fetch('/upload', {method:'POST', body:upload})).text();
            const image = new Image(); image.onload = () => result.image = true; image.onerror = () => result.image = false; image.src='/image'; document.body.append(image);
            const a = document.createElement('a'); a.id='download'; a.href='/download'; a.download='${id}.txt'; a.textContent='Download'; document.body.append(a);
          };
        </script></body></html>`);
      } else if (req.url === "/login") {
        const chunks = [];
        for await (const chunk of req) chunks.push(chunk);
        if (Buffer.concat(chunks).toString() !== `fixture-${id}`) {
          res.writeHead(401);
          res.end();
          return;
        }
        res.setHeader(
          "Set-Cookie",
          `${cookieName}=fixture-${id}; Path=/; HttpOnly; SameSite=None; Secure; Partitioned`,
        );
        res.end("ok");
      } else if (!authorized(req)) {
        res.writeHead(401);
        res.end("unauthorized");
      } else if (req.url === "/read") res.end(`read-${id}`);
      else if (req.url === "/upload") {
        for await (const _chunk of req) {
          /* consume the uploaded body */
        }
        res.end(`upload-${id}`);
      } else if (req.url === "/events") {
        res.setHeader("Content-Type", "text/event-stream");
        res.end(`data: events-${id}\n\n`);
      } else if (req.url === "/image") {
        res.setHeader("Content-Type", "image/svg+xml");
        res.end(
          '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="green"/></svg>',
        );
      } else if (req.url === "/download") {
        res.setHeader("Content-Disposition", `attachment; filename="${id}.txt"`);
        res.end(`download-${id}`);
      } else {
        res.writeHead(404);
        res.end();
      }
    });
    const wsServer = new WebSocketServer({ noServer: true });
    server.on("upgrade", (req, socket, head) => {
      if (!authorized(req) || req.headers.origin !== ownOrigin || req.url !== "/term") {
        socket.destroy();
        return;
      }
      wsServer.handleUpgrade(req, socket, head, (ws) =>
        ws.on("message", (input) => ws.send(`terminal-${id}:${input}`)),
      );
    });
    const ownOrigin = `https://127.0.0.2:${await listen(server, "127.0.0.2")}`;
    targets.push(ownOrigin);
  }
  browser = spawn(
    process.env.CHROME_BIN || "/usr/bin/google-chrome",
    [
      "--headless=new",
      "--no-sandbox",
      "--ignore-certificate-errors",
      "--remote-debugging-port=0",
      "--user-data-dir=" + join(dir, "profile"),
      "about:blank",
    ],
    { stdio: "ignore" },
  );
  const portFile = join(dir, "profile/DevToolsActivePort");
  for (let i = 0; !existsSync(portFile) && i < 100; i++) await delay(100);
  const port = readFileSync(portFile, "utf8").split("\n")[0];
  const tab = await (
    await fetch(`http://127.0.0.1:${port}/json/new?about:blank`, { method: "PUT" })
  ).json();
  cdp = new WebSocket(tab.webSocketDebuggerUrl);
  await once(cdp, "open");
  const pending = new Map();
  let sequence = 0;
  cdp.on("message", (raw) => {
    const result = JSON.parse(raw);
    if (!result.id) {
      if (
        result.method?.startsWith("Browser.download") ||
        result.method === "Network.loadingFailed"
      )
        downloadEvents.push(result);
      return;
    }
    const callbacks = pending.get(result.id);
    pending.delete(result.id);
    if (result.error) callbacks.reject(new Error(JSON.stringify(result.error)));
    else callbacks.resolve(result.result);
  });
  const call = (method, params = {}, sessionId) =>
    new Promise((resolve, reject) => {
      const id = ++sequence;
      pending.set(id, { resolve, reject });
      cdp.send(JSON.stringify({ id, method, params, sessionId }));
    });
  await call("Page.enable");
  await call("Runtime.enable");
  await call("Network.enable");
  await call("Network.setCookieControls", {
    enableThirdPartyCookieRestriction: true,
    disableThirdPartyCookieMetadata: true,
    disableThirdPartyCookieHeuristics: true,
  });
  await call("Browser.setDownloadBehavior", {
    behavior: "allow",
    downloadPath: dir,
    eventsEnabled: true,
  });
  await call("Page.navigate", { url: entryOrigin });
  let frames;
  // Cross-site frames are separate Chrome targets. Keep process isolation enabled.
  for (let i = 0; i < 50; i++) {
    const infos = (await call("Target.getTargets")).targetInfos;
    frames = targets
      .map((origin) => infos.find((info) => info.type === "iframe" && info.url === origin + "/"))
      .filter(Boolean);
    if (frames.length === 2) break;
    await delay(100);
  }
  assert.equal(frames?.length, 2);
  const worlds = [];
  for (let i = 0; i < frames.length; i++) {
    const context = await call("Target.attachToTarget", {
      targetId: frames[i].targetId,
      flatten: true,
    });
    worlds.push(context.sessionId);
    await call("Runtime.enable", {}, context.sessionId);
    await call("Network.enable", {}, context.sessionId);
    await call(
      "Network.setCookieControls",
      {
        enableThirdPartyCookieRestriction: true,
        disableThirdPartyCookieMetadata: true,
        disableThirdPartyCookieHeuristics: true,
      },
      context.sessionId,
    );
    const id = ["b", "c"][i];
    await call(
      "Runtime.evaluate",
      {
        expression: `document.querySelector('#token').value='fixture-${id}'; document.querySelector('#login').click()`,
      },
      context.sessionId,
    );
  }
  await delay(1000);
  // Inspect page-world results through DOM and same-origin requests in each isolated frame.
  for (let i = 0; i < worlds.length; i++) {
    const id = ["b", "c"][i];
    const result = await call(
      "Runtime.evaluate",
      {
        awaitPromise: true,
        returnByValue: true,
        expression: `(async () => ({read:await (await fetch('/read')).text(), image:document.querySelector('img').naturalWidth, secure:isSecureContext, operations:JSON.parse(document.querySelector('#result').textContent)}))()`,
      },
      worlds[i],
    );
    assert.deepEqual(result.result.value, {
      read: `read-${id}`,
      image: 10,
      secure: true,
      operations: {
        login: 200,
        read: `read-${id}`,
        sse: `events-${id}`,
        ws: `terminal-${id}:typed-input`,
        upload: `upload-${id}`,
        image: true,
      },
    });
    await call(
      "Runtime.evaluate",
      {
        userGesture: true,
        awaitPromise: true,
        expression: `(async () => {
      const response = await fetch('/download'); if (!response.ok) throw new Error('download denied');
      const href = URL.createObjectURL(await response.blob()); const a=document.querySelector('#download');
      a.href=href; a.click(); setTimeout(()=>URL.revokeObjectURL(href),1000);
    })()`,
      },
      worlds[i],
    );
    for (let attempt = 0; !existsSync(join(dir, `${id}.txt`)) && attempt < 50; attempt++)
      await delay(100);
    if (!existsSync(join(dir, `${id}.txt`)))
      console.warn(JSON.stringify({ downloadRequests, downloadEvents, files: readdirSync(dir) }));
    assert.equal(readFileSync(join(dir, `${id}.txt`), "utf8"), `download-${id}`);
  }
  const cookies = (await call("Storage.getCookies")).cookies.filter((cookie) =>
    cookie.name.startsWith("cccc_access_fixture_"),
  );
  assert.equal(cookies.length, 2);
  assert(cookies.every((cookie) => cookie.partitionKey && cookie.httpOnly && cookie.secure));
  assert(sharedHostCookieVisibility, "Cookies on the same host are visible across ports");
  // A cannot directly inspect either target's document or its credentials.
  const sop = await call("Runtime.evaluate", {
    returnByValue: true,
    expression: `(()=>{try {return document.querySelector('iframe').contentWindow.document.body.innerText} catch {return 'isolated'}})()`,
  });
  assert.equal(sop.result.value, "isolated");
  console.warn(
    JSON.stringify({
      ...evidence,
      sharedHostCookieVisibility,
      partitionedCookies: cookies.length,
      perPortLoginIsolation: true,
      nativeSseAndWebSocket: true,
      httpUploadImageDownload: true,
      downloadViaAuthenticatedFetch: true,
      parentOriginIsolation: true,
    }),
  );
} finally {
  cdp?.close();
  if (browser && browser.exitCode === null) {
    browser.kill("SIGTERM");
    await Promise.race([once(browser, "exit"), delay(3000)]);
    if (browser.exitCode === null) browser.kill("SIGKILL");
  }
  for (const socket of sockets) socket.destroy();
  for (const server of servers) server.close();
  rmSync(dir, { recursive: true, force: true });
}
