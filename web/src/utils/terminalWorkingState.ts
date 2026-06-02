import type { TerminalSignal } from "../stores/useTerminalSignalsStore";
import { getEffectiveActorRunner } from "./headlessRuntimeSupport";

const MAX_TERMINAL_BUFFER_CHARS = 4000;
const CODEX_TERMINAL_SIGNAL_WINDOW_CHARS = 1600;
export const WORKING_OUTPUT_TTL_MS = 5000;
export const IDLE_PROMPT_TTL_MS = 3000;
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
  }
  return false;
}

export function isCodexWorkingBannerVisible(buffer: string): boolean {
  return /(?:^|\n)\s*[◦·•]\s+Working\s*\([^)\n]*esc to interrupt[^)\n]*\)/i.test(String(buffer || ""));
}

export function stripTerminalWorkingBanners(text: string): string {
  const stripped = String(text || "")
    .split(/\r\n|\r|\n/)
    .filter((line) => !isTerminalWorkingBannerLine(line))
    .join("\n");
  return stripped.trim() ? stripped : "";
}

export function stripInactiveTerminalWorkingBanners(text: string, _workingState?: string): string {
  return stripTerminalWorkingBanners(text);
}

function isTerminalWorkingBannerLine(line: string): boolean {
  return /^\s*[◦·•]\s*(?:�\s*)?Working\b.*$/i.test(stripControlChars(stripAnsi(String(line || ""))));
}

function isTerminalWorkingBannerPrefix(line: string): boolean {
  const value = stripControlChars(stripAnsi(String(line || "")));
  if (isTerminalWorkingBannerLine(value)) return true;
  return /^\s*[◦·•]\s*(?:�\s*)?(?:W(?:o(?:r(?:k(?:i(?:n(?:g.*)?)?)?)?)?)?)?$/i.test(value);
}

export function filterTerminalWorkingBannerChunk(
  previousTail: string,
  chunk: string,
): {
  visible: string;
  nextTail: string;
} {
  const combined = `${previousTail || ""}${chunk || ""}`;
  if (!combined) return { visible: "", nextTail: "" };

  let visible = "";
  let lineStart = 0;
  const consumeLine = (end: number, includeSeparator: number) => {
    const line = combined.slice(lineStart, end);
    if (!isTerminalWorkingBannerLine(line)) {
      visible += combined.slice(lineStart, end + includeSeparator);
    }
    lineStart = end + includeSeparator;
  };

  for (let index = 0; index < combined.length; index += 1) {
    const ch = combined[index];
    if (ch === "\r") {
      const separatorLength = combined[index + 1] === "\n" ? 2 : 1;
      consumeLine(index, separatorLength);
      if (separatorLength === 2) index += 1;
    } else if (ch === "\n") {
      consumeLine(index, 1);
    }
  }

  const tail = combined.slice(lineStart);
  if (!tail) return { visible, nextTail: "" };
  if (isTerminalWorkingBannerPrefix(tail)) {
    return { visible, nextTail: tail };
  }
  return { visible: `${visible}${tail}`, nextTail: "" };
}

function lastTerminalPromptOffset(text: string): number {
  let offset = 0;
  let lastOffset = -1;
  for (const rawLine of String(text || "").split(/(\n)/)) {
    if (rawLine === "\n") {
      offset += rawLine.length;
      continue;
    }
    const line = rawLine.trim();
    if (/^(?:>|›)\s+\S.*$/.test(line) || /^(?:\$|%|#|❯|➜|›)\s+.*$/.test(line) || /^[\w.@:/~-]+\s*(?:\$|%|#)\s*$/.test(line)) {
      lastOffset = offset;
    }
    offset += rawLine.length;
  }
  return lastOffset;
}

function lastCodexWorkingBannerOffset(text: string): number {
  let lastOffset = -1;
  const re = /\bworking\s*\(/gi;
  let match: RegExpExecArray | null;
  while ((match = re.exec(String(text || ""))) !== null) {
    lastOffset = match.index;
  }
  return lastOffset;
}

function isCodexIdlePromptAfterWorkingBanner(buffer: string): boolean {
  const promptOffset = lastTerminalPromptOffset(buffer);
  if (promptOffset < 0) return false;
  return promptOffset > lastCodexWorkingBannerOffset(buffer);
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
    if (isCodexIdlePromptAfterWorkingBanner(nextBuffer)) {
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
  runtime_state_source?: string;
  effective_working_state?: string;
};

export function getActorDisplayWorkingState(
  actor: ActorWorkingStateInput,
  signal: TerminalSignal | null | undefined,
  now: number = Date.now(),
): string {
  const backendState = String(actor.effective_working_state || "").trim().toLowerCase() || "idle";
  const effectiveRunner = getEffectiveActorRunner(actor);
  const stateSource = String(actor.runtime_state_source || "").trim().toLowerCase();
  const isRunning = actor.running ?? actor.enabled ?? false;

  if (!isRunning || effectiveRunner === "headless" || stateSource === "app_server") {
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
