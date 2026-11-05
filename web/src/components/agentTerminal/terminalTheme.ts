import type { ITheme } from "@xterm/xterm";

/**
 * Interactive terminal apps commonly render their own ANSI background blocks.
 * Keep the PTY canvas dark so those blocks and the surrounding xterm surface
 * always use the same color model, regardless of the dashboard theme. Its ground is
 * a neutral dark, a step deeper than the page in dark mode, so ANSI black stays legible.
 */
export function getTerminalTheme(): ITheme {
  return {
    background: "#181715",
    foreground: "#e2e8f0",
    cursor: "#e2e8f0",
    cursorAccent: "#181715",
    selectionBackground: "#3a3936",
    black: "#64748b",
    red: "#f87171",
    green: "#4ade80",
    yellow: "#facc15",
    blue: "#60a5fa",
    magenta: "#c084fc",
    cyan: "#22d3ee",
    white: "#f1f5f9",
    brightBlack: "#94a3b8",
    brightRed: "#fca5a5",
    brightGreen: "#86efac",
    brightYellow: "#fde047",
    brightBlue: "#93c5fd",
    brightMagenta: "#d8b4fe",
    brightCyan: "#67e8f9",
    brightWhite: "#f8fafc",
  };
}
