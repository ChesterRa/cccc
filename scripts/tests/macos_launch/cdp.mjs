// The caller checks the private profile PID owns this listener before starting us.
const port = process.argv[2];
const base = `http://127.0.0.1:${port}`;
const timer = setTimeout(() => process.exit(1), 1500);
try {
    const response = await fetch(base + "/json/version", { redirect: "error" });
    if (!response.ok) throw new Error("CDP version unavailable");
    const version = await response.json();
    const ws = new URL(version.webSocketDebuggerUrl);
    if (ws.protocol !== "ws:" || !["127.0.0.1", "localhost"].includes(ws.hostname) ||
        ws.port !== port || ws.username || ws.password || !ws.pathname.startsWith("/devtools/browser/")) {
        throw new Error("Unexpected CDP endpoint");
    }
    ws.hostname = "127.0.0.1";
    const socket = new WebSocket(ws);
    socket.addEventListener("error", () => process.exit(1));
    socket.addEventListener("open", () => {
        socket.send(JSON.stringify({ id: 1, method: "SystemInfo.getProcessInfo" }));
    });
    socket.addEventListener("message", event => {
        const reply = JSON.parse(event.data);
        if (reply.id !== 1) return;
        const pid = reply.result?.processInfo?.find(p => p.type === "browser")?.id;
        if (!Number.isSafeInteger(pid) || pid <= 1) process.exit(1);
        console.log(JSON.stringify({ pid, version: version.Browser }));
        clearTimeout(timer);
        socket.close();
    });
} catch {
    process.exit(1);
}
