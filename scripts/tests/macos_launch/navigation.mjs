// Run against a newly spawned fixture browser whose profile, process and port the caller checked.
import {writeFileSync} from "node:fs";
import {setTimeout as delay} from "node:timers/promises";
const [port, pid, origin, output] = process.argv.slice(2);
const local = new URL(origin);
if (local.origin !== origin || local.hostname !== "127.0.0.1" || local.protocol !== "http:") throw Error("Expected loopback HTTP fixture");
const report = {steps: [], observations: [], cases: {}};
const begin = Date.now();
const record = (kind, detail) => report.steps.push({ms: Date.now() - begin, kind, detail});
const save = () => writeFileSync(output, JSON.stringify(report, null, 2));
const logError = error => error instanceof Error ? error.message : String(error);
let socket;
const pending = new Map();
let seq = 0;
function call(method, params = {}, sessionId) {
    const id = ++seq;
    record("request", {id, method});
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
            pending.delete(id);
            reject(Error(method + " timed out"));
        }, 2500);
        pending.set(id, reply => {
            clearTimeout(timer);
            record("response", {id, method, error: reply.error});
            if (reply.error) reject(Error(method + ": " + JSON.stringify(reply.error)));
            else resolve(reply.result);
        });
        socket.send(JSON.stringify({id, method, params, sessionId}));
    });
}
async function evaluate(session, expression) {
    const result = await call("Runtime.evaluate", {expression, returnByValue: true}, session);
    if (result.exceptionDetails) throw Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
}
async function runCase(name, method, background, initial) {
    const result = report.cases[name] = {};
    try {
        const {targetId} = await call("Target.createTarget", {url: "about:blank", newWindow: true, background});
        result.targetId = targetId;
        const {sessionId} = await call("Target.attachToTarget", {targetId, flatten: true});
        for (const t of initial) {
            if (t.type === "page" && ["", "about:blank", "chrome://newtab/", "chrome://new-tab-page/"].includes(t.url))
                await call("Target.closeTarget", {targetId: t.targetId});
        }
        await call("Page.enable", {}, sessionId);
        await call("Network.enable", {}, sessionId);
        result.viewport = await evaluate(sessionId, "({width:window.innerWidth,height:window.innerHeight})");
        const url = origin + "/document/" + name;
        const destination = method === "data" ? "data:text/html,<title>renderer-control</title>" : url;
        if (method === "assign") {
            await evaluate(sessionId, "globalThis.__cccc_browser_surface_navigation_pending_v1=true; window.location.assign(" + JSON.stringify(url) + ")");
        } else {
            const reply = await call("Page.navigate", {url: destination}, sessionId);
            result.navigateReply = reply;
            if (reply.errorText) throw Error(reply.errorText);
        }
        const deadline = Date.now() + 5000;
        do {
            try {
                result.document = await evaluate(sessionId, "({url: document.URL, title: document.title, ready: document.readyState})");
            } catch (error) {
                if (!/context.*destroyed|Cannot find context/i.test(logError(error))) throw error;
                await delay(100);
                continue;
            }
            if (method === "data") {
                if (result.document.title === "renderer-control") { result.ready = true; break; }
            } else if (result.document.url === url && result.document.title === name) {
                const res = await fetch(origin + "/observed/" + name, {signal: AbortSignal.timeout(1000)});
                if (res.ok && (await res.json()).script) { result.ready = true; break; }
            }
            await delay(100);
        } while (Date.now() < deadline);
    } catch (error) { result.error = logError(error); }
    try {
        result.browserResponsive = (await call("Browser.getVersion")).product;
        if (result.targetId) result.finalUrl = (await call("Target.getTargetInfo", {targetId: result.targetId})).targetInfo.url;
    } catch (error) { result.targetReadError = logError(error); }
    record(name, result.ready ? "ready" : "unconfirmed");
    save();
}
try {
    const response = await fetch("http://127.0.0.1:" + port + "/json/version", {redirect: "error", signal: AbortSignal.timeout(2000)});
    const info = await response.json();
    const endpoint = new URL(info.webSocketDebuggerUrl);
    if (endpoint.protocol !== "ws:" || !["localhost","127.0.0.1"].includes(endpoint.hostname) || endpoint.port !== port ||
        endpoint.username || endpoint.password || !endpoint.pathname.startsWith("/devtools/browser/")) throw Error("Unexpected fixture CDP endpoint");
    endpoint.hostname = "127.0.0.1";
    socket = new WebSocket(endpoint);
    await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(Error("WebSocket open timed out")), 2000);
        socket.addEventListener("open", () => { clearTimeout(timer); resolve(); }, {once: true});
        socket.addEventListener("error", () => { clearTimeout(timer); reject(Error("WebSocket open failed")); }, {once: true});
    });
    socket.addEventListener("message", event => {
        const reply = JSON.parse(event.data);
        if (reply.id) {
            pending.get(reply.id)?.(reply);
            pending.delete(reply.id);
        } else if (report.observations.length < 64 && ["Page.frameNavigated","Network.loadingFailed","Inspector.targetCrashed","Target.targetCrashed"].includes(reply.method)) {
            const e = reply.params;
            report.observations.push({ms: Date.now()-begin, session:reply.sessionId, method:reply.method, url:e.frame?.url, unreachable:e.frame?.unreachableUrl, error:e.errorText, status:e.status});
        }
    });
    const processes = (await call("SystemInfo.getProcessInfo")).processInfo;
    if (processes.find(p => p.type === "browser")?.id !== Number(pid)) throw Error("Fixture browser PID mismatch");
    report.browser = info.Browser;
    const initial = (await call("Target.getTargets")).targetInfos;
    await runCase("js-background", "assign", true, initial);
    await runCase("native-background", "navigate", true, []);
    await runCase("inline-data", "data", true, []);
    if (!report.cases["js-background"].ready) await runCase("js-foreground", "assign", false, []);
} catch (error) {
    report.error = logError(error);
    process.exitCode = 1;
} finally {
    save();
    if (socket?.readyState === WebSocket.OPEN) socket.close();
}
