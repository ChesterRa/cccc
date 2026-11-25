import type { FitAddon } from "@xterm/addon-fit";

export interface TerminalSize {
  cols: number;
  rows: number;
}

interface SizedTerminal {
  readonly cols: number;
  readonly rows: number;
  resize: (cols: number, rows: number) => void;
}

/**
 * Sizes a terminal for its window. The writer fits its container and sizes the PTY
 * from it. Any other window shows the PTY at the writer's size (`followSize`):
 * full-screen apps draw for that size and send only changed cells, so a window
 * of any other size breaks their rows apart.
 */
export function fitTerminalViewport(args: {
  fitAddon: FitAddon | null;
  container: HTMLElement | null;
  terminal: SizedTerminal | null;
  followSize: TerminalSize | null;
}): boolean {
  const { fitAddon, container, terminal, followSize } = args;
  if (followSize && terminal) {
    if (terminal.cols !== followSize.cols || terminal.rows !== followSize.rows) {
      terminal.resize(followSize.cols, followSize.rows);
    }
    return true;
  }
  if (!fitAddon || !container) return false;
  if (container.clientWidth <= 50 || container.clientHeight <= 50) return false;
  try {
    fitAddon.fit();
    return true;
  } catch {
    return false;
  }
}

/**
 * Applies the PTY size the server reports. A window that does not write follows it;
 * the writer keeps fitting its own container. Returns true when a following
 * terminal had to change size, so rows already on screen were drawn at the old one.
 */
export function followServerTerminalSize(args: {
  terminal: SizedTerminal | null;
  followSizeRef: { current: TerminalSize | null };
  size: TerminalSize;
  follows: boolean;
}): boolean {
  const { terminal, followSizeRef, size, follows } = args;
  if (!follows) return false;
  followSizeRef.current = { cols: size.cols, rows: size.rows };
  if (!terminal || (terminal.cols === size.cols && terminal.rows === size.rows)) return false;
  terminal.resize(size.cols, size.rows);
  return true;
}

export function parseTerminalSize(value: unknown): TerminalSize | null {
  if (!value || typeof value !== "object") return null;
  const { cols, rows } = value as Record<string, unknown>;
  if (typeof cols !== "number" || typeof rows !== "number") return null;
  if (!Number.isInteger(cols) || !Number.isInteger(rows)) return null;
  if (cols < 10 || rows < 2 || cols > 4096 || rows > 4096) return null;
  return { cols, rows };
}
