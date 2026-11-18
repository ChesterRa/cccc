// Only after the acceptance verdict is fixed. The Python caller verifies the target belongs to this fixture.
const [port, target, diagnostic] = process.argv.slice(2);
const url = new URL(diagnostic);
if (!/^\d{1,5}$/.test(port) || !/^[a-f0-9]+$/i.test(target) ||
    url.protocol !== "http:" || url.hostname !== "127.0.0.1" || url.pathname !== "/ci-navigation-check") throw Error("Invalid fixture target");
const state = {};
const socket = new WebSocket(`ws://127.0.0.1:${port}/devtools/page/${target}`);
let pending;
let nextId = 0;
function call(method, params = {}) {
    const id = ++nextId;
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {pending = null; reject(Error(method + " timeout"));}, 2000);
        pending = reply => {
            if (reply.id !== id) return;
            clearTimeout(timer);
            pending = null;
            if (reply.error) reject(Error(method + ": " + JSON.stringify(reply.error)));
            else resolve(reply.result);
        };
        socket.send(JSON.stringify({id, method, params}));
    });
}
async function observe(name, fn) {
    const start = Date.now();
    try { state[name] = {value: await fn(), ms: Date.now()-start}; }
    catch (error) { state[name] = {error: error.message, ms: Date.now()-start}; }
}
try {
    await new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(Error("WebSocket open timeout")), 2000);
        socket.addEventListener("open", () => {clearTimeout(timer); resolve();}, {once:true});
        socket.addEventListener("error", () => {clearTimeout(timer); reject(Error("WebSocket handshake failed"));}, {once:true});
    });
    socket.addEventListener("message", ev => pending?.(JSON.parse(ev.data)));
    await observe("frameBefore", async () => {
        const {frame} = (await call("Page.getFrameTree")).frameTree;
        return {url:frame.url, unreachable:frame.unreachableUrl};
    });
    await observe("document", async () => {
        const x = await call("Runtime.evaluate", {expression:`({url:document.URL,title:document.title,
            ready:document.readyState,chromeError:document.querySelector("#main-frame-error .error-code")?.textContent?.trim(),
            composerPresent:!!document.querySelector("textarea#prompt-textarea"),
            formHandlerInstalled:typeof document.querySelector("main form")?.onsubmit==="function"})`, returnByValue:true});
        if (x.exceptionDetails) throw Error(JSON.stringify(x.exceptionDetails));
        return x.result.value;
    });
    await observe("nativeNavigation", () => call("Page.navigate", {url: url.href}));
} catch (error) { state.error = error.message; }
finally {
    console.log(JSON.stringify(state));
    if (socket.readyState === WebSocket.OPEN) socket.close();
}
