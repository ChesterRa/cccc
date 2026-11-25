// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vite-plus/test";
import type { FitAddon } from "@xterm/addon-fit";

import { fitTerminalViewport, followServerTerminalSize } from "./terminalViewportSize";

function terminal(cols: number, rows: number) {
  const term = {
    cols,
    rows,
    resize: vi.fn((nextCols: number, nextRows: number) => {
      term.cols = nextCols;
      term.rows = nextRows;
    }),
  };
  return term;
}

function container(width: number, height: number) {
  const element = document.createElement("div");
  Object.defineProperty(element, "clientWidth", { value: width });
  Object.defineProperty(element, "clientHeight", { value: height });
  return element;
}

describe("terminal viewport size", () => {
  it("fits the writer's terminal to its container", () => {
    const fitAddon = { fit: vi.fn() } as unknown as FitAddon;
    const term = terminal(80, 24);
    expect(
      fitTerminalViewport({
        fitAddon,
        container: container(900, 600),
        terminal: term,
        followSize: null,
      }),
    ).toBe(true);
    expect(fitAddon.fit).toHaveBeenCalledTimes(1);
    expect(term.resize).not.toHaveBeenCalled();
  });

  it("shows any other window at the PTY's size whatever its container measures", () => {
    const fitAddon = { fit: vi.fn() } as unknown as FitAddon;
    const term = terminal(68, 22);
    const followSize = { cols: 170, rows: 46 };
    fitTerminalViewport({ fitAddon, container: container(600, 400), terminal: term, followSize });
    fitTerminalViewport({ fitAddon, container: container(600, 400), terminal: term, followSize });
    expect(fitAddon.fit).not.toHaveBeenCalled();
    expect(term.resize).toHaveBeenCalledTimes(1);
    expect([term.cols, term.rows]).toEqual([170, 46]);
  });

  it("follows a reported size only for a window that does not write", () => {
    const followSizeRef = { current: null as { cols: number; rows: number } | null };
    const writer = terminal(120, 40);
    expect(
      followServerTerminalSize({
        terminal: writer,
        followSizeRef,
        size: { cols: 90, rows: 30 },
        follows: false,
      }),
    ).toBe(false);
    expect(writer.resize).not.toHaveBeenCalled();
    expect(followSizeRef.current).toBeNull();

    const viewer = terminal(68, 22);
    const size = { cols: 145, rows: 42 };
    expect(followServerTerminalSize({ terminal: viewer, followSizeRef, size, follows: true })).toBe(
      true,
    );
    expect([viewer.cols, viewer.rows]).toEqual([145, 42]);
    expect(followSizeRef.current).toEqual(size);
    // The same size again changes nothing, so it never triggers another redraw.
    expect(followServerTerminalSize({ terminal: viewer, followSizeRef, size, follows: true })).toBe(
      false,
    );
  });
});
