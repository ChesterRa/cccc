import type { VoiceSecretaryCaptureMode } from "./voiceSecretaryTypes";

export type VoiceServiceStopDispatchKind = "" | "prompt" | "instruction";

const LOW_VALUE_VOICE_DISPATCH_CJK_CHARS = new Set([
  "嗯",
  "呃",
  "啊",
  "哦",
]);

const LOW_VALUE_VOICE_DISPATCH_WORDS = new Set([
  "a",
  "ah",
  "an",
  "i",
  "l",
  "no",
  "nope",
  "oh",
  "ok",
  "okay",
  "the",
  "uh",
  "um",
  "yeah",
  "yep",
  "yes",
]);

function cleanVoiceDispatchText(value: string): string {
  return String(value || "")
    .replace(/^Speaker\s*\?:\s*/i, "")
    .replace(/\s+/g, " ")
    .trim();
}

function cjkSemanticText(value: string): string {
  return value.replace(/[^\u3040-\u30ff\u3400-\u9fff\uac00-\ud7af]+/g, "");
}

function latinSemanticTokens(value: string): string[] {
  return value.match(/[0-9A-Za-z]+/g)?.map((token) => token.toLowerCase()) ?? [];
}

function hasMeaningfulCjkText(value: string): boolean {
  const text = cjkSemanticText(value);
  if (text.length < 2) return false;
  return Array.from(text).some((char) => !LOW_VALUE_VOICE_DISPATCH_CJK_CHARS.has(char));
}

function hasMeaningfulLatinText(value: string): boolean {
  const tokens = latinSemanticTokens(value).filter((token) => !LOW_VALUE_VOICE_DISPATCH_WORDS.has(token));
  if (!tokens.length) return false;
  return tokens.some((token) => /\d/.test(token) || token.length >= 2);
}

export function isMeaningfulVoiceDispatchText(value: string): boolean {
  const text = cleanVoiceDispatchText(value);
  if (!text) return false;
  return hasMeaningfulCjkText(text) || hasMeaningfulLatinText(text);
}

export function voiceServiceStopDispatchKind(params: {
  mode: VoiceSecretaryCaptureMode;
  transcriptText: string;
  pendingPromptRequestId?: string;
  pendingAskRequestId?: string;
}): VoiceServiceStopDispatchKind {
  if (!isMeaningfulVoiceDispatchText(params.transcriptText)) return "";
  if (params.mode === "prompt" && !String(params.pendingPromptRequestId || "").trim()) return "prompt";
  if (params.mode === "instruction" && !String(params.pendingAskRequestId || "").trim()) return "instruction";
  return "";
}
