// The caller checks the private profile, browser PID and target before invoking this probe.
const [port, target] = process.argv.slice(2);
if (!/^\d{1,5}$/.test(port) || !/^[a-f0-9]+$/i.test(target)) {
    throw new Error("Invalid fixture target");
}
const socket = new WebSocket(`ws://127.0.0.1:${port}/devtools/page/${target}`);
const deadline = setTimeout(() => process.exit(1), 2000);
socket.addEventListener("error", () => process.exit(1));
socket.addEventListener("open", () => {
    socket.send(JSON.stringify({
        id: 1,
        method: "Runtime.evaluate",
        params: {
            expression: `({
                url: document.URL,
                title: document.title,
                state: document.readyState,
                chromeError: document.querySelector("#main-frame-error .error-code")?.textContent?.trim(),
                composerPresent: !!document.querySelector("textarea#prompt-textarea"),
                formHandlerInstalled: typeof document.querySelector("main form")?.onsubmit === "function",
                browser: navigator.userAgent
            })`,
            returnByValue: true
        }
    }));
});
socket.addEventListener("message", event => {
    const reply = JSON.parse(event.data);
    if (reply.id !== 1) return;
    const state = reply.result?.result?.value;
    if (!state || reply.result.exceptionDetails) process.exit(1);
    console.log(JSON.stringify(state));
    clearTimeout(deadline);
    socket.close();
});
