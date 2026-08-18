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
  outputFlowControl?: "ack_v1";
  bootstrap?: "snapshot_v1";
  cols?: number;
  rows?: number;
}): string {
  const protocol = args.protocol === "https:" ? "wss:" : "ws:";
  const url = `${protocol}//${args.host}/api/v1/groups/${encodeURIComponent(args.groupId)}/actors/${encodeURIComponent(args.actorId)}/term`;
  const params = new URLSearchParams();
  params.set("mode", args.mode === "viewer" ? "viewer" : "control");
  if (args.takeover) params.set("takeover", "true");
  if (args.outputFlowControl) params.set("output_flow", args.outputFlowControl);
  if (args.bootstrap) params.set("bootstrap", args.bootstrap);
  if (Number.isFinite(args.cols) && Number.isFinite(args.rows)) {
    params.set("cols", String(Math.max(1, Math.floor(args.cols || 0))));
    params.set("rows", String(Math.max(1, Math.floor(args.rows || 0))));
  }
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
export const TERMINAL_FRAME_OUTPUT_ACK = 53; // "5"
export const TERMINAL_FRAME_WRITABLE = 54; // "6"
export const TERMINAL_FRAME_SNAPSHOT = 55; // "7"

const terminalTextEncoder = new TextEncoder();
const terminalTextDecoder = new TextDecoder();
const terminalResponseFilteringRuntimes = new Set(["codex", "devin", "droid"]);
const terminalServerOwnedResponseToken = String.raw`\x1b\[(?:\?|>)(?:\d+)?(?:;\d+)*c|\x1b\[(?:\?)?\d+(?:;\d+)?[nR]|\x1b\](?:10|11);rgb:[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}(?:\x07|\x1b\\)`;
const terminalGeneratedInputSequencePattern = new RegExp(
  `^(?:${terminalServerOwnedResponseToken}|\\x1b\\[[IO])+$`,
);
const terminalServerOwnedResponseGlobalPattern = new RegExp(terminalServerOwnedResponseToken, "g");
const terminalColorReplySequencePattern =
  /\x1b\](?:10|11);rgb:[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}(?:\x07|\x1b\\)/g;
const terminalCursorStatusReplyPattern = /^(?:\x1b\[(?:\?)?\d+(?:;\d+)?[nR])+$/;
const bareTerminalColorReplyPattern =
  /^(?:10|11);rgb:[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}(?:(?:10|11);rgb:[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4})*$/;

export type TerminalBinaryFrame =
  | { type: "input"; payload: Uint8Array }
  | { type: "output"; payload: Uint8Array }
  | { type: "resize"; payload: Uint8Array }
  | { type: "attach"; payload: Uint8Array }
  | { type: "input_ack"; payload: Uint8Array }
  | { type: "output_ack"; payload: Uint8Array }
  | { type: "writable"; payload: Uint8Array }
  | { type: "snapshot"; payload: Uint8Array };

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
    terminalTextEncoder.encode(
      JSON.stringify({ cols: Math.max(0, Math.floor(cols)), rows: Math.max(0, Math.floor(rows)) }),
    ),
  );
}

export function encodeTerminalOutputAckFrame(cursor: number): Uint8Array {
  return buildTerminalFrame(
    TERMINAL_FRAME_OUTPUT_ACK,
    terminalTextEncoder.encode(JSON.stringify({ cursor: Math.max(0, Math.floor(cursor)) })),
  );
}

export function splitTerminalOutputByReplayBoundary(
  data: Uint8Array,
  startCursor: number | null,
  replayEndCursor: number | null,
): Array<{ data: Uint8Array; replaying: boolean }> {
  if (data.byteLength === 0) return [];
  if (
    startCursor === null ||
    replayEndCursor === null ||
    !Number.isFinite(startCursor) ||
    !Number.isFinite(replayEndCursor)
  ) {
    return [{ data, replaying: false }];
  }

  const replayBytes = Math.max(
    0,
    Math.min(data.byteLength, Math.floor(replayEndCursor - startCursor)),
  );
  if (replayBytes === 0) return [{ data, replaying: false }];
  if (replayBytes === data.byteLength) return [{ data, replaying: true }];
  return [
    { data: data.subarray(0, replayBytes), replaying: true },
    { data: data.subarray(replayBytes), replaying: false },
  ];
}

export function filterTerminalInputForRuntime(
  data: string,
  runtime: string | null | undefined,
  options?: { replaying?: boolean; serverResponses?: boolean },
): string {
  const text = String(data || "");
  if (!text) return text;
  const normalizedRuntime = String(runtime || "")
    .trim()
    .toLowerCase();
  const generatedInput = terminalGeneratedInputSequencePattern.test(text);
  const bareColorReply = bareTerminalColorReplyPattern.test(text);

  // Replaying retained output must be side-effect free. Otherwise an old color
  // query is answered again after reconnect and the late reply can become
  // literal prompt text in the runtime.
  if (options?.replaying && (generatedInput || bareColorReply)) return "";
  if (
    bareColorReply &&
    (options?.serverResponses || terminalResponseFilteringRuntimes.has(normalizedRuntime))
  ) {
    return "";
  }
  if (options?.serverResponses) {
    return text.replace(terminalServerOwnedResponseGlobalPattern, "");
  }
  if (!terminalResponseFilteringRuntimes.has(normalizedRuntime)) return text;
  if (!generatedInput) return text;

  // New Rust PTYs own DA/DSR/CPR and OSC 10/11. Older backends do not declare
  // this capability and still need xterm's live color replies.
  if (terminalCursorStatusReplyPattern.test(text)) return text;
  return text.match(terminalColorReplySequencePattern)?.join("") || "";
}

export function decodeTerminalJsonFrame<T = Record<string, unknown>>(
  payload: Uint8Array,
): T | null {
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
    case TERMINAL_FRAME_OUTPUT_ACK:
      return { type: "output_ack", payload };
    case TERMINAL_FRAME_WRITABLE:
      return { type: "writable", payload };
    case TERMINAL_FRAME_SNAPSHOT:
      return { type: "snapshot", payload };
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

export function shouldRetryTerminalClose(args: {
  actorRunning: boolean;
  isHeadless: boolean;
  attachNonRetryable: boolean;
  closeCode: number;
}): boolean {
  return (
    args.actorRunning && !args.isHeadless && !args.attachNonRetryable && args.closeCode !== 4401
  );
}
