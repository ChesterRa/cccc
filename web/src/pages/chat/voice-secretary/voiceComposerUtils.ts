import type {
  AssistantVoiceAskFeedback,
  AssistantVoiceDocument,
  AssistantVoiceMeetingSession,
  AssistantVoicePromptDraft,
} from "../../../types";
import { createVoiceTranscriptItem, type VoiceTranscriptItem } from "./voiceStreamModel";
import { voiceTranscriptSourceDetail, voiceTranscriptSourceLabel } from "./voiceTranscriptSource";

const VOICE_ASK_ACTIVE_TIMEOUT_MS = 90_000;
const VOICE_PROMPT_REQUEST_STALE_MS = 90_000;
const VOICE_TRANSCRIPT_SUMMARY_MAX_CHARS = 72;

const LOW_VALUE_BROWSER_SPEECH_FRAGMENTS = new Set([
  "嗯",
  "嗯嗯",
  "啊",
  "好",
  "好的",
  "呃",
  "额",
  "那个",
  "えー",
  "ええ",
  "あの",
  "その",
  "はい",
  "うん",
  "uh",
  "um",
  "嗯。",
  "啊。",
  "好。",
  "はい。",
]);

const VOICE_LANGUAGE_OPTION_VALUES = ["mixed", "auto", "zh-CN", "en-US", "ja-JP", "ko-KR", "fr-FR", "de-DE", "es-ES"] as const;

export function promptDraftApplyMode(draft: AssistantVoicePromptDraft): "append" | "replace" {
  const operation = String(draft.operation || "").trim().toLowerCase();
  if (operation === "replace_with_refined_prompt" || operation === "replace") return "replace";
  return "append";
}

export function isVoicePromptRequestFresh(startedAt: number, nowMs = Date.now()): boolean {
  return startedAt > 0 && nowMs - startedAt < VOICE_PROMPT_REQUEST_STALE_MS;
}

export function assistantVoiceTimestampMs(value?: string): number {
  const text = String(value || "").trim();
  if (!text) return 0;
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? parsed : 0;
}

export function formatVoiceActivityTimeMs(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export function formatVoiceActivityFullTimeMs(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleString();
}

export function compactVoiceTranscriptSummaryText(value: string): string {
  const text = normalizeBrowserTranscriptChunk(value);
  if (text.length <= VOICE_TRANSCRIPT_SUMMARY_MAX_CHARS) return text;
  return `…${text.slice(-(VOICE_TRANSCRIPT_SUMMARY_MAX_CHARS - 1)).trimStart()}`;
}

export function askFeedbackStatusKey(status: string): string {
  return String(status || "pending").trim().toLowerCase();
}

export function isActiveAskFeedbackStatus(status: string): boolean {
  const key = askFeedbackStatusKey(status);
  return key === "pending" || key === "working";
}

export function isFinalAskFeedbackStatus(status: string): boolean {
  const key = askFeedbackStatusKey(status);
  return key === "done" || key === "needs_user" || key === "failed" || key === "handed_off";
}

export function hasFinalAskReply(item?: AssistantVoiceAskFeedback | null): boolean {
  return Boolean(item && isFinalAskFeedbackStatus(item.status) && String(item.reply_text || "").trim());
}

export function voiceReplyDismissKey(item?: AssistantVoiceAskFeedback | null): string {
  if (!item || !hasFinalAskReply(item)) return "";
  return [
    String(item.request_id || "").trim(),
    askFeedbackStatusKey(item.status),
    String(item.reply_text || "").trim(),
  ].join("\u0001");
}

export function shouldAutoOpenVoiceReplyBubble(params: {
  requestId: string;
  replyText: string;
  dismissKey: string;
  previousReplyKey?: string;
  isLocalRequest?: boolean;
  wasDismissed?: boolean;
}): boolean {
  const requestId = String(params.requestId || "").trim();
  if (!requestId || !String(params.replyText || "").trim() || !params.dismissKey) return false;
  if (params.wasDismissed) return false;
  if (params.isLocalRequest) return true;
  return Boolean(params.previousReplyKey && params.previousReplyKey !== params.dismissKey);
}

export function displayAskFeedbackStatus(item: AssistantVoiceAskFeedback, nowMs: number): string {
  const status = askFeedbackStatusKey(item.status);
  if (!isActiveAskFeedbackStatus(status)) {
    if (status === "done") return "";
    return status;
  }
  const touchedAt = assistantVoiceTimestampMs(item.updated_at) || assistantVoiceTimestampMs(item.created_at);
  if (touchedAt > 0 && nowMs - touchedAt >= VOICE_ASK_ACTIVE_TIMEOUT_MS) return "";
  return status === "pending" ? "working" : status;
}

export function askFeedbackDisplayText(item: AssistantVoiceAskFeedback): string {
  return String(item.reply_text || item.request_preview || item.request_text || "").trim();
}

export function slugifyVoiceDocumentDownloadName(value: string): string {
  return (
    value
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9\u3040-\u30ff\u3400-\u9fff]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 80) || "voice-secretary-document"
  );
}

