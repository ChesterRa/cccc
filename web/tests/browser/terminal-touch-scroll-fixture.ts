// Real xterm and production touch adapter. No daemon connection or mocked mouse modes.
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { attachTerminalTouchScroll } from "../../src/components/agentTerminal/terminalTouchScroll";

type MouseMode = Terminal["modes"]["mouseTrackingMode"];
const term = new Terminal({
  rows: 12,
  cols: 44,
  scrollback: 500,
  fontSize: 14,
  scrollSensitivity: 1,
});
const host = document.getElementById("terminal")!;
term.open(host);
attachTerminalTouchScroll(term);
const input: string[] = [];
const wheels: number[] = [];
term.onData((data) => input.push(data));
term.element!.addEventListener("wheel", (event) => wheels.push(event.deltaY));
const write = (text: string) => new Promise<void>((resolve) => term.write(text, resolve));

async function configure(mode: MouseMode, alternate: boolean) {
  term.reset();
  await write(Array.from({ length: 80 }, (_, index) => `line ${index}`).join("\r\n"));
  const modes = {
    none: "",
    x10: "\x1b[?9h",
    vt200: "\x1b[?1000h",
    drag: "\x1b[?1002h",
    any: "\x1b[?1003h",
  };
  await write(`\x1b[?1006h${modes[mode]}${alternate ? "\x1b[?1049h\x1b[?1h" : ""}`);
  term.scrollToBottom();
  input.length = 0;
  wheels.length = 0;
}

function snapshot() {
  const rect = term.element!.querySelector(".xterm-screen")!.getBoundingClientRect();
  return {
    mode: term.modes.mouseTrackingMode,
    buffer: term.buffer.active.type,
    viewport: term.buffer.active.viewportY,
    base: term.buffer.active.baseY,
    input: [...input],
    wheels: [...wheels],
    point: { x: rect.x + rect.width / 2, y: rect.y + rect.height / 3 },
    cellHeight: rect.height / term.rows,
  };
}

Object.assign(window, { touchScrollFixture: { configure, snapshot } });
await configure("x10", false);
host.dataset.ready = "true";
