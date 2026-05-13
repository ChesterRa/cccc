import type { TerminalSignal } from "../stores/useTerminalSignalsStore";
import { getEffectiveActorRunner } from "./headlessRuntimeSupport";

const MAX_TERMINAL_BUFFER_CHARS = 4000;
const CODEX_TERMINAL_SIGNAL_WINDOW_CHARS = 1600;
const WORKING_OUTPUT_TTL_MS = 5000;
const IDLE_PROMPT_TTL_MS = 3000;
const ESC = String.fromCharCode(27);
const BEL = String.fromCharCode(7);
const ANSI_ESCAPE_RE = new RegExp(
  `${ESC}(?:[@-Z\\\\-_]|\\[[0-?]*[ -/]*[@-~]|\\][^${BEL}]*(?:${BEL}|${ESC}\\\\))`,
  "g",
);

function stripAnsi(text: string): string {
  return text
    .replace(ANSI_ESCAPE_RE, "")
    .replace(/\r/g, "");
}

function stripControlChars(text: string): string {
  let out = "";
  for (const ch of String(text || "")) {
    const code = ch.charCodeAt(0);
    if ((code >= 0 && code <= 8) || (code >= 11 && code <= 31) || code === 127) {
      continue;
    }
    out += ch;
  }
  return out;
}

export function appendTerminalSignalBuffer(previous: string, chunk: string): string {
  const merged = `${previous || ""}${stripAnsi(String(chunk || ""))}`;
  if (merged.length <= MAX_TERMINAL_BUFFER_CHARS) return merged;
  return merged.slice(-MAX_TERMINAL_BUFFER_CHARS);
}

function getRecentNonEmptyLines(text: string, maxLines: number = 4): string[] {
  const result: string[] = [];
  const lines = String(text || "").split("\n");
  for (let index = lines.length - 1; index >= 0; index -= 1) {
    const line = lines[index]?.trim() || "";
    if (!line) continue;
    result.push(line);
    if (result.length >= maxLines) break;
  }
  return result;
}

export function isTerminalPromptVisible(buffer: string): boolean {
  const lines = getRecentNonEmptyLines(buffer);
  for (const line of lines) {
    if (/^(?:>|›)\s?.*/.test(line)) return true;
    if (/^(?:\$|%|#|❯|➜|›)\s+.*$/.test(line)) return true;
    if (/^[\w.@:/~-]+\s*(?:\$|%|#)\s*$/.test(line)) return true;
    return false;
  }
  return false;
}

export function isCodexWorkingBannerVisible(buffer: string): boolean {
  return /(?:^|\n)\s*[◦·•]\s+Working\s*\([^)\n]*esc to interrupt[^)\n]*\)/i.test(String(buffer || ""));
}

function tailWindowHasCodexWorkingBanner(text: string): boolean {
  const compact = String(text || "").replace(/\s+/g, " ");
  return /\bworking\s*\(/i.test(compact);
}

function getTailWindow(text: string, maxChars: number = CODEX_TERMINAL_SIGNAL_WINDOW_CHARS): string {
  const value = String(text || "");
  if (maxChars <= 0 || value.length <= maxChars) return value;
  return value.slice(-maxChars);
}

export function hasVisibleTerminalOutput(chunk: string): boolean {
  const cleaned = stripControlChars(stripAnsi(String(chunk || ""))).trim();
  return cleaned.length > 0;
}

export function getTerminalSignalFromChunk(
  previousBuffer: string,
  chunk: string,
  runtime: string = "",
): {
  nextBuffer: string;
  signalKind: TerminalSignal["kind"] | null;
} {
  const nextBuffer = appendTerminalSignalBuffer(previousBuffer, chunk);
  const runtimeId = String(runtime || "").trim().toLowerCase();
  if (runtimeId === "codex") {
    if (isTerminalPromptVisible(nextBuffer)) {
      return { nextBuffer, signalKind: "idle_prompt" };
    }
    const tailWindow = getTailWindow(nextBuffer);
    if (tailWindowHasCodexWorkingBanner(tailWindow)) {
      return { nextBuffer, signalKind: "working_output" };
    }
    return { nextBuffer, signalKind: null };
  }
  if (isTerminalPromptVisible(nextBuffer)) {
    return { nextBuffer, signalKind: "idle_prompt" };
  }
  if (hasVisibleTerminalOutput(chunk)) {
    return { nextBuffer, signalKind: "working_output" };
  }
  return { nextBuffer, signalKind: null };
}

export type ActorWorkingStateInput = {
  running?: boolean;
  enabled?: boolean;
  runner?: string;
  runner_effective?: string;
  effective_working_state?: string;
};

export function getActorDisplayWorkingState(
  actor: ActorWorkingStateInput,
  signal: TerminalSignal | null | undefined,
  now: number = Date.now(),
): string {
  const backendState = String(actor.effective_working_state || "").trim().toLowerCase() || "idle";
  const effectiveRunner = getEffectiveActorRunner(actor);
  const isRunning = actor.running ?? actor.enabled ?? false;

  if (!isRunning || effectiveRunner === "headless") {
    return backendState;
  }

  if (signal?.kind === "idle_prompt" && now - signal.updatedAt <= IDLE_PROMPT_TTL_MS) {
    return "idle";
  }

  if (signal?.kind === "working_output" && now - signal.updatedAt <= WORKING_OUTPUT_TTL_MS) {
    return "working";
  }

  return backendState;
}
