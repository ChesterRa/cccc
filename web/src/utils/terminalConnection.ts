/* eslint-disable no-control-regex */

export function buildTerminalConnectionKey(args: {
  activated: boolean;
  isRunning: boolean;
  isHeadless: boolean;
  groupId: string;
  actorId: string;
  reconnectTrigger: number;
  canControl: boolean;
}): string {
  return [
    args.activated ? "active" : "inactive",
    args.isRunning ? "running" : "stopped",
    args.isHeadless ? "headless" : "pty",
    String(args.groupId || "").trim(),
    String(args.actorId || "").trim(),
    String(args.reconnectTrigger || 0),
    args.canControl ? "control" : "readonly",
  ].join(":");
}

export function buildTerminalWebSocketUrl(args: {
  protocol: string;
  host: string;
  groupId: string;
  actorId: string;
  since?: number | string | null;
  mode?: "control" | "viewer";
  takeover?: boolean;
}): string {
  const protocol = args.protocol === "https:" ? "wss:" : "ws:";
  const url = `${protocol}//${args.host}/api/v1/groups/${encodeURIComponent(args.groupId)}/actors/${encodeURIComponent(args.actorId)}/term`;
  const params = new URLSearchParams();
  params.set("mode", args.mode === "viewer" ? "viewer" : "control");
  if (args.takeover) params.set("takeover", "true");
  const since = args.since;
  if (since !== null && since !== undefined && String(since).trim()) {
    params.set("since", String(since));
  }
  return `${url}?${params.toString()}`;
}

export const TERMINAL_FRAME_INPUT = 48; // "0"
export const TERMINAL_FRAME_OUTPUT = 49; // "1"
export const TERMINAL_FRAME_RESIZE = 50; // "2"
export const TERMINAL_FRAME_ATTACH = 51; // "3"
export const TERMINAL_FRAME_INPUT_ACK = 52; // "4"

const terminalTextEncoder = new TextEncoder();
const terminalTextDecoder = new TextDecoder();
const terminalResponseSuppressionRuntimes = new Set(["codex", "devin", "droid"]);
const terminalGeneratedInputSequencePattern = /^(?:\x1b\[(?:\?|>)(?:\d+)?(?:;\d+)*c|\x1b\](?:10|11);rgb:[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}(?:\x07|\x1b\\)|\x1b\[[IO])+$/;
const bareTerminalColorReplyPattern = /^(?:10|11);rgb:[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}(?:(?:10|11);rgb:[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4})*$/;

export type TerminalBinaryFrame =
  | { type: "input"; payload: Uint8Array }
  | { type: "output"; payload: Uint8Array }
  | { type: "resize"; payload: Uint8Array }
  | { type: "attach"; payload: Uint8Array }
  | { type: "input_ack"; payload: Uint8Array };

function buildTerminalFrame(opcode: number, payload?: Uint8Array): Uint8Array {
  const body = payload || new Uint8Array();
  const out = new Uint8Array(body.length + 1);
  out[0] = opcode;
  out.set(body, 1);
  return out;
}

export function encodeTerminalInputFrame(data: string): Uint8Array {
  return buildTerminalFrame(TERMINAL_FRAME_INPUT, terminalTextEncoder.encode(String(data || "")));
}

export function encodeTerminalResizeFrame(cols: number, rows: number): Uint8Array {
  return buildTerminalFrame(
    TERMINAL_FRAME_RESIZE,
    terminalTextEncoder.encode(JSON.stringify({ cols: Math.max(0, Math.floor(cols)), rows: Math.max(0, Math.floor(rows)) })),
  );
}

export function shouldSuppressTerminalGeneratedInput(data: string, runtime: string | null | undefined): boolean {
  const normalizedRuntime = String(runtime || "").trim().toLowerCase();
  if (!terminalResponseSuppressionRuntimes.has(normalizedRuntime)) return false;
  const text = String(data || "");
  if (!text) return false;
  return terminalGeneratedInputSequencePattern.test(text) || bareTerminalColorReplyPattern.test(text);
}

export function decodeTerminalJsonFrame<T = Record<string, unknown>>(payload: Uint8Array): T | null {
  try {
    return JSON.parse(terminalTextDecoder.decode(payload)) as T;
  } catch {
    return null;
  }
}

export function parseTerminalBinaryFrame(data: ArrayBuffer): TerminalBinaryFrame | null {
  const bytes = new Uint8Array(data);
  if (bytes.length <= 0) return null;
  const payload = bytes.slice(1);
  switch (bytes[0]) {
    case TERMINAL_FRAME_INPUT:
      return { type: "input", payload };
    case TERMINAL_FRAME_OUTPUT:
      return { type: "output", payload };
    case TERMINAL_FRAME_RESIZE:
      return { type: "resize", payload };
    case TERMINAL_FRAME_ATTACH:
      return { type: "attach", payload };
    case TERMINAL_FRAME_INPUT_ACK:
      return { type: "input_ack", payload };
    default:
      return null;
  }
}

export function isTerminalAttachNonRetryableErrorCode(code: unknown): boolean {
  const normalized = String(code || "").trim();
  return [
    "actor_not_found",
    "auth_required",
    "group_not_found",
    "not_pty_actor",
    "permission_denied",
    "read_only_terminal",
  ].includes(normalized);
}

export function isTerminalAttachStartupRaceErrorCode(code: unknown): boolean {
  const normalized = String(code || "").trim();
  return normalized === "actor_not_running";
}

export function shouldSuppressTerminalAttachErrorOutput(code: unknown): boolean {
  const normalized = String(code || "").trim();
  return normalized === "actor_not_running" || normalized === "not_pty_actor";
}