export function voiceDocumentDownloadFileName(document: AssistantVoiceDocument | null, fallbackTitle: string): string {
  const workspacePath = String(document?.document_path || document?.workspace_path || "").trim().replace(/\\/g, "/");
  const workspaceName = workspacePath.split("/").filter(Boolean).pop() || "";
  if (workspaceName.toLowerCase().endsWith(".md")) return workspaceName;
  return `${slugifyVoiceDocumentDownloadName(fallbackTitle)}.md`;
}

export function voiceDocumentKey(document: AssistantVoiceDocument | null | undefined): string {
  return voiceDocumentPath(document) || String(document?.document_id || "").trim();
}

export function voiceDocumentPath(document: AssistantVoiceDocument | null | undefined): string {
  return String(document?.document_path || document?.workspace_path || "").trim();
}

export function voiceDocumentMatches(document: AssistantVoiceDocument, idOrPath: string): boolean {
  const target = String(idOrPath || "").trim();
  if (!target) return false;
  return (
    voiceDocumentPath(document) === target
    || voiceDocumentKey(document) === target
    || String(document.document_id || "").trim() === target
  );
}

export function findVoiceDocument(documents: AssistantVoiceDocument[], idOrPath: string): AssistantVoiceDocument | null {
  const target = String(idOrPath || "").trim();
  if (!target) return null;
  return documents.find((document) => voiceDocumentMatches(document, target)) || null;
}

export function resolveVoiceDocumentPath(documents: AssistantVoiceDocument[], idOrPath: string): string {
  const target = String(idOrPath || "").trim();
  if (!target) return "";
  const directPath = documents.some((document) => voiceDocumentPath(document) === target);
  if (directPath) return target;
  return voiceDocumentPath(findVoiceDocument(documents, target));
}

export function downloadMarkdownDocument(fileName: string, content: string): void {
  if (typeof document === "undefined") return;
  const blob = new Blob([content], { type: "text/markdown;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  anchor.rel = "noopener";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

export function voiceLanguageOptionValues(configuredLanguage: string): string[] {
  const values: string[] = [...VOICE_LANGUAGE_OPTION_VALUES];
  const configured = String(configuredLanguage || "").trim();
  if (configured && !values.includes(configured)) values.push(configured);
  return values;
}

export function numberFromUnknown(value: unknown, fallback: number, min: number, max: number): number {
  const numberValue = Number(value);
  if (!Number.isFinite(numberValue)) return fallback;
  return Math.max(min, Math.min(max, Math.round(numberValue)));
}

export function recordFromUnknown(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
}

export function normalizeBrowserTranscriptChunk(value: string): string {
  return String(value || "")
    .replace(/\s+/g, " ")
    .replace(/\s+([,.!?;:，。！？；：、])/g, "$1")
    .trim();
}

export function isLowValueBrowserSpeechFragment(value: string): boolean {
  const text = normalizeBrowserTranscriptChunk(value).toLowerCase();
  if (!text) return true;
  return LOW_VALUE_BROWSER_SPEECH_FRAGMENTS.has(text);
}

function longestTranscriptOverlap(left: string, right: string): number {
  const max = Math.min(left.length, right.length, 120);
  for (let size = max; size >= 2; size -= 1) {
    if (left.slice(-size) === right.slice(0, size)) return size;
  }
  return 0;
}

export function mergeTranscriptChunks(previous: string, nextText: string): string {
  const prev = normalizeBrowserTranscriptChunk(previous);
  const next = normalizeBrowserTranscriptChunk(nextText);
  if (!prev) return next;
  if (!next) return prev;
  if (prev === next || prev.endsWith(next)) return prev;
  if (next.endsWith(prev)) return next;
  const overlap = longestTranscriptOverlap(prev, next);
  if (overlap > 0) return `${prev}${next.slice(overlap)}`;
  const cjkBoundary = /[\u3040-\u30ff\u3400-\u9fff]$/.test(prev) && /^[\u3040-\u30ff\u3400-\u9fff]/.test(next);
  return cjkBoundary ? `${prev}${next}` : `${prev} ${next}`;
}

export function stripUncertainSpeakerPrefix(value: string): string {
  return normalizeBrowserTranscriptChunk(value).replace(/^Speaker\s*\?:\s*/i, "").trim();
}

export function nextUncommittedServiceTranscriptText(partialText: string, committedText: string): string {
  const partial = stripUncertainSpeakerPrefix(partialText);
  if (!partial) return "";
  const committed = normalizeBrowserTranscriptChunk(committedText);
  if (committed && (committed === partial || committed.endsWith(partial))) return "";
  return committed && partial.startsWith(committed)
    ? partial.slice(committed.length).trim()
    : partial;
}

export function voiceTranscriptItemsFromMeetingSession(
  session: AssistantVoiceMeetingSession,
  opts?: { documentPathFallback?: string },
): VoiceTranscriptItem[] {
  if (String(session.capture_mode || "document").trim().toLowerCase() !== "document") return [];
  const documentPath = String(session.document_path || opts?.documentPathFallback || "").trim();
  if (!documentPath) return [];
  const diarization = recordFromUnknown(session.diarization);
  const speakerTranscriptSegments = Array.isArray(diarization.speaker_transcript_segments)
    ? diarization.speaker_transcript_segments
    : [];
  const speakerTranscriptModelId = String(diarization.speaker_transcript_model_id || "").trim();
  const segments = speakerTranscriptSegments.length
    ? speakerTranscriptSegments
    : Array.isArray(session.segments) ? session.segments : [];
  const items = segments
    .map((segment, index): VoiceTranscriptItem | null => {
      const record = recordFromUnknown(segment);
      const text = normalizeBrowserTranscriptChunk(String(record.text || ""));
      if (!text) return null;
      const trigger = recordFromUnknown(record.trigger);
      const speakerLabel = String(record.speaker_label || "").trim();
      const rawSpeakerIndex = Number(record.speaker_index);
      const source = speakerTranscriptSegments.length
        ? "assistant_service_local_asr_final"
        : String(trigger.recognition_backend || "").trim();
      const updatedAt = assistantVoiceTimestampMs(String(record.updated_at || record.created_at || "")) || Date.now() - index;
      const item = createVoiceTranscriptItem({
        id: String(record.segment_id || "").trim() || `${session.session_id}-segment-${index}`,
        cleanText: text,
        metadata: {
          mode: "document",
          sessionId: String(session.session_id || "").trim(),
          documentPath,
          language: String(record.language || session.language || "").trim(),
          source,
          sourceLabel: voiceTranscriptSourceLabel(source),
          sourceDetail: voiceTranscriptSourceDetail({
            source,
            modelId: speakerTranscriptModelId || String(trigger.final_model_id || trigger.model_id || "").trim(),
            engine: speakerTranscriptSegments.length ? "sense_voice" : String(trigger.engine || "").trim(),
            language: String(trigger.asr_language || trigger.language || "").trim(),
            chunks: Number.isFinite(Number(trigger.chunk_count)) ? Number(trigger.chunk_count) : undefined,
            fallbackReason: String(trigger.fallback_reason || "").trim(),
          }),
        },
        timing: {
          startMs: Number.isFinite(Number(record.start_ms)) ? Number(record.start_ms) : undefined,
          endMs: Number.isFinite(Number(record.end_ms)) ? Number(record.end_ms) : undefined,
        },
        now: updatedAt,
      });
      if (!item) return null;
      return {
        ...item,
        ...(speakerLabel ? { speakerLabel } : {}),
        ...(Number.isFinite(rawSpeakerIndex) ? { speakerIndex: rawSpeakerIndex } : {}),
      };
    })
    .filter((item): item is VoiceTranscriptItem => item !== null);

  const partial = normalizeBrowserTranscriptChunk(String(session.latest_partial || ""));
  if (partial) {
    const updatedAt = assistantVoiceTimestampMs(String(session.updated_at || "")) || Date.now();
    items.unshift({
      id: `${session.session_id}-partial`,
      phase: "interim",
      text: partial,
      pendingFinalText: "",
      interimText: partial,
      mode: "document",
      documentPath,
      language: String(session.language || "").trim(),
      updatedAt,
      createdAt: updatedAt,
    });
  }
  return items;
}

export function hashComposerSnapshot(value: string): string {
  let hash = 2166136261;
  const text = String(value || "");
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}
