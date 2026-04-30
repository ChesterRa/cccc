import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type {
  AssistantVoiceAskFeedback,
  AssistantVoiceDocument,
  AssistantVoiceMeetingSession,
  AssistantVoicePromptDraft,
  AssistantServiceRuntime,
  AssistantVoiceTranscriptSegmentResult,
  BuiltinAssistant,
  LedgerEvent,
} from "../../types";
import { classNames } from "../../utils/classNames";
import { ChevronDownIcon, CloseIcon, CopyIcon, MaximizeIcon, MicrophoneIcon, RefreshIcon, SparklesIcon, StopIcon } from "../../components/Icons";
import { GroupCombobox } from "../../components/GroupCombobox";
import { LazyMarkdownRenderer } from "../../components/LazyMarkdownRenderer";
import { Popover, PopoverContent, PopoverTrigger } from "../../components/ui/popover";
import {
  ackVoiceAssistantPromptDraft,
  appendVoiceAssistantInput,
  appendVoiceAssistantTranscriptSegment,
  archiveVoiceAssistantDocument,
  clearVoiceAssistantAskRequests,
  fetchAssistant,
  fetchLatestVoiceAssistantMeetingSession,
  saveVoiceAssistantDocument,
  sendVoiceAssistantDocumentInstruction,
  updateAssistantSettings,
  withAuthToken,
} from "../../services/api";
import { useGroupStore, useUIStore } from "../../stores";
import { useModalA11y } from "../../hooks/useModalA11y";
import { AnimatedShinyText } from "../../registry/magicui/animated-shiny-text";
import { copyTextToClipboard } from "../../utils/copy";
import { VoiceSecretaryDocumentListPanel } from "./voice-secretary/VoiceSecretaryDocumentListPanel";
import { VoiceSecretaryRequestPanel } from "./voice-secretary/VoiceSecretaryRequestPanel";
import { VoiceSecretaryWorkspacePanel } from "./voice-secretary/VoiceSecretaryWorkspacePanel";
import type { VoiceWorkspaceView } from "./voice-secretary/VoiceSecretaryWorkspacePanel";
import { useVoiceCaptureTargetDocumentSelection } from "./voice-secretary/useVoiceCaptureTargetDocumentSelection";
import {
  appendFinalVoiceStreamItem,
  createVoiceStreamMessage,
  createVoiceTranscriptPreview,
  type VoiceDiarizationStreamItem,
  type VoiceStreamItem,
  type VoiceTranscriptPreview,
  type VoiceTranscriptPreviewPhase,
  upsertLiveVoiceStreamItem,
} from "./voice-secretary/voiceStreamModel";
import { resolveVoiceServiceReadiness } from "./voice-secretary/voiceServiceReadiness";

type VoiceSecretaryComposerControlProps = {
  isDark: boolean;
  selectedGroupId: string;
  busy: string;
  buttonClassName?: string;
  buttonSizePx?: number;
  disabled?: boolean;
  variant?: "button" | "assistantRow";
  captureMode?: VoiceSecretaryCaptureMode;
  onCaptureModeChange?: (mode: VoiceSecretaryCaptureMode) => void;
  composerText?: string;
  composerContext?: Record<string, unknown>;
  onPromptDraft?: (text: string, opts?: { mode?: "replace" | "append" }) => void;
};

export type VoiceSecretaryCaptureMode = "document" | "instruction" | "prompt";

type BrowserSpeechRecognitionAlternative = {
  transcript: string;
};

type BrowserSpeechRecognitionResult = {
  isFinal: boolean;
  length: number;
  [index: number]: BrowserSpeechRecognitionAlternative;
};

type BrowserSpeechRecognitionResultList = {
  length: number;
  [index: number]: BrowserSpeechRecognitionResult;
};

type BrowserSpeechRecognitionEvent = {
  resultIndex: number;
  results: BrowserSpeechRecognitionResultList;
};

type BrowserSpeechRecognitionErrorEvent = {
  error?: string;
  message?: string;
};

type BrowserSpeechRecognition = {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  maxAlternatives?: number;
  onresult: ((event: BrowserSpeechRecognitionEvent) => void) | null;
  onerror: ((event: BrowserSpeechRecognitionErrorEvent) => void) | null;
  onend: (() => void) | null;
  onspeechstart?: (() => void) | null;
  onspeechend?: (() => void) | null;
  start: () => void;
  stop: () => void;
  abort: () => void;
};

type BrowserSpeechRecognitionConstructor = new () => BrowserSpeechRecognition;

type VoiceCaptureLock = {
  ownerId: string;
  groupId: string;
  updatedAt: number;
};

type VoiceCaptureChannelMessage = {
  type?: "probe" | "alive";
  ownerId?: string;
  groupId?: string;
  sentAt?: number;
};

type VoiceDocumentActivityStatus = "queued" | "updated";

type VoiceDocumentActivityItem = {
  id: string;
  status: VoiceDocumentActivityStatus;
  documentTitle?: string;
  documentPath?: string;
  preview?: string;
  mode: VoiceSecretaryCaptureMode;
  createdAt: number;
};

type VoiceDiarizationActivityItem = VoiceDiarizationStreamItem;

type AssistantVoiceDocumentLedgerData = {
  action?: unknown;
  document_path?: unknown;
  input_kind?: unknown;
  input_preview?: unknown;
  request_id?: unknown;
  status?: unknown;
  title?: unknown;
  workspace_path?: unknown;
};

type BrowserMicrophoneSupportIssue = "" | "secure_context" | "get_user_media";
type BrowserAudioSupportIssue = BrowserMicrophoneSupportIssue;
type BrowserSpeechSupportIssue = "" | "unsupported";

const VOICE_CAPTURE_LOCK_KEY = "cccc.voiceSecretary.activeCapture";
const VOICE_CAPTURE_CHANNEL_NAME = "cccc.voiceSecretary.capture";
const VOICE_CAPTURE_LOCK_TTL_MS = 30 * 1000;
const VOICE_CAPTURE_LOCK_PROBE_TIMEOUT_MS = 300;
const BROWSER_DEFAULT_MIC_LABEL = "browser_default";
const SERVICE_DEFAULT_MIC_LABEL = "service_default";
const BROWSER_SPEECH_MIN_QUIET_MS = 1_000;
const BROWSER_SPEECH_FAST_MODE_QUIET_REDUCTION_MS = 2_000;
const BROWSER_SPEECH_MAX_WINDOW_FALLBACK_MS = 120_000;
const BROWSER_SPEECH_MIN_MAX_WINDOW_MS = 10_000;
const BROWSER_SPEECH_RESTART_BASE_MS = 500;
const BROWSER_SPEECH_RESTART_MAX_MS = 8000;
const BROWSER_SPEECH_MAX_TRANSIENT_ERRORS = 8;
const BROWSER_SPEECH_RECOVERABLE_ERRORS = new Set(["no-speech", "aborted", "network", "audio-capture"]);
const BROWSER_SPEECH_FATAL_ERRORS = new Set(["not-allowed", "service-not-allowed"]);
const VOICE_ASK_ACTIVE_TIMEOUT_MS = 90_000;
const VOICE_LIVE_TRANSCRIPT_VISIBLE_MS = 60_000;
const VOICE_DOCUMENT_ACTIVITY_VISIBLE_MS = 60_000;
const VOICE_ACTIVITY_FEED_LIMIT = 10;
const VOICE_TRANSCRIPT_SUMMARY_MAX_CHARS = 72;
const STREAMING_ASR_RUNTIME_ID = "sherpa_onnx_streaming";
const TWO_LINE_STATUS_STYLE = {
  display: "-webkit-box",
  WebkitLineClamp: 2,
  WebkitBoxOrient: "vertical",
  overflow: "hidden",
} as const;
function promptDraftApplyMode(draft: AssistantVoicePromptDraft): "append" | "replace" {
  const operation = String(draft.operation || "").trim().toLowerCase();
  if (operation === "replace_with_refined_prompt" || operation === "replace") return "replace";
  return "append";
}
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

function assistantVoiceTimestampMs(value?: string): number {
  const text = String(value || "").trim();
  if (!text) return 0;
  const parsed = Date.parse(text);
  return Number.isFinite(parsed) ? parsed : 0;
}

function formatVoiceActivityTimeMs(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function formatVoiceActivityFullTimeMs(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  return date.toLocaleString();
}

function formatVoiceOffsetMs(value: number): string {
  const totalSeconds = Math.max(0, Math.floor(Number(value || 0) / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function compactVoiceTranscriptSummaryText(value: string): string {
  const text = normalizeBrowserTranscriptChunk(value);
  if (text.length <= VOICE_TRANSCRIPT_SUMMARY_MAX_CHARS) return text;
  return `…${text.slice(-(VOICE_TRANSCRIPT_SUMMARY_MAX_CHARS - 1)).trimStart()}`;
}

function askFeedbackStatusKey(status: string): string {
  return String(status || "pending").trim().toLowerCase();
}

function isActiveAskFeedbackStatus(status: string): boolean {
  const key = askFeedbackStatusKey(status);
  return key === "pending" || key === "working";
}

function isFinalAskFeedbackStatus(status: string): boolean {
  const key = askFeedbackStatusKey(status);
  return key === "done" || key === "needs_user" || key === "failed" || key === "handed_off";
}

function hasFinalAskReply(item?: AssistantVoiceAskFeedback | null): boolean {
  return Boolean(item && isFinalAskFeedbackStatus(item.status) && String(item.reply_text || "").trim());
}

function voiceReplyDismissKey(item?: AssistantVoiceAskFeedback | null): string {
  if (!item || !hasFinalAskReply(item)) return "";
  return [
    String(item.request_id || "").trim(),
    askFeedbackStatusKey(item.status),
    String(item.reply_text || "").trim(),
  ].join("\u0001");
}

function displayAskFeedbackStatus(item: AssistantVoiceAskFeedback, nowMs: number): string {
  const status = askFeedbackStatusKey(item.status);
  if (!isActiveAskFeedbackStatus(status)) {
    if (status === "done") return "";
    return status;
  }
  const touchedAt = assistantVoiceTimestampMs(item.updated_at) || assistantVoiceTimestampMs(item.created_at);
  if (touchedAt > 0 && nowMs - touchedAt >= VOICE_ASK_ACTIVE_TIMEOUT_MS) return "";
  return status === "pending" ? "working" : status;
}

function askFeedbackDisplayText(item: AssistantVoiceAskFeedback): string {
  return String(item.reply_text || item.request_preview || item.request_text || "").trim();
}
const VOICE_LANGUAGE_OPTION_VALUES = ["mixed", "auto", "zh-CN", "en-US", "ja-JP", "ko-KR", "fr-FR", "de-DE", "es-ES"] as const;

function slugifyVoiceDocumentDownloadName(value: string): string {
  return (
    value
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9\u3040-\u30ff\u3400-\u9fff]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 80) || "voice-secretary-document"
  );
}

function voiceDocumentDownloadFileName(document: AssistantVoiceDocument | null, fallbackTitle: string): string {
  const workspacePath = String(document?.document_path || document?.workspace_path || "").trim().replace(/\\/g, "/");
  const workspaceName = workspacePath.split("/").filter(Boolean).pop() || "";
  if (workspaceName.toLowerCase().endsWith(".md")) return workspaceName;
  return `${slugifyVoiceDocumentDownloadName(fallbackTitle)}.md`;
}

function voiceDocumentKey(document: AssistantVoiceDocument | null | undefined): string {
  return voiceDocumentPath(document) || String(document?.document_id || "").trim();
}

function voiceDocumentPath(document: AssistantVoiceDocument | null | undefined): string {
  return String(document?.document_path || document?.workspace_path || "").trim();
}

function voiceDocumentMatches(document: AssistantVoiceDocument, idOrPath: string): boolean {
  const target = String(idOrPath || "").trim();
  if (!target) return false;
  return (
    voiceDocumentPath(document) === target
    || voiceDocumentKey(document) === target
    || String(document.document_id || "").trim() === target
  );
}

function findVoiceDocument(documents: AssistantVoiceDocument[], idOrPath: string): AssistantVoiceDocument | null {
  const target = String(idOrPath || "").trim();
  if (!target) return null;
  return documents.find((document) => voiceDocumentMatches(document, target)) || null;
}

function resolveVoiceDocumentPath(documents: AssistantVoiceDocument[], idOrPath: string): string {
  const target = String(idOrPath || "").trim();
  if (!target) return "";
  const directPath = documents.some((document) => voiceDocumentPath(document) === target);
  if (directPath) return target;
  return voiceDocumentPath(findVoiceDocument(documents, target));
}

function downloadMarkdownDocument(fileName: string, content: string): void {
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

function createVoiceCaptureOwnerId(): string {
  return `voice-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

function readVoiceCaptureLock(): VoiceCaptureLock | null {
  if (typeof window === "undefined") return null;
  try {
    const raw = window.localStorage.getItem(VOICE_CAPTURE_LOCK_KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<VoiceCaptureLock>;
    const ownerId = String(parsed.ownerId || "").trim();
    const groupId = String(parsed.groupId || "").trim();
    const updatedAt = Number(parsed.updatedAt || 0);
    if (!ownerId || !groupId || !Number.isFinite(updatedAt)) return null;
    if (Date.now() - updatedAt > VOICE_CAPTURE_LOCK_TTL_MS) {
      window.localStorage.removeItem(VOICE_CAPTURE_LOCK_KEY);
      return null;
    }
    return { ownerId, groupId, updatedAt };
  } catch {
    return null;
  }
}

function writeVoiceCaptureLock(ownerId: string, groupId: string): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(
      VOICE_CAPTURE_LOCK_KEY,
      JSON.stringify({ ownerId, groupId, updatedAt: Date.now() }),
    );
  } catch {
    void 0;
  }
}

function clearVoiceCaptureLock(ownerId?: string): void {
  if (typeof window === "undefined") return;
  try {
    if (ownerId) {
      const active = readVoiceCaptureLock();
      if (active && active.ownerId !== ownerId) return;
    }
    window.localStorage.removeItem(VOICE_CAPTURE_LOCK_KEY);
  } catch {
    void 0;
  }
}

function openVoiceCaptureChannel(): BroadcastChannel | null {
  if (typeof BroadcastChannel === "undefined") return null;
  try {
    return new BroadcastChannel(VOICE_CAPTURE_CHANNEL_NAME);
  } catch {
    return null;
  }
}

function probeVoiceCaptureOwner(lock: VoiceCaptureLock): Promise<boolean> {
  const channel = openVoiceCaptureChannel();
  if (!channel) return Promise.resolve(true);
  return new Promise((resolve) => {
    let settled = false;
    const finish = (alive: boolean) => {
      if (settled) return;
      settled = true;
      channel.removeEventListener("message", handleMessage);
      channel.close();
      resolve(alive);
    };
    const handleMessage = (event: MessageEvent<VoiceCaptureChannelMessage>) => {
      const message = event.data || {};
      if (message.type !== "alive") return;
      if (String(message.ownerId || "") !== lock.ownerId) return;
      finish(true);
    };
    channel.addEventListener("message", handleMessage);
    window.setTimeout(() => finish(false), VOICE_CAPTURE_LOCK_PROBE_TIMEOUT_MS);
    channel.postMessage({
      type: "probe",
      ownerId: lock.ownerId,
      groupId: lock.groupId,
      sentAt: Date.now(),
    } satisfies VoiceCaptureChannelMessage);
  });
}

async function claimVoiceCaptureLock(ownerId: string, groupId: string): Promise<VoiceCaptureLock | null> {
  const active = readVoiceCaptureLock();
  if (active && active.ownerId !== ownerId) {
    const ownerAlive = await probeVoiceCaptureOwner(active);
    if (ownerAlive) return active;
    clearVoiceCaptureLock(active.ownerId);
  }
  writeVoiceCaptureLock(ownerId, groupId);
  return null;
}

function refreshVoiceCaptureLock(ownerId: string, groupId: string): void {
  const active = readVoiceCaptureLock();
  if (!active || active.ownerId === ownerId) writeVoiceCaptureLock(ownerId, groupId);
}

function releaseVoiceCaptureLock(ownerId: string): void {
  clearVoiceCaptureLock(ownerId);
}

function getBrowserSpeechRecognitionConstructor(): BrowserSpeechRecognitionConstructor | null {
  if (typeof window === "undefined") return null;
  const speechWindow = window as typeof window & {
    SpeechRecognition?: BrowserSpeechRecognitionConstructor;
    webkitSpeechRecognition?: BrowserSpeechRecognitionConstructor;
  };
  return speechWindow.SpeechRecognition || speechWindow.webkitSpeechRecognition || null;
}

function getBrowserSpeechSupportIssue(): BrowserSpeechSupportIssue {
  if (!getBrowserSpeechRecognitionConstructor()) return "unsupported";
  return "";
}

function mediaRecorderSupported(): boolean {
  return !getBrowserAudioSupportIssue();
}

function getBrowserMicrophoneSupportIssue(): BrowserMicrophoneSupportIssue {
  if (typeof window !== "undefined" && window.isSecureContext === false) return "secure_context";
  if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia) return "get_user_media";
  return "";
}

function getBrowserAudioSupportIssue(): BrowserAudioSupportIssue {
  const microphoneIssue = getBrowserMicrophoneSupportIssue();
  if (microphoneIssue) return microphoneIssue;
  return "";
}

function stopMediaStream(stream: MediaStream | null): void {
  if (!stream) return;
  try {
    stream.getTracks().forEach((track) => track.stop());
  } catch {
    // ignore browser cleanup failure
  }
}

function mediaStreamHasLiveAudio(stream: MediaStream | null): boolean {
  if (!stream) return false;
  try {
    return stream.getAudioTracks().some((track) => track.readyState === "live");
  } catch {
    return false;
  }
}

function browserSpeechRestartDelayMs(transientErrorCount: number): number {
  const count = Math.max(1, transientErrorCount);
  return Math.min(BROWSER_SPEECH_RESTART_MAX_MS, BROWSER_SPEECH_RESTART_BASE_MS * count);
}

function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    const chunk = bytes.subarray(offset, offset + chunkSize);
    binary += String.fromCharCode(...chunk);
  }
  return btoa(binary);
}

class Pcm16Resampler {
  private readonly ratio: number;
  private carry = new Float32Array(0);
  private gain = 1.8;

  constructor(inputSampleRate: number) {
    const sourceRate = Math.max(1, Number(inputSampleRate) || 48000);
    this.ratio = sourceRate / 16000;
  }

  push(input: Float32Array): Uint8Array {
    if (!input.length) return new Uint8Array(0);
    const samples = this.carry.length ? new Float32Array(this.carry.length + input.length) : input;
    if (this.carry.length) {
      samples.set(this.carry, 0);
      samples.set(input, this.carry.length);
    }
    const outputLength = Math.max(0, Math.floor(samples.length / this.ratio));
    const consumedSamples = Math.min(samples.length, Math.floor(outputLength * this.ratio));
    this.carry = consumedSamples < samples.length ? samples.slice(consumedSamples) : new Float32Array(0);
    if (outputLength <= 0) return new Uint8Array(0);
    const output = new Int16Array(outputLength);
    for (let index = 0; index < outputLength; index += 1) {
      const start = index * this.ratio;
      const end = Math.min(samples.length, (index + 1) * this.ratio);
      const startIndex = Math.floor(start);
      const endIndex = Math.max(startIndex + 1, Math.ceil(end));
      let total = 0;
      let totalWeight = 0;
      for (let sourceIndex = startIndex; sourceIndex < endIndex; sourceIndex += 1) {
        const sampleStart = Math.max(start, sourceIndex);
        const sampleEnd = Math.min(end, sourceIndex + 1);
        const weight = Math.max(0, sampleEnd - sampleStart);
        if (weight > 0) {
          total += (samples[sourceIndex] || 0) * weight;
          totalWeight += weight;
        }
      }
      const averaged = totalWeight > 0 ? total / totalWeight : 0;
      const sample = Math.max(-1, Math.min(1, averaged * this.gain));
      output[index] = sample < 0 ? sample * 0x8000 : sample * 0x7fff;
    }
    return new Uint8Array(output.buffer);
  }
}

function voiceLanguageOptionValues(configuredLanguage: string): string[] {
  const values: string[] = [...VOICE_LANGUAGE_OPTION_VALUES];
  const configured = String(configuredLanguage || "").trim();
  if (configured && !values.includes(configured)) values.push(configured);
  return values;
}

function numberFromUnknown(value: unknown, fallback: number, min: number, max: number): number {
  const numberValue = Number(value);
  if (!Number.isFinite(numberValue)) return fallback;
  return Math.max(min, Math.min(max, Math.round(numberValue)));
}

function recordFromUnknown(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
}

function voiceSessionTimestampMs(value: unknown, fallback: number): number {
  const parsed = Date.parse(String(value || ""));
  return Number.isFinite(parsed) ? parsed : fallback;
}

function voiceStreamItemsFromMeetingSession(session: AssistantVoiceMeetingSession): VoiceStreamItem[] {
  const segments = Array.isArray(session.segments) ? session.segments : [];
  const items = segments.map((segment, index): VoiceStreamItem | null => {
    const record = recordFromUnknown(segment);
    const text = normalizeBrowserTranscriptChunk(String(record.text || ""));
    if (!text) return null;
    const createdAt = voiceSessionTimestampMs(record.created_at, Date.now() - index);
    const trigger = recordFromUnknown(record.trigger);
    const mode = String(trigger.mode || "document").trim() as VoiceSecretaryCaptureMode;
    return {
      id: String(record.segment_id || `persisted-${index}`).trim() || `persisted-${index}`,
      phase: "final",
      text,
      mode: mode === "instruction" || mode === "prompt" ? mode : "document",
      documentPath: String(record.document_path || trigger.document_path || "").trim() || undefined,
      language: String(record.language || trigger.language || session.language || "").trim() || undefined,
      startMs: Number.isFinite(Number(record.start_ms)) ? Number(record.start_ms) : undefined,
      endMs: Number.isFinite(Number(record.end_ms)) ? Number(record.end_ms) : undefined,
      createdAt,
      updatedAt: createdAt,
    };
  }).filter((item): item is VoiceStreamItem => item !== null);

  const partial = normalizeBrowserTranscriptChunk(String(session.latest_partial || ""));
  if (partial) {
    const updatedAt = voiceSessionTimestampMs(session.updated_at, Date.now());
    items.unshift({
      id: `${session.session_id}:latest-partial`,
      phase: "interim",
      text: partial,
      mode: "document",
      language: String(session.language || "").trim() || undefined,
      documentPath: String(session.document_path || "").trim() || undefined,
      createdAt: updatedAt,
      updatedAt,
    });
  }
  return items;
}

function diarizationItemFromMeetingSession(session: AssistantVoiceMeetingSession): VoiceDiarizationStreamItem | null {
  const diarization = recordFromUnknown(session.diarization);
  const artifact = recordFromUnknown(diarization.artifact_path ? diarization : session.diarization);
  const segments = Array.isArray(diarization.segments)
    ? diarization.segments
    : Array.isArray(artifact.segments)
      ? artifact.segments
      : [];
  const normalizedSegments = segments.map((item) => {
    const record = recordFromUnknown(item);
    return {
      speaker_label: String(record.speaker_label || "").trim(),
      start_ms: Number(record.start_ms || 0),
      end_ms: Number(record.end_ms || 0),
    };
  }).filter((item) => item.speaker_label && item.end_ms > item.start_ms);
  const speakerTranscriptSegments = (Array.isArray(diarization.speaker_transcript_segments)
    ? diarization.speaker_transcript_segments
    : Array.isArray(artifact.speaker_transcript_segments)
      ? artifact.speaker_transcript_segments
      : []
  ).map((item) => {
    const record = recordFromUnknown(item);
    return {
      speaker_label: String(record.speaker_label || "").trim(),
      start_ms: Number(record.start_ms || 0),
      end_ms: Number(record.end_ms || 0),
      text: String(record.text || "").trim(),
    };
  }).filter((item) => item.speaker_label && item.text && item.end_ms > item.start_ms);
  const error = recordFromUnknown(session.error || artifact.speaker_transcript_error);
  if (!normalizedSegments.length && !Object.keys(error).length && !session.diarization_artifact_path) return null;
  const diarizationReady = Boolean(session.diarization_ready) || String(artifact.status || diarization.status || "").trim() === "ready";
  const speakerLabels = normalizedSegments
    .map((item) => item.speaker_label || "")
    .filter((label, index, labels) => label && labels.indexOf(label) === index);
  return {
    id: `diarization-${session.session_id}`,
    status: Object.keys(error).length ? "failed" : diarizationReady || normalizedSegments.length ? "ready" : "working",
    speakerCount: speakerLabels.length,
    preview: normalizedSegments.slice(0, 4).map((item) => `${item.speaker_label} ${formatVoiceOffsetMs(item.start_ms || 0)}-${formatVoiceOffsetMs(item.end_ms || 0)}`).join("\n"),
    segments: normalizedSegments,
    speakerTranscriptSegments,
    artifactPath: String(session.diarization_artifact_path || diarization.artifact_path || "").trim() || undefined,
    error: String(error.message || "").trim() || undefined,
    createdAt: voiceSessionTimestampMs(session.updated_at, Date.now()),
  };
}

function mergeRestoredDiarizationActivity(
  currentItems: VoiceDiarizationActivityItem[],
  restoredItem: VoiceDiarizationActivityItem,
): VoiceDiarizationActivityItem[] {
  const existingIndex = currentItems.findIndex((item) => item.id === restoredItem.id);
  if (existingIndex < 0) return [restoredItem, ...currentItems].slice(0, VOICE_ACTIVITY_FEED_LIMIT);
  const existing = currentItems[existingIndex];
  const shouldReplace =
    existing.status === "working"
    || existing.status === "provisional"
    || restoredItem.status === "ready"
    || restoredItem.status === "failed"
    || restoredItem.createdAt >= existing.createdAt;
  if (!shouldReplace) return currentItems;
  return [
    restoredItem,
    ...currentItems.filter((item) => item.id !== restoredItem.id),
  ].slice(0, VOICE_ACTIVITY_FEED_LIMIT);
}

function normalizeBrowserTranscriptChunk(value: string): string {
  return String(value || "")
    .replace(/\s+/g, " ")
    .replace(/\s+([,.!?;:，。！？；：、])/g, "$1")
    .trim();
}

function isLowValueBrowserSpeechFragment(value: string): boolean {
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

function mergeTranscriptChunks(previous: string, nextText: string): string {
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

function hashComposerSnapshot(value: string): string {
  let hash = 2166136261;
  const text = String(value || "");
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}

function abortBrowserSpeechRecognition(recognition: BrowserSpeechRecognition | null): void {
  if (!recognition) return;
  recognition.onend = null;
  recognition.onerror = null;
  recognition.onresult = null;
  recognition.onspeechstart = null;
  recognition.onspeechend = null;
  try {
    recognition.abort();
  } catch {
    // ignore browser cleanup failure
  }
}

export function VoiceSecretaryComposerControl({
  isDark,
  selectedGroupId,
  busy,
  buttonClassName = "",
  buttonSizePx = 44,
  disabled,
  variant = "button",
  captureMode = "document",
  onCaptureModeChange,
  composerText = "",
  composerContext = {},
  onPromptDraft,
}: VoiceSecretaryComposerControlProps) {
  const { t } = useTranslation("chat");
  const showError = useUIStore((state) => state.showError);
  const showNotice = useUIStore((state) => state.showNotice);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const refreshSeq = useRef(0);
  const recognitionRef = useRef<BrowserSpeechRecognition | null>(null);
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const mediaStreamRef = useRef<MediaStream | null>(null);
  const mediaChunksRef = useRef<Blob[]>([]);
  const serviceAudioContextRef = useRef<AudioContext | null>(null);
  const serviceAudioProcessorRef = useRef<ScriptProcessorNode | null>(null);
  const serviceAudioSourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const serviceAudioWsRef = useRef<WebSocket | null>(null);
  const serviceAudioResamplerRef = useRef<Pcm16Resampler | null>(null);
  const serviceAudioSeqRef = useRef(0);
  const serviceFinalTranscriptRef = useRef("");
  const serviceLatestPartialTranscriptRef = useRef("");
  const serviceCommittedTranscriptRef = useRef("");
  const serviceAudioDurationMsRef = useRef(0);
  const serviceCommittedEndMsRef = useRef(0);
  const serviceFinalSpeakerSegmentsRef = useRef<Record<string, unknown>[]>([]);
  const servicePartialCommitTimerRef = useRef<number | null>(null);
  const voiceCaptureOwnerIdRef = useRef(createVoiceCaptureOwnerId());
  const recordingRef = useRef(false);
  const transcriptFlushTimerRef = useRef<number | null>(null);
  const transcriptMaxFlushTimerRef = useRef<number | null>(null);
  const transcriptSegmentSeqRef = useRef(0);
  const browserFinalTranscriptBufferRef = useRef("");
  const browserSpeechReceivedFinalRef = useRef(false);
  const browserSpeechHadErrorRef = useRef(false);
  const browserSpeechStopRequestedRef = useRef(false);
  const browserSpeechRestartTimerRef = useRef<number | null>(null);
  const browserSpeechStopFinalizeTimerRef = useRef<number | null>(null);
  const browserSpeechMediaCleanupRef = useRef<(() => void) | null>(null);
  const browserSpeechTransientErrorCountRef = useRef(0);
  const pendingPromptRequestIdRef = useRef("");
  const pendingAskRequestIdRef = useRef("");
  const pendingPromptComposerHashRef = useRef("");
  const lastVoiceLedgerSignalRef = useRef("");
  const dismissedVoiceReplyKeysRef = useRef<Set<string>>(new Set());
  const localVoiceReplyRequestIdsRef = useRef<Set<string>>(new Set());
  const viewedDocumentPathRef = useRef("");
  const captureTargetDocumentPathRef = useRef("");
  const documentBaseTitleRef = useRef("");
  const documentBaseContentRef = useRef("");
  const documentTitleDraftRef = useRef("");
  const documentDraftRef = useRef("");
  const voiceStreamItemIdRef = useRef("");
  const archivedDocumentIdsRef = useRef<Set<string>>(new Set());
  const documentUpdatedSignatureByPathRef = useRef<Map<string, string>>(new Map());
  const [open, setOpen] = useState(false);
  const [showAssistantModeMenu, setShowAssistantModeMenu] = useState(false);
  const [showAssistantLanguageMenu, setShowAssistantLanguageMenu] = useState(false);
  const [loading, setLoading] = useState(false);
  const [actionBusy, setActionBusy] = useState<"" | "enable" | "transcribe" | "save_doc" | "new_doc" | "instruct_doc" | "instruct_ask" | "archive_doc" | "clear_ask">("");
  const [recognitionLanguageSaving, setRecognitionLanguageSaving] = useState(false);
  const [assistant, setAssistant] = useState<BuiltinAssistant | null>(null);
  const [serviceRuntimesById, setServiceRuntimesById] = useState<Record<string, AssistantServiceRuntime>>({});
  const [documents, setDocuments] = useState<AssistantVoiceDocument[]>([]);
  const [viewedDocumentPath, setViewedDocumentPath] = useState("");
  const [captureTargetDocumentPath, setCaptureTargetDocumentPath] = useState("");
  const [documentTitleDraft, setDocumentTitleDraft] = useState("");
  const [documentDraft, setDocumentDraft] = useState("");
  const [documentBaseTitle, setDocumentBaseTitle] = useState("");
  const [documentBaseContent, setDocumentBaseContent] = useState("");
  const [documentEditing, setDocumentEditing] = useState(false);
  const [voiceWorkspaceView, setVoiceWorkspaceView] = useState<VoiceWorkspaceView>("document");
  const [documentRemoteChanged, setDocumentRemoteChanged] = useState(false);
  const [creatingDocument, setCreatingDocument] = useState(false);
  const [newDocumentTitleDraft, setNewDocumentTitleDraft] = useState("");
  const [documentInstruction, setDocumentInstruction] = useState("");
  const [recording, setRecording] = useState(false);
  const [speechError, setSpeechError] = useState("");
  const [speechSupported, setSpeechSupported] = useState(() => !getBrowserSpeechSupportIssue());
  const [serviceAudioSupported, setServiceAudioSupported] = useState(() => mediaRecorderSupported());
  const [audioDevices, setAudioDevices] = useState<MediaDeviceInfo[]>([]);
  const [selectedAudioDeviceId, setSelectedAudioDeviceId] = useState("");
  const [pendingPromptRequestId, setPendingPromptRequestId] = useState("");
  const [pendingAskRequestId, setPendingAskRequestId] = useState("");
  const [pendingPromptDraft, setPendingPromptDraft] = useState<AssistantVoicePromptDraft | null>(null);
  const [askFeedbackItems, setAskFeedbackItems] = useState<AssistantVoiceAskFeedback[]>([]);
  const [askFeedbackClockMs, setAskFeedbackClockMs] = useState(() => Date.now());
  const [liveTranscriptPreview, setLiveTranscriptPreview] = useState<VoiceTranscriptPreview | null>(null);
  const [voiceStreamItems, setVoiceStreamItems] = useState<VoiceStreamItem[]>([]);
  const [documentActivityItems, setDocumentActivityItems] = useState<VoiceDocumentActivityItem[]>([]);
  const [diarizationActivityItems, setDiarizationActivityItems] = useState<VoiceDiarizationActivityItem[]>([]);
  const [activityClockMs, setActivityClockMs] = useState(() => Date.now());
  const [voiceReplyBubbleRequestId, setVoiceReplyBubbleRequestId] = useState("");
  const [copiedVoiceReplyRequestId, setCopiedVoiceReplyRequestId] = useState("");
  const latestVoiceLedgerEvent = useGroupStore((state): LedgerEvent | null => {
    const gid = String(selectedGroupId || "").trim();
    const events = gid ? (state.chatByGroup[gid]?.events || []) : [];
    for (let index = events.length - 1; index >= 0; index -= 1) {
      const event = events[index];
      const kind = String(event?.kind || "").trim();
      if (!kind.startsWith("assistant.voice.")) continue;
      return event;
    }
    return null;
  });

  useEffect(() => {
    recordingRef.current = recording;
    if (recording) {
      voiceStreamItemIdRef.current = "";
      setVoiceWorkspaceView("stream");
    }
  }, [recording]);

  useEffect(() => {
    const channel = openVoiceCaptureChannel();
    if (!channel) return undefined;
    const handleMessage = (event: MessageEvent<VoiceCaptureChannelMessage>) => {
      const message = event.data || {};
      if (message.type !== "probe") return;
      if (String(message.ownerId || "") !== voiceCaptureOwnerIdRef.current) return;
      if (!recordingRef.current) return;
      channel.postMessage({
        type: "alive",
        ownerId: voiceCaptureOwnerIdRef.current,
        groupId: selectedGroupId,
        sentAt: Date.now(),
      } satisfies VoiceCaptureChannelMessage);
    };
    channel.addEventListener("message", handleMessage);
    return () => {
      channel.removeEventListener("message", handleMessage);
      channel.close();
    };
  }, [selectedGroupId]);

  useEffect(() => {
    const releaseCurrentLock = () => releaseVoiceCaptureLock(voiceCaptureOwnerIdRef.current);
    window.addEventListener("pagehide", releaseCurrentLock);
    window.addEventListener("beforeunload", releaseCurrentLock);
    return () => {
      window.removeEventListener("pagehide", releaseCurrentLock);
      window.removeEventListener("beforeunload", releaseCurrentLock);
      releaseCurrentLock();
    };
  }, []);

  const activeDocument = useMemo(() => {
    const path = String(viewedDocumentPath || "").trim();
    if (path) {
      const match = findVoiceDocument(documents, path);
      if (match) return match;
    }
    return documents.find((document) => String(document.status || "active") === "active") || null;
  }, [viewedDocumentPath, documents]);
  const activeDocumentWritePath = useMemo(() => voiceDocumentPath(activeDocument), [activeDocument]);
  const activeDocumentTitle = String(activeDocument?.title || "").trim();
  const documentDisplayTitle =
    activeDocumentTitle ||
    documentTitleDraft.trim() ||
    t("voiceSecretaryWorkdocTitle", { defaultValue: "Voice Secretary workdoc" });
  const documentHasUnsavedEdits = documentDraft !== documentBaseContent;
  const assistantEnabled = !!assistant?.enabled;
  const recognitionBackend = String(assistant?.config?.recognition_backend || "browser_asr").trim();
  const configuredRecognitionLanguage = String(assistant?.config?.recognition_language || "mixed").trim() || "mixed";
  const effectiveRecognitionLanguage = configuredRecognitionLanguage === "auto"
    ? (typeof navigator !== "undefined" && navigator.language ? navigator.language : "en-US")
    : configuredRecognitionLanguage;
  const browserRecognitionLanguage =
    configuredRecognitionLanguage === "mixed"
      ? (typeof navigator !== "undefined" && navigator.language ? navigator.language : "zh-CN")
      : effectiveRecognitionLanguage;
  const voiceLanguageOptions = useMemo(
    () => voiceLanguageOptionValues(configuredRecognitionLanguage),
    [configuredRecognitionLanguage],
  );
  const voiceLanguageLabel = useCallback((value: string) => {
    switch (value) {
      case "mixed":
        return t("voiceSecretaryLanguageMixed", { defaultValue: "Mixed" });
      case "auto":
        return t("voiceSecretaryLanguageAuto", { defaultValue: "System default" });
      case "zh-CN":
        return t("voiceSecretaryLanguageChinese", { defaultValue: "Chinese" });
      case "en-US":
        return t("voiceSecretaryLanguageEnglish", { defaultValue: "English" });
      case "ja-JP":
        return t("voiceSecretaryLanguageJapanese", { defaultValue: "Japanese" });
      case "ko-KR":
        return t("voiceSecretaryLanguageKorean", { defaultValue: "Korean" });
      case "fr-FR":
        return t("voiceSecretaryLanguageFrench", { defaultValue: "French" });
      case "de-DE":
        return t("voiceSecretaryLanguageGerman", { defaultValue: "German" });
      case "es-ES":
        return t("voiceSecretaryLanguageSpanish", { defaultValue: "Spanish" });
      default:
        return value;
    }
  }, [t]);
  const voiceLanguageShortLabel = useCallback((value: string) => {
    switch (value) {
      case "mixed":
        return t("voiceSecretaryLanguageShortMixed", { defaultValue: "MIX" });
      case "auto":
        return t("voiceSecretaryLanguageShortAuto", { defaultValue: "SYS" });
      case "zh-CN":
        return t("voiceSecretaryLanguageShortChinese", { defaultValue: "ZH" });
      case "en-US":
        return t("voiceSecretaryLanguageShortEnglish", { defaultValue: "EN" });
      case "ja-JP":
        return t("voiceSecretaryLanguageShortJapanese", { defaultValue: "JA" });
      case "ko-KR":
        return t("voiceSecretaryLanguageShortKorean", { defaultValue: "KO" });
      case "fr-FR":
        return t("voiceSecretaryLanguageShortFrench", { defaultValue: "FR" });
      case "de-DE":
        return t("voiceSecretaryLanguageShortGerman", { defaultValue: "DE" });
      case "es-ES":
        return t("voiceSecretaryLanguageShortSpanish", { defaultValue: "ES" });
      default:
        return String(value || "").slice(0, 2).toUpperCase() || "ASR";
    }
  }, [t]);
  const configuredRecognitionLanguageLabel = voiceLanguageLabel(configuredRecognitionLanguage);
  const configuredRecognitionLanguageShortLabel = voiceLanguageShortLabel(configuredRecognitionLanguage);
  const autoDocumentQuietMs = useMemo(
    () => numberFromUnknown(
      assistant?.config?.auto_document_quiet_ms,
      BROWSER_SPEECH_MIN_QUIET_MS,
      BROWSER_SPEECH_MIN_QUIET_MS,
      60_000,
    ),
    [assistant?.config?.auto_document_quiet_ms],
  );
  const effectiveAutoDocumentQuietMs = useMemo(() => {
    if (captureMode !== "instruction" && captureMode !== "prompt") return autoDocumentQuietMs;
    return Math.max(BROWSER_SPEECH_MIN_QUIET_MS, autoDocumentQuietMs - BROWSER_SPEECH_FAST_MODE_QUIET_REDUCTION_MS);
  }, [autoDocumentQuietMs, captureMode]);
  const autoDocumentMaxWindowMs = useMemo(
    () => numberFromUnknown(
      assistant?.config?.auto_document_max_window_seconds,
      BROWSER_SPEECH_MAX_WINDOW_FALLBACK_MS / 1000,
      BROWSER_SPEECH_MIN_MAX_WINDOW_MS / 1000,
      300,
    ) * 1000,
    [assistant?.config?.auto_document_max_window_seconds],
  );
  const browserSpeechReady = recognitionBackend === "browser_asr";
  const serviceReadiness = resolveVoiceServiceReadiness({
    assistant,
    serviceRuntimesById,
    streamingRuntimeId: STREAMING_ASR_RUNTIME_ID,
  });
  const serviceAsrReady = serviceReadiness.serviceAsrReady;
  const serviceHealth = recordFromUnknown(assistant?.health?.service);
  const streamingRuntimeReady = serviceReadiness.streamingRuntimeReady;
  const serviceAsrConfigured = serviceReadiness.serviceAsrConfigured;
  const browserSpeechSupportIssue = browserSpeechReady ? getBrowserSpeechSupportIssue() : "";
  const serviceAudioSupportIssue = serviceAsrReady ? getBrowserAudioSupportIssue() : "";
  const getBrowserSpeechIssueMessage = useCallback((issue: BrowserSpeechSupportIssue) => {
    if (issue === "unsupported") {
      return t("voiceSecretaryBrowserUnsupported", {
        defaultValue: "Browser speech recognition is not available in this browser page. Try another current browser.",
      });
    }
    return "";
  }, [t]);
  const getAudioSupportIssueMessage = useCallback((issue: BrowserAudioSupportIssue) => {
    if (issue === "secure_context") {
      return t("voiceSecretarySecureContextRequired", {
        defaultValue: "Microphone capture requires a secure browser context. Open this page through localhost or HTTPS, not a raw WSL IP over HTTP.",
      });
    }
    if (issue === "get_user_media") {
      return t("voiceSecretaryGetUserMediaUnavailable", {
        defaultValue: "This browser page cannot access the microphone API. Open it through localhost or HTTPS and allow microphone access.",
      });
    }
    return "";
  }, [t]);
  const getAudioCaptureErrorMessage = useCallback((error: unknown) => {
    const errorName = typeof error === "object" && error && "name" in error ? String((error as { name?: unknown }).name || "") : "";
    if (errorName === "NotAllowedError" || errorName === "SecurityError") {
      return {
        message: t("voiceSecretaryMicPermissionBlocked", {
          defaultValue: "Microphone permission is blocked. Allow microphone access for this site in the browser, then try again.",
        }),
        resetSelectedDevice: false,
      };
    }
    if (errorName === "NotFoundError" || errorName === "DevicesNotFoundError") {
      return {
        message: t("voiceSecretaryMicNotFound", {
          defaultValue: "No microphone was found or the selected microphone is unavailable.",
        }),
        resetSelectedDevice: false,
      };
    }
    if (errorName === "NotReadableError" || errorName === "TrackStartError") {
      return {
        message: t("voiceSecretaryMicBusyOrUnavailable", {
          defaultValue: "The microphone could not be started. Check whether another app is using it or the OS blocked access.",
        }),
        resetSelectedDevice: false,
      };
    }
    if (errorName === "OverconstrainedError" || errorName === "ConstraintNotSatisfiedError") {
      return {
        message: t("voiceSecretarySelectedMicUnavailable", {
          defaultValue: "The selected microphone is unavailable. Reset to the system default microphone and try again.",
        }),
        resetSelectedDevice: true,
      };
    }
    return {
      message: t("voiceSecretaryAudioCaptureFailed", { defaultValue: "Audio capture failed." }),
      resetSelectedDevice: false,
    };
  }, [t]);
  const controlDisabled = disabled || !selectedGroupId || busy === "send";
  const isAssistantRow = variant === "assistantRow";
  const selectedAudioDeviceLabel = useMemo(() => {
    if (!selectedAudioDeviceId) return SERVICE_DEFAULT_MIC_LABEL;
    const index = audioDevices.findIndex((device) => device.deviceId === selectedAudioDeviceId);
    const device = index >= 0 ? audioDevices[index] : null;
    return device?.label || `microphone_${index + 1 || "selected"}`;
  }, [audioDevices, selectedAudioDeviceId]);
  const captureTargetDocument = useMemo(() => {
    const targetPath = String(captureTargetDocumentPath || "").trim();
    if (targetPath) {
      const match = findVoiceDocument(documents, targetPath);
      if (match) return match;
    }
    return activeDocument;
  }, [activeDocument, captureTargetDocumentPath, documents]);
  const captureTargetDocumentTitle =
    String(captureTargetDocument?.title || "").trim() || documentDisplayTitle;
  const effectiveCaptureTargetDocumentPath =
    voiceDocumentPath(captureTargetDocument) || String(captureTargetDocumentPath || "").trim();

  useEffect(() => {
    viewedDocumentPathRef.current = viewedDocumentPath;
  }, [viewedDocumentPath]);

  useEffect(() => {
    captureTargetDocumentPathRef.current = captureTargetDocumentPath;
  }, [captureTargetDocumentPath]);

  useEffect(() => {
    documentTitleDraftRef.current = documentTitleDraft;
    documentDraftRef.current = documentDraft;
    documentBaseTitleRef.current = documentBaseTitle;
    documentBaseContentRef.current = documentBaseContent;
  }, [documentBaseContent, documentBaseTitle, documentDraft, documentTitleDraft]);

  const loadDocumentDraft = useCallback((document: AssistantVoiceDocument | null) => {
    const title = String(document?.title || "");
    const content = String(document?.content || "");
    documentTitleDraftRef.current = title;
    documentDraftRef.current = content;
    documentBaseTitleRef.current = title;
    documentBaseContentRef.current = content;
    setDocumentTitleDraft(title);
    setDocumentDraft(content);
    setDocumentBaseTitle(title);
    setDocumentBaseContent(content);
    setDocumentRemoteChanged(false);
  }, []);

  const updateDocumentDraft = useCallback((value: string) => {
    documentDraftRef.current = value;
    setDocumentDraft(value);
  }, []);

  const pushDocumentActivity = useCallback((
    item: Omit<VoiceDocumentActivityItem, "id" | "createdAt" | "mode"> & {
      createdAt?: number;
      mode?: VoiceSecretaryCaptureMode;
    },
  ) => {
    const createdAt = item.createdAt || Date.now();
    const id = `doc-activity-${createdAt.toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
    setDocumentActivityItems((prev) => {
      const nextPath = String(item.documentPath || "").trim();
      const nextPreview = normalizeBrowserTranscriptChunk(String(item.preview || ""));
      const duplicate = prev.some((existing) => (
        existing.status === item.status
        && String(existing.documentPath || "").trim() === nextPath
        && (
          normalizeBrowserTranscriptChunk(String(existing.preview || "")) === nextPreview
          || (item.status === "updated" && !nextPreview)
        )
        && Math.abs(existing.createdAt - createdAt) <= 3000
      ));
      if (duplicate) return prev;
      return [{
        id,
        status: item.status,
        documentTitle: item.documentTitle,
        documentPath: item.documentPath,
        preview: item.preview,
        mode: item.mode || "document",
        createdAt,
      },
      ...prev].slice(0, 20);
    });
    setActivityClockMs(createdAt);
  }, []);

  const pushDocumentUpdatedActivity = useCallback((
    item: Omit<VoiceDocumentActivityItem, "id" | "createdAt" | "mode" | "status"> & {
      createdAt?: number;
      mode?: VoiceSecretaryCaptureMode;
    },
  ) => {
    pushDocumentActivity({
      ...item,
      status: "updated",
    });
  }, [pushDocumentActivity]);

  const updateLiveTranscriptPreview = useCallback((text: string, phase: VoiceTranscriptPreviewPhase, opts?: { startMs?: number; endMs?: number }) => {
    const clean = normalizeBrowserTranscriptChunk(text);
    if (!clean) return;
    const now = Date.now();
    const streamId = voiceStreamItemIdRef.current || `voice-stream-${now.toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
    voiceStreamItemIdRef.current = streamId;
    const pendingFinalText = normalizeBrowserTranscriptChunk(browserFinalTranscriptBufferRef.current);
    const preview = createVoiceTranscriptPreview({
      id: streamId,
      cleanText: clean,
      phase,
      pendingFinalText,
      metadata: {
        mode: captureMode,
        documentTitle: captureTargetDocumentTitle,
        documentPath: effectiveCaptureTargetDocumentPath,
        language: effectiveRecognitionLanguage,
      },
      timing: opts,
      now,
    });
    setLiveTranscriptPreview(preview);
    setVoiceStreamItems((prev) => upsertLiveVoiceStreamItem(prev, preview));
    setActivityClockMs(now);
  }, [captureMode, effectiveCaptureTargetDocumentPath, captureTargetDocumentTitle, effectiveRecognitionLanguage]);

  const pushVoiceStreamMessage = useCallback((text: string, opts?: { startMs?: number; endMs?: number; documentPath?: string }) => {
    const clean = normalizeBrowserTranscriptChunk(text);
    if (!clean) return;
    const now = Date.now();
    const liveId = voiceStreamItemIdRef.current;
    voiceStreamItemIdRef.current = "";
    setLiveTranscriptPreview(null);
    const documentPath = String(opts?.documentPath || effectiveCaptureTargetDocumentPath || "").trim();
    const documentTitle = documentPath
      ? String(findVoiceDocument(documents, documentPath)?.title || "").trim() || captureTargetDocumentTitle
      : captureTargetDocumentTitle;
    const item = createVoiceStreamMessage({
      id: `voice-message-${now.toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
      cleanText: clean,
      metadata: {
        mode: captureMode,
        documentTitle,
        documentPath,
        language: effectiveRecognitionLanguage,
      },
      timing: opts,
      now,
    });
    setVoiceStreamItems((prev) => appendFinalVoiceStreamItem(prev, item, liveId));
    setActivityClockMs(now);
  }, [captureMode, documents, effectiveCaptureTargetDocumentPath, captureTargetDocumentTitle, effectiveRecognitionLanguage]);

  const loadAudioDevices = useCallback(async () => {
    if (typeof navigator === "undefined" || !navigator.mediaDevices?.enumerateDevices) {
      setServiceAudioSupported(false);
      setAudioDevices([]);
      return;
    }
    setServiceAudioSupported(mediaRecorderSupported());
    try {
      const devices = await navigator.mediaDevices.enumerateDevices();
      const inputs = devices.filter((device) => device.kind === "audioinput");
      setAudioDevices(inputs);
      setSelectedAudioDeviceId((current) => {
        if (!current || inputs.some((device) => device.deviceId === current)) return current;
        return "";
      });
    } catch {
      setAudioDevices([]);
    }
  }, []);

  const refreshAssistant = useCallback(async (opts?: { quiet?: boolean }) => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    const seq = ++refreshSeq.current;
    const quiet = Boolean(opts?.quiet);
    if (!quiet) setLoading(true);
    try {
      const promptRequestId = String(pendingPromptRequestIdRef.current || "").trim();
      const resp = await fetchAssistant(gid, "voice_secretary", { promptRequestId });
      if (seq !== refreshSeq.current) return;
      if (!resp.ok) {
        if (!quiet) showError(resp.error.message);
        return;
      }
      setAssistant(resp.result.assistant || null);
      setServiceRuntimesById(resp.result.service_runtimes_by_id || {});
      const promptDraft = resp.result.prompt_draft || null;
      if (promptDraft && pendingPromptRequestIdRef.current && promptDraft.request_id === pendingPromptRequestIdRef.current) {
        setPendingPromptDraft(promptDraft);
      }
      const nextAskFeedbackItems = resp.result.ask_requests || [];
      setAskFeedbackItems(nextAskFeedbackItems);
      const currentAskRequestId = String(pendingAskRequestIdRef.current || "").trim();
      if (currentAskRequestId) {
        const currentAsk = nextAskFeedbackItems.find((item) => item.request_id === currentAskRequestId);
        if (currentAsk && !["pending", "working"].includes(String(currentAsk.status || "").trim().toLowerCase())) {
          pendingAskRequestIdRef.current = "";
          setPendingAskRequestId("");
        }
      }
      const nextDocuments = resp.result.documents || [];
      const previousDocumentSignatures = documentUpdatedSignatureByPathRef.current;
      const nextDocumentSignatures = new Map<string, string>();
      nextDocuments.forEach((document) => {
        const docKey = voiceDocumentPath(document);
        if (!docKey) return;
        const signature = [
          String(document.updated_at || ""),
          String(document.content_sha256 || ""),
          String(document.revision_count || ""),
          String(document.content_chars || ""),
        ].join(":");
        if (!signature.replace(/:/g, "")) return;
        const previousSignature = previousDocumentSignatures.get(docKey);
        if (previousSignature && previousSignature !== signature) {
          pushDocumentUpdatedActivity({
            documentTitle: String(document.title || "").trim(),
            documentPath: docKey,
          });
        }
        nextDocumentSignatures.set(docKey, signature);
      });
      documentUpdatedSignatureByPathRef.current = nextDocumentSignatures;
      setDocuments(nextDocuments);
      const serverCaptureTargetPath = resolveVoiceDocumentPath(nextDocuments, String(
        resp.result.capture_target_document_path ||
          resp.result.active_document_path ||
          "",
      ).trim()) || resolveVoiceDocumentPath(
        nextDocuments,
        String(resp.result.capture_target_document_id || resp.result.active_document_id || "").trim(),
      );
      const currentCaptureTargetPath = String(captureTargetDocumentPathRef.current || "").trim();
      const currentCaptureTargetExists = currentCaptureTargetPath
        ? Boolean(findVoiceDocument(nextDocuments, currentCaptureTargetPath))
        : false;
      const resolvedCaptureTargetPath = serverCaptureTargetPath
        || (currentCaptureTargetExists ? currentCaptureTargetPath : "");
      captureTargetDocumentPathRef.current = resolvedCaptureTargetPath;
      setCaptureTargetDocumentPath(resolvedCaptureTargetPath);
      const serverViewedPath = resolveVoiceDocumentPath(nextDocuments, String(
        resp.result.active_document_path ||
          resp.result.capture_target_document_path ||
          "",
      ).trim()) || resolveVoiceDocumentPath(
        nextDocuments,
        String(resp.result.active_document_id || resp.result.capture_target_document_id || "").trim(),
      );
      const currentViewedPath = String(viewedDocumentPathRef.current || "").trim();
      const currentViewedDocument = findVoiceDocument(nextDocuments, currentViewedPath);
      const serverViewedDocument = findVoiceDocument(nextDocuments, serverViewedPath);
      const nextActiveDocument = currentViewedDocument || serverViewedDocument || nextDocuments[0] || null;
      const nextViewedPath = voiceDocumentPath(nextActiveDocument);
      setViewedDocumentPath(nextViewedPath);
      if (nextActiveDocument) {
        const serverTitle = String(nextActiveDocument.title || "");
        const serverContent = String(nextActiveDocument.content || "");
        const localDirty = documentDraftRef.current !== documentBaseContentRef.current;
        const sameDocument = currentViewedPath
          ? voiceDocumentMatches(nextActiveDocument, currentViewedPath)
          : false;
        const serverChangedFromBase =
          serverTitle !== documentBaseTitleRef.current || serverContent !== documentBaseContentRef.current;
        if (!localDirty || !sameDocument) {
          loadDocumentDraft(nextActiveDocument);
        } else if (serverChangedFromBase) {
          setDocumentRemoteChanged(true);
        }
      } else {
        loadDocumentDraft(null);
      }
      if (!recordingRef.current) {
        const sessionResp = await fetchLatestVoiceAssistantMeetingSession(gid);
        if (seq !== refreshSeq.current) return;
        if (sessionResp.ok && sessionResp.result.session) {
          const restoredItems = voiceStreamItemsFromMeetingSession(sessionResp.result.session);
          if (restoredItems.length) {
            setVoiceStreamItems((prev) => (prev.length ? prev : restoredItems));
          }
          const restoredDiarization = diarizationItemFromMeetingSession(sessionResp.result.session);
          if (restoredDiarization) {
            setDiarizationActivityItems((prev) => mergeRestoredDiarizationActivity(prev, restoredDiarization));
          }
        }
      }
    } finally {
      if (seq === refreshSeq.current && !quiet) setLoading(false);
    }
  }, [loadDocumentDraft, pushDocumentUpdatedActivity, selectedGroupId, showError]);

  useEffect(() => {
    if (!open) return;
    void refreshAssistant();
  }, [open, refreshAssistant]);

  useEffect(() => {
    if (!latestVoiceLedgerEvent) return;
    if (!open) return;
    const eventId = String(latestVoiceLedgerEvent.id || "").trim();
    const eventKey = eventId || [
      String(latestVoiceLedgerEvent.kind || "").trim(),
      String(latestVoiceLedgerEvent.ts || "").trim(),
    ].join(":");
    if (!eventKey || eventKey === lastVoiceLedgerSignalRef.current) return;
    lastVoiceLedgerSignalRef.current = eventKey;
    const kind = String(latestVoiceLedgerEvent.kind || "").trim();
    const data = latestVoiceLedgerEvent.data && typeof latestVoiceLedgerEvent.data === "object"
      ? latestVoiceLedgerEvent.data as AssistantVoiceDocumentLedgerData
      : null;
    if (kind === "assistant.voice.document" && data) {
      const action = String(data.action || "").trim();
      const inputPreview = String(data.input_preview || "").trim();
      if (action === "input_appended" && inputPreview) {
        const createdAt = assistantVoiceTimestampMs(String(latestVoiceLedgerEvent.ts || "")) || Date.now();
        pushDocumentUpdatedActivity({
          documentTitle: String(data.title || "").trim(),
          documentPath: String(data.document_path || data.workspace_path || "").trim(),
          preview: inputPreview,
          createdAt,
          mode: "document",
        });
      }
    }
    if (kind === "assistant.voice.session") {
      void refreshAssistant({ quiet: true });
    }
  }, [
    latestVoiceLedgerEvent,
    open,
    pushDocumentUpdatedActivity,
    refreshAssistant,
  ]);

  useEffect(() => {
    const hasActiveAsk = askFeedbackItems.some((item) => isActiveAskFeedbackStatus(item.status));
    if (!hasActiveAsk) return undefined;
    if (typeof window === "undefined") return undefined;
    const timer = window.setInterval(() => {
      setAskFeedbackClockMs(Date.now());
    }, 15_000);
    return () => {
      window.clearInterval(timer);
    };
  }, [askFeedbackItems]);

  useEffect(() => {
    const hasActiveAsk = askFeedbackItems.some((item) => isActiveAskFeedbackStatus(item.status));
    const hasVisibleTranscript = liveTranscriptPreview
      ? recording || Date.now() - liveTranscriptPreview.updatedAt < VOICE_LIVE_TRANSCRIPT_VISIBLE_MS
      : false;
    const hasVisibleDocumentActivity = documentActivityItems.some((item) => Date.now() - item.createdAt < VOICE_DOCUMENT_ACTIVITY_VISIBLE_MS);
    if (!hasActiveAsk && !hasVisibleTranscript && !hasVisibleDocumentActivity && !pendingPromptRequestId) return undefined;
    if (typeof window === "undefined") return undefined;
    const timer = window.setInterval(() => {
      setActivityClockMs(Date.now());
    }, 1000);
    return () => {
      window.clearInterval(timer);
    };
  }, [askFeedbackItems, documentActivityItems, liveTranscriptPreview, pendingPromptRequestId, recording]);

  const acknowledgePromptDraft = useCallback(async (
    draft: AssistantVoicePromptDraft,
    status: "applied" | "dismissed" | "stale",
  ) => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid || !draft.request_id) return;
    const resp = await ackVoiceAssistantPromptDraft(gid, {
      requestId: draft.request_id,
      status,
      by: "user",
    });
    if (resp.ok && resp.result.assistant) setAssistant(resp.result.assistant);
  }, [selectedGroupId]);

  const applyPromptDraft = useCallback(async (draft: AssistantVoicePromptDraft) => {
    const text = String(draft.draft_text || "").trim();
    if (!text) return;
    pendingPromptRequestIdRef.current = "";
    pendingPromptComposerHashRef.current = "";
    setPendingPromptRequestId("");
    setPendingPromptDraft(null);
    const applyMode = promptDraftApplyMode(draft);
    onPromptDraft?.(text, { mode: applyMode });
    try {
      await acknowledgePromptDraft(draft, "applied");
    } catch {
      // Applying locally is the critical path; ack retry is non-critical.
    }
    showNotice({
      message: applyMode === "replace"
        ? t("voiceSecretaryPromptDraftReplaced", {
            defaultValue: "Refined prompt replaced the composer.",
          })
        : t("voiceSecretaryPromptDraftFilled", {
            defaultValue: "Refined prompt appended to the composer.",
          }),
    });
  }, [acknowledgePromptDraft, onPromptDraft, showNotice, t]);

  useEffect(() => {
    if (!pendingPromptDraft) return;
    const requested = String(pendingPromptRequestIdRef.current || pendingPromptRequestId || "").trim();
    if (!requested || pendingPromptDraft.request_id !== requested) return;
    void applyPromptDraft(pendingPromptDraft);
  }, [applyPromptDraft, pendingPromptDraft, pendingPromptRequestId]);

  useEffect(() => {
    if (!open || !serviceAsrReady) return;
    void loadAudioDevices();
  }, [loadAudioDevices, open, serviceAsrReady]);

  useEffect(() => {
    refreshSeq.current += 1;
    const recognition = recognitionRef.current;
    recognitionRef.current = null;
    abortBrowserSpeechRecognition(recognition);
    browserSpeechStopRequestedRef.current = true;
    browserSpeechTransientErrorCountRef.current = 0;
    const recorder = mediaRecorderRef.current;
    mediaRecorderRef.current = null;
    if (recorder && recorder.state !== "inactive") {
      recorder.onstop = null;
      try {
        recorder.stop();
      } catch {
        // ignore browser cleanup failure
      }
    }
    const cleanupBrowserSpeechMedia = browserSpeechMediaCleanupRef.current;
    browserSpeechMediaCleanupRef.current = null;
    if (cleanupBrowserSpeechMedia) cleanupBrowserSpeechMedia();
    stopMediaStream(mediaStreamRef.current);
    mediaStreamRef.current = null;
    mediaChunksRef.current = [];
    setOpen(false);
    setLoading(false);
    setActionBusy("");
    setRecognitionLanguageSaving(false);
    setAssistant(null);
    setDocuments([]);
    setViewedDocumentPath("");
    setCaptureTargetDocumentPath("");
    loadDocumentDraft(null);
    setDocumentEditing(false);
    setDocumentInstruction("");
    setRecording(false);
    setSpeechError("");
    setAudioDevices([]);
    setSelectedAudioDeviceId("");
    pendingPromptRequestIdRef.current = "";
    pendingAskRequestIdRef.current = "";
    pendingPromptComposerHashRef.current = "";
    dismissedVoiceReplyKeysRef.current.clear();
    localVoiceReplyRequestIdsRef.current.clear();
    setPendingPromptRequestId("");
    setPendingAskRequestId("");
    setPendingPromptDraft(null);
    setAskFeedbackItems([]);
    setLiveTranscriptPreview(null);
    setVoiceStreamItems([]);
    voiceStreamItemIdRef.current = "";
    setDocumentActivityItems([]);
    setActivityClockMs(Date.now());
    documentUpdatedSignatureByPathRef.current = new Map();
    setVoiceReplyBubbleRequestId("");
    setCopiedVoiceReplyRequestId("");
    releaseVoiceCaptureLock(voiceCaptureOwnerIdRef.current);
    if (transcriptFlushTimerRef.current !== null) {
      window.clearTimeout(transcriptFlushTimerRef.current);
      transcriptFlushTimerRef.current = null;
    }
    if (transcriptMaxFlushTimerRef.current !== null) {
      window.clearTimeout(transcriptMaxFlushTimerRef.current);
      transcriptMaxFlushTimerRef.current = null;
    }
    if (servicePartialCommitTimerRef.current !== null) {
      window.clearTimeout(servicePartialCommitTimerRef.current);
      servicePartialCommitTimerRef.current = null;
    }
    browserFinalTranscriptBufferRef.current = "";
    serviceLatestPartialTranscriptRef.current = "";
    serviceCommittedTranscriptRef.current = "";
    serviceAudioResamplerRef.current = null;
    serviceAudioDurationMsRef.current = 0;
    serviceCommittedEndMsRef.current = 0;
    serviceFinalSpeakerSegmentsRef.current = [];
    if (browserSpeechRestartTimerRef.current !== null) {
      window.clearTimeout(browserSpeechRestartTimerRef.current);
      browserSpeechRestartTimerRef.current = null;
    }
    if (browserSpeechStopFinalizeTimerRef.current !== null) {
      window.clearTimeout(browserSpeechStopFinalizeTimerRef.current);
      browserSpeechStopFinalizeTimerRef.current = null;
    }
  }, [loadDocumentDraft, selectedGroupId]);

  useEffect(() => {
    if (!selectedGroupId) return;
    void refreshAssistant({ quiet: true });
  }, [refreshAssistant, selectedGroupId]);

  const clearTranscriptFlushTimer = useCallback(() => {
    if (transcriptFlushTimerRef.current === null) return;
    window.clearTimeout(transcriptFlushTimerRef.current);
    transcriptFlushTimerRef.current = null;
  }, []);

  const clearTranscriptMaxFlushTimer = useCallback(() => {
    if (transcriptMaxFlushTimerRef.current === null) return;
    window.clearTimeout(transcriptMaxFlushTimerRef.current);
    transcriptMaxFlushTimerRef.current = null;
  }, []);

  const clearBrowserSpeechRestartTimer = useCallback(() => {
    if (browserSpeechRestartTimerRef.current === null) return;
    window.clearTimeout(browserSpeechRestartTimerRef.current);
    browserSpeechRestartTimerRef.current = null;
  }, []);

  const clearBrowserSpeechStopFinalizeTimer = useCallback(() => {
    if (browserSpeechStopFinalizeTimerRef.current === null) return;
    window.clearTimeout(browserSpeechStopFinalizeTimerRef.current);
    browserSpeechStopFinalizeTimerRef.current = null;
  }, []);

  const clearServicePartialCommitTimer = useCallback(() => {
    if (servicePartialCommitTimerRef.current === null) return;
    window.clearTimeout(servicePartialCommitTimerRef.current);
    servicePartialCommitTimerRef.current = null;
  }, []);

  const clearBrowserSpeechMediaHandlers = useCallback(() => {
    const cleanup = browserSpeechMediaCleanupRef.current;
    browserSpeechMediaCleanupRef.current = null;
    if (cleanup) cleanup();
  }, []);

  const applyTranscriptAppendResult = useCallback((result: AssistantVoiceTranscriptSegmentResult) => {
    if (result.assistant) setAssistant(result.assistant);
    if ((result.document_updated || result.input_event_created) && result.document) {
      const document = result.document;
      const docPath = voiceDocumentPath(document);
      if (docPath && !archivedDocumentIdsRef.current.has(docPath) && String(document.status || "active").trim() !== "archived") {
        setDocuments((prev) => {
          const index = prev.findIndex((item) => voiceDocumentPath(item) === docPath);
          if (index < 0) return [document, ...prev];
          const next = [...prev];
          next[index] = document;
          return next;
        });
        if (!captureTargetDocumentPathRef.current) {
          captureTargetDocumentPathRef.current = docPath;
          setCaptureTargetDocumentPath(docPath);
        }
        const viewingDocumentPath = String(viewedDocumentPathRef.current || "").trim();
        const localDirty = documentDraftRef.current !== documentBaseContentRef.current;
        if (!viewingDocumentPath || viewingDocumentPath === docPath) {
          setViewedDocumentPath(docPath);
          if (!localDirty || !viewingDocumentPath) loadDocumentDraft(document);
          else setDocumentRemoteChanged(true);
        }
      }
    }
  }, [loadDocumentDraft]);

  const applyDocumentMutationResult = useCallback((document: AssistantVoiceDocument | undefined, assistantNext?: BuiltinAssistant) => {
    if (assistantNext) setAssistant(assistantNext);
    if (!document) return;
    const docPath = voiceDocumentPath(document);
    if (!docPath) return;
    setDocuments((prev) => {
      const index = prev.findIndex((item) => voiceDocumentPath(item) === docPath);
      if (index < 0) return [document, ...prev];
      const next = [...prev];
      next[index] = document;
      return next;
    });
    if (!viewedDocumentPathRef.current || viewedDocumentPathRef.current === docPath) {
      setViewedDocumentPath(docPath);
      loadDocumentDraft(document);
    }
  }, [loadDocumentDraft]);

  const appendTranscriptSegment = useCallback(async (
    text: string,
    opts?: {
      flush?: boolean;
      triggerKind?: string;
      source?: string;
      inputDeviceLabel?: string;
      documentPath?: string;
      startMs?: number;
      endMs?: number;
      speakerSegments?: Record<string, unknown>[];
    },
  ) => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid || !assistantEnabled) return;
    const cleanText = String(text || "").trim();
    const flush = Boolean(opts?.flush);
    if (!cleanText && !flush) return;
    const targetDocumentPath = String(opts?.documentPath || captureTargetDocumentPathRef.current || "").trim();
    const segmentSeq = transcriptSegmentSeqRef.current + 1;
    transcriptSegmentSeqRef.current = segmentSeq;
    try {
      const resp = await appendVoiceAssistantTranscriptSegment(gid, {
        sessionId: voiceCaptureOwnerIdRef.current,
        segmentId: cleanText ? `seg-${segmentSeq}` : "",
        documentPath: targetDocumentPath,
        text: cleanText,
        language: effectiveRecognitionLanguage,
        isFinal: true,
        flush,
        startMs: opts?.startMs,
        endMs: opts?.endMs,
        trigger: {
          mode: "meeting",
          trigger_kind: opts?.triggerKind || (flush ? "push_to_talk_stop" : "meeting_window"),
          capture_mode: serviceAsrReady ? "service" : "browser",
          recognition_backend: opts?.source || recognitionBackend,
          client_session_id: voiceCaptureOwnerIdRef.current,
          input_device_label: opts?.inputDeviceLabel || (serviceAsrReady ? selectedAudioDeviceLabel : BROWSER_DEFAULT_MIC_LABEL),
          language: effectiveRecognitionLanguage,
          document_path: targetDocumentPath,
          speaker_segments: opts?.speakerSegments || [],
        },
        by: "user",
      });
      if (!resp.ok) {
        showError(resp.error.message);
        return;
      }
      applyTranscriptAppendResult(resp.result);
    } catch {
      showError(t("voiceSecretaryTranscriptAppendFailed", {
        defaultValue: "Failed to save Voice Secretary transcript segment.",
      }));
    }
  }, [
    applyTranscriptAppendResult,
    assistantEnabled,
    effectiveRecognitionLanguage,
    recognitionBackend,
    selectedAudioDeviceLabel,
    selectedGroupId,
    serviceAsrReady,
    showError,
    t,
  ]);

  const sendInstructionTranscript = useCallback(async (
    text: string,
    opts?: { triggerKind?: string },
  ): Promise<boolean> => {
    const gid = String(selectedGroupId || "").trim();
    const instruction = normalizeBrowserTranscriptChunk(text);
    if (!gid || !assistantEnabled || !instruction) return false;
    const requestId = `voice-ask-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
    const currentDocumentPath = String(captureTargetDocumentPathRef.current || activeDocumentWritePath || viewedDocumentPath || "").trim();
    try {
      const resp = await appendVoiceAssistantInput(gid, {
        kind: "voice_instruction",
        instruction,
        requestId,
        trigger: {
          trigger_kind: opts?.triggerKind || "voice_instruction",
          mode: "voice_instruction",
          target_kind: "secretary",
          current_document_path: currentDocumentPath,
          recognition_backend: recognitionBackend,
          language: effectiveRecognitionLanguage,
        },
        by: "user",
      });
      if (!resp.ok) {
        showError(resp.error.message);
        return false;
      }
      const nextRequestId = String(resp.result.request_id || requestId).trim();
      localVoiceReplyRequestIdsRef.current.add(nextRequestId);
      pendingAskRequestIdRef.current = nextRequestId;
      setPendingAskRequestId(nextRequestId);
      setAskFeedbackItems((prev) => [
        {
          request_id: nextRequestId,
          status: "pending",
          request_text: instruction,
          request_preview: instruction.slice(0, 240),
          target_kind: "secretary",
          created_at: new Date().toISOString(),
          updated_at: new Date().toISOString(),
        },
        ...prev.filter((item) => item.request_id !== nextRequestId),
      ].slice(0, 10));
      setLiveTranscriptPreview(null);
      applyDocumentMutationResult(resp.result.document, resp.result.assistant);
      showNotice({
        message: t("voiceSecretaryDocumentInstructionQueued", { defaultValue: "Request sent to Voice Secretary." }),
      });
      void refreshAssistant({ quiet: true });
      return true;
    } catch {
      showError(t("voiceSecretaryDocumentInstructionFailed", { defaultValue: "Failed to send the request to Voice Secretary." }));
      return false;
    }
  }, [
    viewedDocumentPath,
    activeDocumentWritePath,
    applyDocumentMutationResult,
    assistantEnabled,
    effectiveRecognitionLanguage,
    recognitionBackend,
    refreshAssistant,
    selectedGroupId,
    showError,
    showNotice,
    t,
  ]);

  const requestPromptRefine = useCallback(async (
    text: string,
    triggerKind = "prompt_refine",
    opts?: { operation?: "append_to_composer_end" | "replace_with_refined_prompt" },
  ) => {
    const gid = String(selectedGroupId || "").trim();
    const voiceTranscript = normalizeBrowserTranscriptChunk(text);
    const snapshot = String(composerText || "");
    if (!gid || !assistantEnabled || (!voiceTranscript && !snapshot.trim())) return;
    const operation = opts?.operation || "append_to_composer_end";
    const snapshotHash = hashComposerSnapshot(snapshot);
    const existingRequestId = String(pendingPromptRequestIdRef.current || pendingPromptRequestId || "").trim();
    const requestId = existingRequestId || `voice-prompt-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
    pendingPromptRequestIdRef.current = requestId;
    pendingPromptComposerHashRef.current = snapshotHash;
    setPendingPromptRequestId(requestId);
    try {
      const resp = await appendVoiceAssistantInput(gid, {
        kind: "prompt_refine",
        voiceTranscript,
        composerText: snapshot,
        requestId,
        operation,
        composerSnapshotHash: snapshotHash,
        composerContext,
        language: effectiveRecognitionLanguage,
        trigger: {
          trigger_kind: triggerKind,
          mode: "prompt",
          recognition_backend: recognitionBackend,
          language: effectiveRecognitionLanguage,
        },
        by: "user",
      });
      if (!resp.ok) {
        pendingPromptRequestIdRef.current = "";
        pendingPromptComposerHashRef.current = "";
        setPendingPromptRequestId("");
        showError(resp.error.message);
        return;
      }
      setPendingPromptDraft(null);
      if (resp.result.assistant) setAssistant(resp.result.assistant);
      setLiveTranscriptPreview(null);
      showNotice({
        message: operation === "replace_with_refined_prompt"
          ? t("voiceSecretaryPromptOptimizeQueued", {
              defaultValue: "Voice Secretary is optimizing the current prompt.",
            })
          : t("voiceSecretaryPromptRefineQueued", {
              defaultValue: "Voice Secretary is refining the prompt.",
            }),
      });
      void refreshAssistant({ quiet: true });
    } catch {
      pendingPromptRequestIdRef.current = "";
      pendingPromptComposerHashRef.current = "";
      setPendingPromptRequestId("");
      showError(t("voiceSecretaryPromptRefineFailed", { defaultValue: "Failed to send prompt refinement to Voice Secretary." }));
    }
  }, [
    assistantEnabled,
    composerContext,
    composerText,
    effectiveRecognitionLanguage,
    pendingPromptRequestId,
    recognitionBackend,
    refreshAssistant,
    selectedGroupId,
    showError,
    showNotice,
    t,
  ]);

  const takeBrowserFinalTranscriptBuffer = useCallback((): string => {
    const text = String(browserFinalTranscriptBufferRef.current || "").trim();
    browserFinalTranscriptBufferRef.current = "";
    return text;
  }, []);

  const flushBrowserTranscriptWindow = useCallback(async (
    triggerKind = "meeting_window",
    opts?: { documentPath?: string },
  ): Promise<void> => {
    clearTranscriptFlushTimer();
    clearTranscriptMaxFlushTimer();
    const documentPath = String(opts?.documentPath || captureTargetDocumentPathRef.current || "").trim();
    const text = takeBrowserFinalTranscriptBuffer();
    if (captureMode === "prompt") {
      await requestPromptRefine(text, triggerKind || "prompt_refine");
      return;
    }
    if (captureMode === "instruction") {
      await sendInstructionTranscript(text, { triggerKind });
      return;
    }
    await appendTranscriptSegment(text, {
      flush: true,
      triggerKind,
      source: "browser_asr",
      inputDeviceLabel: BROWSER_DEFAULT_MIC_LABEL,
      documentPath,
    });
    pushVoiceStreamMessage(text, { documentPath });
  }, [
    appendTranscriptSegment,
    captureMode,
    clearTranscriptFlushTimer,
    clearTranscriptMaxFlushTimer,
    requestPromptRefine,
    sendInstructionTranscript,
    pushVoiceStreamMessage,
    takeBrowserFinalTranscriptBuffer,
  ]);

  const scheduleTranscriptFlush = useCallback((triggerKind: string, options?: { preserveExisting?: boolean }) => {
    if (options?.preserveExisting && transcriptFlushTimerRef.current !== null) return;
    clearTranscriptFlushTimer();
    const documentPath = captureTargetDocumentPathRef.current;
    // Browser speech boundary events can lag; use recognition-result idle as
    // the primary quiet window. Delayed speechend must not postpone it.
    transcriptFlushTimerRef.current = window.setTimeout(() => {
      transcriptFlushTimerRef.current = null;
      void flushBrowserTranscriptWindow(triggerKind, { documentPath });
    }, effectiveAutoDocumentQuietMs);
  }, [clearTranscriptFlushTimer, effectiveAutoDocumentQuietMs, flushBrowserTranscriptWindow]);

  const scheduleTranscriptMaxFlush = useCallback((triggerKind: string) => {
    if (transcriptMaxFlushTimerRef.current !== null) return;
    const documentPath = captureTargetDocumentPathRef.current;
    transcriptMaxFlushTimerRef.current = window.setTimeout(() => {
      transcriptMaxFlushTimerRef.current = null;
      void flushBrowserTranscriptWindow(triggerKind, { documentPath });
    }, autoDocumentMaxWindowMs);
  }, [autoDocumentMaxWindowMs, flushBrowserTranscriptWindow]);

  const queueBrowserFinalTranscript = useCallback((text: string) => {
    const clean = normalizeBrowserTranscriptChunk(text);
    if (!clean) return;
    if (!browserFinalTranscriptBufferRef.current && isLowValueBrowserSpeechFragment(clean)) return;
    browserSpeechTransientErrorCountRef.current = 0;
    browserSpeechReceivedFinalRef.current = true;
    browserSpeechHadErrorRef.current = false;
    setSpeechError("");
    const merged = mergeTranscriptChunks(browserFinalTranscriptBufferRef.current, clean);
    browserFinalTranscriptBufferRef.current = merged;
    scheduleTranscriptMaxFlush("max_window");
  }, [scheduleTranscriptMaxFlush]);

  const cleanupServiceAudio = useCallback(() => {
    const ws = serviceAudioWsRef.current;
    serviceAudioWsRef.current = null;
    if (ws && ws.readyState === WebSocket.OPEN) {
      try {
        ws.close(1000, "cleanup");
      } catch {
        // ignore websocket cleanup failure
      }
    }
    const processor = serviceAudioProcessorRef.current;
    serviceAudioProcessorRef.current = null;
    if (processor) {
      processor.onaudioprocess = null;
      try {
        processor.disconnect();
      } catch {
        // ignore audio processor cleanup failure
      }
    }
    const source = serviceAudioSourceRef.current;
    serviceAudioSourceRef.current = null;
    if (source) {
      try {
        source.disconnect();
      } catch {
        // ignore audio source cleanup failure
      }
    }
    const audioContext = serviceAudioContextRef.current;
    serviceAudioContextRef.current = null;
    if (audioContext && audioContext.state !== "closed") {
      void audioContext.close().catch(() => undefined);
    }
    const recorder = mediaRecorderRef.current;
    mediaRecorderRef.current = null;
    if (recorder) {
      recorder.ondataavailable = null;
      recorder.onerror = null;
      recorder.onstop = null;
    }
    clearBrowserSpeechMediaHandlers();
    stopMediaStream(mediaStreamRef.current);
    mediaStreamRef.current = null;
    mediaChunksRef.current = [];
    serviceFinalTranscriptRef.current = "";
    serviceLatestPartialTranscriptRef.current = "";
    serviceCommittedTranscriptRef.current = "";
    serviceAudioResamplerRef.current = null;
    serviceAudioDurationMsRef.current = 0;
    serviceCommittedEndMsRef.current = 0;
    serviceFinalSpeakerSegmentsRef.current = [];
    clearServicePartialCommitTimer();
    releaseVoiceCaptureLock(voiceCaptureOwnerIdRef.current);
    setRecording(false);
  }, [clearBrowserSpeechMediaHandlers, clearServicePartialCommitTimer]);

  const stopBrowserSpeech = useCallback(() => {
    browserSpeechStopRequestedRef.current = true;
    browserSpeechTransientErrorCountRef.current = 0;
    clearBrowserSpeechRestartTimer();
    clearBrowserSpeechStopFinalizeTimer();
    const finalizeStoppedSpeech = (recognition: BrowserSpeechRecognition | null) => {
      if (recognition && recognitionRef.current === recognition) recognitionRef.current = null;
      abortBrowserSpeechRecognition(recognition);
      clearBrowserSpeechMediaHandlers();
      stopMediaStream(mediaStreamRef.current);
      mediaStreamRef.current = null;
      releaseVoiceCaptureLock(voiceCaptureOwnerIdRef.current);
      setRecording(false);
      void flushBrowserTranscriptWindow("push_to_talk_stop");
    };
    const recognition = recognitionRef.current;
    setRecording(false);
    if (!recognition) {
      finalizeStoppedSpeech(null);
      return;
    }
    browserSpeechStopFinalizeTimerRef.current = window.setTimeout(() => {
      browserSpeechStopFinalizeTimerRef.current = null;
      finalizeStoppedSpeech(recognition);
    }, 2000);
    try {
      // stop() lets the browser emit any final result before onend performs the stop flush.
      recognition.stop();
    } catch {
      clearBrowserSpeechStopFinalizeTimer();
      finalizeStoppedSpeech(recognition);
    }
  }, [clearBrowserSpeechMediaHandlers, clearBrowserSpeechRestartTimer, clearBrowserSpeechStopFinalizeTimer, flushBrowserTranscriptWindow]);

  const stopServiceAudio = useCallback(() => {
    const ws = serviceAudioWsRef.current;
    const processor = serviceAudioProcessorRef.current;
    if (processor) processor.onaudioprocess = null;
    setRecording(false);
    if (ws && ws.readyState === WebSocket.OPEN) {
      try {
        serviceAudioSeqRef.current += 1;
        ws.send(JSON.stringify({ type: "stop", seq: serviceAudioSeqRef.current }));
        return;
      } catch {
        // fall through to local cleanup
      }
    }
    const recorder = mediaRecorderRef.current;
    if (!recorder) {
      cleanupServiceAudio();
      return;
    }
    try {
      if (recorder.state !== "inactive") {
        recorder.stop();
        return;
      }
    } catch {
      // fall through to cleanup
    }
    cleanupServiceAudio();
  }, [cleanupServiceAudio]);

  const stopCurrentRecording = useCallback(() => {
    if (mediaRecorderRef.current || serviceAudioWsRef.current) {
      stopServiceAudio();
      return;
    }
    stopBrowserSpeech();
  }, [stopBrowserSpeech, stopServiceAudio]);

  const closePanel = useCallback(() => {
    if (recordingRef.current) stopCurrentRecording();
    setOpen(false);
  }, [stopCurrentRecording]);
  const { modalRef } = useModalA11y(open, closePanel);

  useEffect(() => {
    if (!showAssistantModeMenu) return undefined;
    const handlePointerDown = (event: MouseEvent) => {
      const root = rootRef.current;
      if (root && event.target instanceof Node && root.contains(event.target)) return;
      setShowAssistantModeMenu(false);
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setShowAssistantModeMenu(false);
    };
    document.addEventListener("mousedown", handlePointerDown);
    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("mousedown", handlePointerDown);
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [showAssistantModeMenu]);

  useEffect(() => () => {
    const recognition = recognitionRef.current;
    recognitionRef.current = null;
    abortBrowserSpeechRecognition(recognition);
    browserSpeechStopRequestedRef.current = true;
    browserSpeechTransientErrorCountRef.current = 0;
    clearBrowserSpeechRestartTimer();
    clearBrowserSpeechStopFinalizeTimer();
    clearTranscriptFlushTimer();
    clearTranscriptMaxFlushTimer();
    clearServicePartialCommitTimer();
    cleanupServiceAudio();
    releaseVoiceCaptureLock(voiceCaptureOwnerIdRef.current);
  }, [cleanupServiceAudio, clearBrowserSpeechRestartTimer, clearBrowserSpeechStopFinalizeTimer, clearServicePartialCommitTimer, clearTranscriptFlushTimer, clearTranscriptMaxFlushTimer]);

  const startBrowserSpeech = useCallback(async () => {
    const gid = String(selectedGroupId || "").trim();
    if (!assistantEnabled) {
      showError(t("voiceSecretaryEnableFirst", { defaultValue: "Enable Voice Secretary first." }));
      return;
    }
    if (!browserSpeechReady) {
      showError(t("voiceSecretaryBrowserBackendRequired", { defaultValue: "Switch recognition to Browser ASR in Assistants settings first." }));
      return;
    }
    const microphoneIssue = getBrowserMicrophoneSupportIssue();
    if (microphoneIssue) {
      const message = getAudioSupportIssueMessage(microphoneIssue);
      setSpeechError(message);
      showError(message);
      return;
    }
    const supportIssue = getBrowserSpeechSupportIssue();
    setSpeechSupported(!supportIssue);
    if (supportIssue) {
      const message = getBrowserSpeechIssueMessage(supportIssue);
      setSpeechError(message);
      showError(message);
      return;
    }
    const SpeechRecognition = getBrowserSpeechRecognitionConstructor();
    if (!SpeechRecognition) {
      const message = t("voiceSecretaryBrowserUnsupported", {
        defaultValue: "Browser speech recognition is not available in this browser page. Try another current browser.",
      });
      setSpeechError(message);
      showError(message);
      return;
    }
    const activeLock = await claimVoiceCaptureLock(voiceCaptureOwnerIdRef.current, gid);
    if (activeLock) {
      showError(t("voiceSecretaryAnotherRecording", {
        groupId: activeLock.groupId,
        defaultValue: "Voice Secretary is already recording in group {{groupId}} in another active tab. Stop that recording before starting another one.",
      }));
      return;
    }

    const existingRecognition = recognitionRef.current;
    recognitionRef.current = null;
    clearBrowserSpeechRestartTimer();
    clearBrowserSpeechStopFinalizeTimer();
    clearBrowserSpeechMediaHandlers();
    abortBrowserSpeechRecognition(existingRecognition);
    stopMediaStream(mediaStreamRef.current);
    mediaStreamRef.current = null;
    browserSpeechReceivedFinalRef.current = false;
    browserSpeechHadErrorRef.current = false;
    browserSpeechStopRequestedRef.current = false;
    browserSpeechTransientErrorCountRef.current = 0;

    let stream: MediaStream;
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    } catch (error) {
      releaseVoiceCaptureLock(voiceCaptureOwnerIdRef.current);
      const { message, resetSelectedDevice } = getAudioCaptureErrorMessage(error);
      if (resetSelectedDevice) setSelectedAudioDeviceId("");
      setSpeechError(message);
      showError(message);
      return;
    }
    if (!mediaStreamHasLiveAudio(stream)) {
      stopMediaStream(stream);
      releaseVoiceCaptureLock(voiceCaptureOwnerIdRef.current);
      const message = t("voiceSecretaryMicNotFound", {
        defaultValue: "No microphone was found or the selected microphone is unavailable.",
      });
      setSpeechError(message);
      showError(message);
      return;
    }

    // Browser SpeechRecognition owns its own capture lifecycle. Keep getUserMedia
    // as a permission/device probe only; using its track "ended" as truth can
    // stop otherwise valid dictation sessions when the auxiliary stream closes.
    stopMediaStream(stream);
    mediaStreamRef.current = null;
    setRecording(true);
    setSpeechError("");
    refreshVoiceCaptureLock(voiceCaptureOwnerIdRef.current, gid);
    void loadAudioDevices();

    const SpeechRecognitionCtor = SpeechRecognition;
    const stopAfterFatalSpeechFailure = (recognition: BrowserSpeechRecognition | null, message: string, showToast = true) => {
      browserSpeechHadErrorRef.current = true;
      browserSpeechStopRequestedRef.current = true;
      clearBrowserSpeechRestartTimer();
      clearBrowserSpeechStopFinalizeTimer();
      clearBrowserSpeechMediaHandlers();
      if (recognition && recognitionRef.current === recognition) recognitionRef.current = null;
      abortBrowserSpeechRecognition(recognition);
      stopMediaStream(mediaStreamRef.current);
      mediaStreamRef.current = null;
      void flushBrowserTranscriptWindow("meeting_window");
      releaseVoiceCaptureLock(voiceCaptureOwnerIdRef.current);
      setRecording(false);
      setSpeechError(message);
      if (showToast) showError(message);
    };

    function startRecognitionCycle(delayMs = 0): void {
      const runCycle = () => {
        browserSpeechRestartTimerRef.current = null;
        if (browserSpeechStopRequestedRef.current || !assistantEnabled || !browserSpeechReady) {
          recognitionRef.current = null;
          clearBrowserSpeechMediaHandlers();
          stopMediaStream(mediaStreamRef.current);
          mediaStreamRef.current = null;
          releaseVoiceCaptureLock(voiceCaptureOwnerIdRef.current);
          setRecording(false);
          if (!browserSpeechStopRequestedRef.current) void flushBrowserTranscriptWindow("meeting_window");
          return;
        }

        const recognition = new SpeechRecognitionCtor();
        recognition.continuous = true;
        recognition.interimResults = true;
        recognition.lang = browserRecognitionLanguage;
        recognition.maxAlternatives = 1;
        recognition.onspeechstart = () => {
          clearTranscriptFlushTimer();
        };
        recognition.onspeechend = () => {
          scheduleTranscriptFlush("speech_end", { preserveExisting: true });
        };
        recognition.onresult = (event) => {
          let finalText = "";
          let interimText = "";
          for (let resultIndex = event.resultIndex; resultIndex < event.results.length; resultIndex += 1) {
            const result = event.results[resultIndex];
            let text = "";
            for (let altIndex = 0; altIndex < result.length; altIndex += 1) {
              text += result[altIndex]?.transcript || "";
            }
            if (result.isFinal) finalText += text;
            else interimText += text;
          }
          const hasFinalText = Boolean(finalText.trim());
          const cleanInterimText = interimText.replace(/\s+/g, " ").trim();
          if (hasFinalText || cleanInterimText) {
            browserSpeechHadErrorRef.current = false;
            browserSpeechTransientErrorCountRef.current = 0;
            clearTranscriptFlushTimer();
          }
          if (cleanInterimText) {
            updateLiveTranscriptPreview(cleanInterimText, "interim");
          }
          if (hasFinalText) {
            queueBrowserFinalTranscript(finalText);
            updateLiveTranscriptPreview(finalText, "final");
          }
          if (hasFinalText || cleanInterimText) {
            scheduleTranscriptFlush("result_idle");
          }
        };
        recognition.onerror = (event) => {
          const code = String(event.error || "").trim();
          const fatal = BROWSER_SPEECH_FATAL_ERRORS.has(code);
          const recoverable = !fatal && (BROWSER_SPEECH_RECOVERABLE_ERRORS.has(code) || !code);
          if (recoverable) {
            browserSpeechHadErrorRef.current = true;
            // A real missing microphone is already caught by the start-time
            // getUserMedia probe. During a running Web Speech session,
            // Chromium/Edge can report audio-capture for service lifecycle
            // hiccups, so treat it like no-speech instead of burning the fatal
            // retry budget.
            const countsAsTransientFailure = code !== "no-speech" && code !== "aborted" && code !== "audio-capture";
            if (countsAsTransientFailure) browserSpeechTransientErrorCountRef.current += 1;
            if (
              countsAsTransientFailure
              && browserSpeechTransientErrorCountRef.current >= BROWSER_SPEECH_MAX_TRANSIENT_ERRORS
            ) {
              const message = code === "audio-capture"
                ? t("voiceSecretaryMicNotFound", {
                    defaultValue: "No microphone was found or the selected microphone is unavailable.",
                  })
                : code
                  ? t("voiceSecretarySpeechError", { code, defaultValue: "Speech recognition error: {{code}}" })
                  : t("voiceSecretarySpeechErrorGeneric", { defaultValue: "Speech recognition stopped unexpectedly." });
              stopAfterFatalSpeechFailure(recognition, message, code !== "no-speech" && code !== "aborted");
              return;
            }
            if (code && code !== "no-speech" && code !== "aborted") {
              setSpeechError(t("voiceSecretarySpeechRecovering", {
                code,
                defaultValue: "Browser speech recognition is reconnecting after a temporary {{code}} event. Recording is still on.",
              }));
            }
            return;
          }

          const message = code === "not-allowed" || code === "service-not-allowed"
            ? t("voiceSecretaryMicPermissionBlocked", {
                defaultValue: "Microphone permission is blocked. Allow microphone access for this site in the browser, then try again.",
              })
            : code === "audio-capture"
              ? t("voiceSecretaryMicNotFound", {
                  defaultValue: "No microphone was found or the selected microphone is unavailable.",
                })
              : code
                ? t("voiceSecretarySpeechError", { code, defaultValue: "Speech recognition error: {{code}}" })
                : t("voiceSecretarySpeechErrorGeneric", { defaultValue: "Speech recognition stopped unexpectedly." });
          stopAfterFatalSpeechFailure(recognition, message, code !== "no-speech" && code !== "aborted");
        };
        recognition.onend = () => {
          clearBrowserSpeechStopFinalizeTimer();
          const stoppedByUser = browserSpeechStopRequestedRef.current;
          const shouldRestart = !stoppedByUser && assistantEnabled && browserSpeechReady;
          const restartDelay = browserSpeechHadErrorRef.current
            ? browserSpeechRestartDelayMs(browserSpeechTransientErrorCountRef.current)
            : 250;
          if (stoppedByUser) {
            void flushBrowserTranscriptWindow("push_to_talk_stop");
          } else if (!shouldRestart) {
            void flushBrowserTranscriptWindow("meeting_window");
          }
          if (recognitionRef.current === recognition) recognitionRef.current = null;
          if (shouldRestart) {
            setRecording(true);
            startRecognitionCycle(restartDelay);
            return;
          }
          clearBrowserSpeechMediaHandlers();
          stopMediaStream(mediaStreamRef.current);
          mediaStreamRef.current = null;
          releaseVoiceCaptureLock(voiceCaptureOwnerIdRef.current);
          setRecording(false);
          if (!browserSpeechReceivedFinalRef.current && !browserSpeechHadErrorRef.current) {
            setSpeechError(t("voiceSecretaryBrowserAsrEndedWithoutTranscript", {
              defaultValue: "Browser ASR stopped without returning transcript. Check the microphone connection, site permission, and system input device, then try again.",
            }));
          }
        };

        try {
          recognitionRef.current = recognition;
          setRecording(true);
          refreshVoiceCaptureLock(voiceCaptureOwnerIdRef.current, gid);
          browserSpeechHadErrorRef.current = false;
          recognition.start();
          setSpeechError("");
        } catch {
          if (recognitionRef.current === recognition) recognitionRef.current = null;
          browserSpeechHadErrorRef.current = true;
          browserSpeechTransientErrorCountRef.current += 1;
          setSpeechError(t("voiceSecretarySpeechRecovering", {
            code: "start-failed",
            defaultValue: "Browser speech recognition is reconnecting after a temporary {{code}} event. Recording is still on.",
          }));
          if (
            browserSpeechTransientErrorCountRef.current < BROWSER_SPEECH_MAX_TRANSIENT_ERRORS
            && !browserSpeechStopRequestedRef.current
            && assistantEnabled
            && browserSpeechReady
          ) {
            startRecognitionCycle(browserSpeechRestartDelayMs(browserSpeechTransientErrorCountRef.current));
            return;
          }
          stopAfterFatalSpeechFailure(recognition, t("voiceSecretarySpeechStartFailed", {
            defaultValue: "Could not start browser speech recognition.",
          }));
        }
      };

      clearBrowserSpeechRestartTimer();
      if (delayMs > 0) {
        browserSpeechRestartTimerRef.current = window.setTimeout(runCycle, delayMs);
        return;
      }
      setSpeechError("");
      runCycle();
    }

    startRecognitionCycle();
  }, [
    assistantEnabled,
    browserSpeechReady,
    clearBrowserSpeechMediaHandlers,
    clearBrowserSpeechRestartTimer,
    clearBrowserSpeechStopFinalizeTimer,
    clearTranscriptFlushTimer,
    browserRecognitionLanguage,
    flushBrowserTranscriptWindow,
    getAudioCaptureErrorMessage,
    getAudioSupportIssueMessage,
    getBrowserSpeechIssueMessage,
    loadAudioDevices,
    queueBrowserFinalTranscript,
    scheduleTranscriptFlush,
    selectedGroupId,
    showError,
    t,
    updateLiveTranscriptPreview,
  ]);

  const handleServiceStreamingFinal = useCallback(async (text: string) => {
    const clean = normalizeBrowserTranscriptChunk(text);
    if (!clean) return;
    clearServicePartialCommitTimer();
    const committed = normalizeBrowserTranscriptChunk(serviceCommittedTranscriptRef.current);
    if (committed && (committed === clean || committed.endsWith(clean))) {
      serviceFinalTranscriptRef.current = committed;
      const liveId = voiceStreamItemIdRef.current;
      setLiveTranscriptPreview(null);
      voiceStreamItemIdRef.current = "";
      if (liveId) {
        setVoiceStreamItems((prev) => prev.filter((item) => item.id !== liveId));
      }
      return;
    }
    const newText = committed && clean.startsWith(committed)
      ? clean.slice(committed.length).trim()
      : clean;
    serviceFinalTranscriptRef.current = mergeTranscriptChunks(serviceFinalTranscriptRef.current, clean);
    if (captureMode === "prompt") {
      updateLiveTranscriptPreview(newText || clean, "final");
      await requestPromptRefine(newText, "service_prompt_refine");
    } else if (captureMode === "instruction") {
      updateLiveTranscriptPreview(newText || clean, "final");
      await sendInstructionTranscript(newText, { triggerKind: "service_voice_instruction" });
    } else {
      const startMs = serviceCommittedEndMsRef.current;
      const endMs = Math.max(startMs, serviceAudioDurationMsRef.current);
      const documentPath = String(captureTargetDocumentPathRef.current || "").trim();
      await appendTranscriptSegment(newText, {
        flush: false,
        documentPath,
        source: "assistant_service_local_asr_streaming",
        triggerKind: "service_streaming_transcript",
        inputDeviceLabel: selectedAudioDeviceLabel,
        startMs,
        endMs,
      });
      pushVoiceStreamMessage(newText, { startMs, endMs, documentPath });
      serviceCommittedEndMsRef.current = endMs;
    }
    serviceCommittedTranscriptRef.current = mergeTranscriptChunks(committed, newText);
  }, [
    appendTranscriptSegment,
    captureMode,
    clearServicePartialCommitTimer,
    requestPromptRefine,
    selectedAudioDeviceLabel,
    sendInstructionTranscript,
    updateLiveTranscriptPreview,
    pushVoiceStreamMessage,
  ]);

  const startServiceAudio = useCallback(async () => {
    const gid = String(selectedGroupId || "").trim();
    let latestReadiness = resolveVoiceServiceReadiness({
      assistant,
      serviceRuntimesById,
      streamingRuntimeId: STREAMING_ASR_RUNTIME_ID,
    });
    if (gid) {
      const resp = await fetchAssistant(gid, "voice_secretary");
      if (resp.ok) {
        setAssistant(resp.result.assistant || null);
        setServiceRuntimesById(resp.result.service_runtimes_by_id || {});
        latestReadiness = resolveVoiceServiceReadiness({
          assistant: resp.result.assistant || null,
          serviceRuntimesById: resp.result.service_runtimes_by_id || {},
          streamingRuntimeId: STREAMING_ASR_RUNTIME_ID,
        });
      }
    }
    if (!latestReadiness.assistantEnabled) {
      showError(t("voiceSecretaryEnableFirst", { defaultValue: "Enable Voice Secretary first." }));
      return;
    }
    if (!latestReadiness.serviceAsrReady) {
      showError(t("voiceSecretaryServiceBackendRequired", {
        defaultValue: "Switch recognition to Assistant service local ASR in Assistants settings first.",
      }));
      return;
    }
    if (!latestReadiness.serviceAsrConfigured) {
      showError(latestReadiness.streamingRuntimeReady
        ? t("voiceSecretaryStreamingRuntimeNotConnected", {
          defaultValue: "Streaming ASR runtime is installed, but the live streaming transcription backend is not connected yet. Use Browser ASR until the streaming backend is enabled.",
        })
        : t("voiceSecretaryServiceAsrNeedsRuntime", {
          defaultValue: "Browser microphone capture is available, but assistant service local ASR runtime is not ready yet. Install the streaming ASR runtime in Settings > Assistants, or switch back to Browser ASR.",
        }));
      return;
    }
    const supportIssue = getBrowserAudioSupportIssue();
    if (supportIssue) {
      const message = getAudioSupportIssueMessage(supportIssue);
      setServiceAudioSupported(false);
      setSpeechError(message);
      showError(message);
      return;
    }
    const activeLock = await claimVoiceCaptureLock(voiceCaptureOwnerIdRef.current, gid);
    if (activeLock) {
      showError(t("voiceSecretaryAnotherRecording", {
        groupId: activeLock.groupId,
        defaultValue: "Voice Secretary is already recording in group {{groupId}} in another active tab. Stop that recording before starting another one.",
      }));
      return;
    }
    try {
      const audioConstraints: MediaTrackConstraints = {
        channelCount: { ideal: 1 },
        echoCancellation: { ideal: true },
        noiseSuppression: { ideal: true },
        autoGainControl: { ideal: true },
        sampleRate: { ideal: 48000 },
      };
      if (selectedAudioDeviceId) audioConstraints.deviceId = { exact: selectedAudioDeviceId };
      const constraints: MediaStreamConstraints = { audio: audioConstraints };
      const stream = await navigator.mediaDevices.getUserMedia(constraints);
      const AudioContextConstructor = window.AudioContext || (window as typeof window & { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!AudioContextConstructor) {
        throw new Error("AudioContext unavailable");
      }
      const audioContext = new AudioContextConstructor();
      const source = audioContext.createMediaStreamSource(stream);
      const processor = audioContext.createScriptProcessor(4096, 1, 1);
      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const wsUrl = withAuthToken(`${protocol}//${window.location.host}/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/transcriptions/ws`);
      const ws = new WebSocket(wsUrl);
      mediaStreamRef.current = stream;
      serviceAudioContextRef.current = audioContext;
      serviceAudioSourceRef.current = source;
      serviceAudioProcessorRef.current = processor;
      serviceAudioWsRef.current = ws;
      serviceAudioSeqRef.current = 0;
      serviceFinalTranscriptRef.current = "";
      serviceLatestPartialTranscriptRef.current = "";
      serviceCommittedTranscriptRef.current = "";
      serviceAudioResamplerRef.current = new Pcm16Resampler(audioContext.sampleRate);
      serviceAudioDurationMsRef.current = 0;
      serviceCommittedEndMsRef.current = 0;
      serviceFinalSpeakerSegmentsRef.current = [];
      setDiarizationActivityItems([]);
      clearServicePartialCommitTimer();
      processor.onaudioprocess = (event) => {
        const activeWs = serviceAudioWsRef.current;
        if (!activeWs || activeWs.readyState !== WebSocket.OPEN || !recordingRef.current) return;
        const input = event.inputBuffer.getChannelData(0);
        let resampler = serviceAudioResamplerRef.current;
        if (!resampler) {
          resampler = new Pcm16Resampler(audioContext.sampleRate);
          serviceAudioResamplerRef.current = resampler;
        }
        const pcm = resampler.push(input);
        if (!pcm.byteLength) return;
        serviceAudioDurationMsRef.current += Math.round((pcm.byteLength / 2 / 16000) * 1000);
        serviceAudioSeqRef.current += 1;
        activeWs.send(JSON.stringify({
          type: "audio",
          seq: serviceAudioSeqRef.current,
          sample_rate: 16000,
          format: "pcm16",
          audio_base64: bytesToBase64(pcm),
        }));
      };
      ws.onopen = () => {
        serviceAudioSeqRef.current += 1;
        ws.send(JSON.stringify({
          type: "start",
          seq: serviceAudioSeqRef.current,
          session_id: voiceCaptureOwnerIdRef.current,
          sample_rate: 16000,
          language: effectiveRecognitionLanguage,
          by: "user",
        }));
      };
      ws.onmessage = (event) => {
        void (async () => {
          const payload = JSON.parse(String(event.data || "{}")) as Record<string, unknown>;
          const type = String(payload.type || "").trim();
          if (type === "ready") {
            setSpeechError("");
            return;
          }
          if (type === "partial") {
            const text = String(payload.text || "").trim();
            if (text) {
              serviceLatestPartialTranscriptRef.current = text;
              updateLiveTranscriptPreview(text, "interim", {
                startMs: serviceCommittedEndMsRef.current,
                endMs: serviceAudioDurationMsRef.current,
              });
            }
            return;
          }
          if (type === "final") {
            await handleServiceStreamingFinal(String(payload.text || ""));
            return;
          }
          if (type === "diarization_started" || type === "diarization_status") {
            const id = `diarization-${voiceCaptureOwnerIdRef.current}`;
            const status = String(payload.status || "").trim();
            const preview = status === "transcribing_speakers"
              ? t("voiceSecretaryDiarizationTranscribing", { defaultValue: "Transcribing speaker turns..." })
              : t("voiceSecretaryDiarizationWorking", { defaultValue: "Separating speakers..." });
            setDiarizationActivityItems((prev) => [
              {
                id,
                status: "working" as const,
                speakerCount: 0,
                preview,
                artifactPath: String(payload.artifact_path || "").trim() || undefined,
                createdAt: Date.now(),
              },
              ...prev.filter((item) => item.id !== id),
            ].slice(0, VOICE_ACTIVITY_FEED_LIMIT));
            return;
          }
          if (type === "diarization_delta" || type === "diarization") {
            if (type === "diarization_delta") return;
            const id = `diarization-${voiceCaptureOwnerIdRef.current}`;
            if (payload.ok === false) {
              const error = recordFromUnknown(payload.error);
              setDiarizationActivityItems((prev) => [
                {
                  id,
                  status: "failed" as const,
                speakerCount: 0,
                preview: "",
                error: String(error.message || "") || t("voiceSecretaryDiarizationFailed", { defaultValue: "Speaker separation failed." }),
                artifactPath: String(payload.artifact_path || "").trim() || undefined,
                createdAt: Date.now(),
              },
              ...prev.filter((item) => item.id !== id),
              ].slice(0, VOICE_ACTIVITY_FEED_LIMIT));
              return;
            }
            const result = recordFromUnknown(payload.result);
            const rawSegments = Array.isArray(result.segments) ? result.segments : [];
            const rawSpeakerTranscriptSegments = Array.isArray(result.speaker_transcript_segments)
              ? result.speaker_transcript_segments
              : [];
            const provisional = Boolean(result.provisional);
            if (!provisional) {
              serviceFinalSpeakerSegmentsRef.current = rawSegments
                .map((item) => recordFromUnknown(item))
                .filter((item) => Object.keys(item).length > 0);
            }
            const speakerLabels = rawSegments
              .map((item) => String(recordFromUnknown(item).speaker_label || "").trim())
              .filter((label, index, labels) => label && labels.indexOf(label) === index);
            const preview = rawSegments.slice(0, 4).map((item) => {
              const record = recordFromUnknown(item);
              const label = String(record.speaker_label || "").trim() || "Speaker";
              const startMs = Number(record.start_ms || 0);
              const endMs = Number(record.end_ms || 0);
              return `${label} ${formatVoiceOffsetMs(startMs)}-${formatVoiceOffsetMs(endMs)}`;
            }).join("\n");
            setDiarizationActivityItems((prev) => [
              {
                id,
                status: provisional ? "provisional" as const : "ready" as const,
                speakerCount: speakerLabels.length,
                preview: preview || (provisional
                  ? t("voiceSecretaryDiarizationEstimating", { defaultValue: "Estimating speakers..." })
                  : t("voiceSecretaryDiarizationNoSpeech", { defaultValue: "No speaker turn detected." })),
                segments: rawSegments.map((item) => {
                  const record = recordFromUnknown(item);
                  return {
                    speaker_label: String(record.speaker_label || "").trim(),
                    start_ms: Number(record.start_ms || 0),
                    end_ms: Number(record.end_ms || 0),
                  };
                }),
                speakerTranscriptSegments: rawSpeakerTranscriptSegments.map((item) => {
                  const record = recordFromUnknown(item);
                  return {
                    speaker_label: String(record.speaker_label || "").trim(),
                    start_ms: Number(record.start_ms || 0),
                    end_ms: Number(record.end_ms || 0),
                    text: String(record.text || "").trim(),
                  };
                }),
                artifactPath: String(result.artifact_path || "").trim() || undefined,
                createdAt: Date.now(),
              },
              ...prev.filter((item) => item.id !== id),
            ].slice(0, VOICE_ACTIVITY_FEED_LIMIT));
            return;
          }
          if (type === "closed") {
            cleanupServiceAudio();
            if (captureMode === "document") {
              void appendTranscriptSegment("", {
                flush: true,
                source: "assistant_service_local_asr_streaming",
                triggerKind: "push_to_talk_stop",
                inputDeviceLabel: selectedAudioDeviceLabel,
                speakerSegments: serviceFinalSpeakerSegmentsRef.current,
              });
            }
            window.setTimeout(() => {
              if (open) void refreshAssistant({ quiet: true });
            }, 1800);
            return;
          }
          if (type === "error" || payload.ok === false) {
            const error = recordFromUnknown(payload.error);
            const message = String(error.message || "") || t("voiceSecretaryAudioTranscribeFailed", {
              defaultValue: "Voice Secretary could not transcribe the recorded audio.",
            });
            setSpeechError(message);
            showError(message);
            cleanupServiceAudio();
          }
        })().catch(() => {
          const message = t("voiceSecretaryAudioTranscribeFailed", {
            defaultValue: "Voice Secretary could not transcribe the recorded audio.",
          });
          setSpeechError(message);
          showError(message);
          cleanupServiceAudio();
        });
      };
      ws.onerror = () => {
        const message = t("voiceSecretaryAudioCaptureFailed", { defaultValue: "Audio capture failed." });
        setSpeechError(message);
        showError(message);
        cleanupServiceAudio();
      };
      ws.onclose = () => {
        if (serviceAudioWsRef.current === ws) cleanupServiceAudio();
      };
      source.connect(processor);
      processor.connect(audioContext.destination);
      setSpeechError("");
      setRecording(true);
      void loadAudioDevices();
    } catch (error) {
      cleanupServiceAudio();
      const { message, resetSelectedDevice } = getAudioCaptureErrorMessage(error);
      if (resetSelectedDevice) setSelectedAudioDeviceId("");
      setSpeechError(message);
      showError(message);
    }
  }, [
    assistantEnabled,
    cleanupServiceAudio,
    getAudioCaptureErrorMessage,
    getAudioSupportIssueMessage,
    handleServiceStreamingFinal,
    clearServicePartialCommitTimer,
    loadAudioDevices,
    effectiveRecognitionLanguage,
    appendTranscriptSegment,
    captureMode,
    refreshAssistant,
    open,
    selectedAudioDeviceId,
    selectedGroupId,
    assistant,
    serviceAsrReady,
    serviceAsrConfigured,
    serviceRuntimesById,
    showError,
    streamingRuntimeReady,
    t,
    selectedAudioDeviceLabel,
    updateLiveTranscriptPreview,
  ]);

  useEffect(() => {
    if (!recording) return undefined;
    const gid = String(selectedGroupId || "").trim();
    const interval = window.setInterval(() => {
      refreshVoiceCaptureLock(voiceCaptureOwnerIdRef.current, gid);
    }, 5000);
    return () => window.clearInterval(interval);
  }, [recording, selectedGroupId]);

  const setAssistantEnabledForGroup = useCallback(async (nextEnabled: boolean) => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return false;
    setActionBusy("enable");
    try {
      if (!nextEnabled && recordingRef.current) {
        stopCurrentRecording();
      }
      const resp = await updateAssistantSettings(gid, "voice_secretary", {
        enabled: nextEnabled,
      });
      if (!resp.ok) {
        showError(resp.error.message);
        return false;
      }
      setAssistant(resp.result.assistant || null);
      showNotice({
        message: nextEnabled
          ? t("voiceSecretaryEnabledForGroup", { defaultValue: "Voice Secretary enabled for this group." })
          : t("voiceSecretaryDisabledForGroup", { defaultValue: "Voice Secretary disabled for this group." }),
      });
      return true;
    } catch {
      showError(nextEnabled
        ? t("voiceSecretaryEnableFailed", { defaultValue: "Failed to enable Voice Secretary." })
        : t("voiceSecretaryDisableFailed", { defaultValue: "Failed to disable Voice Secretary." }));
      return false;
    } finally {
      setActionBusy("");
    }
  }, [selectedGroupId, showError, showNotice, stopCurrentRecording, t]);

  const updateRecognitionLanguage = useCallback(async (nextLanguage: string) => {
    const gid = String(selectedGroupId || "").trim();
    const language = String(nextLanguage || "auto").trim() || "auto";
    if (!gid || language === configuredRecognitionLanguage) return;
    const previousAssistant = assistant;
    const nextConfig = { ...(assistant?.config || {}), recognition_language: language };
    setAssistant((current) => current
      ? { ...current, config: { ...(current.config || {}), recognition_language: language } }
      : current);
    setRecognitionLanguageSaving(true);
    try {
      const resp = await updateAssistantSettings(gid, "voice_secretary", {
        config: nextConfig,
        by: "user",
      });
      if (!resp.ok) {
        setAssistant(previousAssistant || null);
        showError(resp.error.message);
        return;
      }
      setAssistant(resp.result.assistant || null);
    } catch {
      setAssistant(previousAssistant || null);
      showError(t("voiceSecretaryLanguageSaveFailed", { defaultValue: "Failed to update Voice Secretary language." }));
    } finally {
      setRecognitionLanguageSaving(false);
    }
  }, [
    assistant,
    configuredRecognitionLanguage,
    selectedGroupId,
    showError,
    t,
  ]);

  const clearAskFeedbackHistory = useCallback(async () => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) {
      setLiveTranscriptPreview(null);
      setVoiceStreamItems([]);
      voiceStreamItemIdRef.current = "";
      setDocumentActivityItems([]);
      setDiarizationActivityItems([]);
      return;
    }
    if (!askFeedbackItems.length && !documentActivityItems.length && !diarizationActivityItems.length && !liveTranscriptPreview && !voiceStreamItems.length) return;
    const previousItems = askFeedbackItems;
    const previousLiveTranscriptPreview = liveTranscriptPreview;
    const previousVoiceStreamItems = voiceStreamItems;
    const previousDocumentActivityItems = documentActivityItems;
    const previousDiarizationActivityItems = diarizationActivityItems;
    setAskFeedbackItems([]);
    setLiveTranscriptPreview(null);
    setVoiceStreamItems([]);
    voiceStreamItemIdRef.current = "";
    setDocumentActivityItems([]);
    setDiarizationActivityItems([]);
    if (voiceReplyBubbleRequestId) {
      setVoiceReplyBubbleRequestId("");
    }
    if (!askFeedbackItems.length) return;
    setActionBusy("clear_ask");
    try {
      const resp = await clearVoiceAssistantAskRequests(gid, { keepActive: false, by: "user" });
      if (!resp.ok) {
        setAskFeedbackItems(previousItems);
        setLiveTranscriptPreview(previousLiveTranscriptPreview);
        setVoiceStreamItems(previousVoiceStreamItems);
        setDocumentActivityItems(previousDocumentActivityItems);
        setDiarizationActivityItems(previousDiarizationActivityItems);
        showError(resp.error.message);
        return;
      }
      const nextItems = resp.result.ask_requests || [];
      setAskFeedbackItems(nextItems);
      const currentAskRequestId = String(pendingAskRequestIdRef.current || "").trim();
      if (currentAskRequestId && !nextItems.some((item) => item.request_id === currentAskRequestId)) {
        pendingAskRequestIdRef.current = "";
        setPendingAskRequestId("");
      }
      const replyRequestId = String(voiceReplyBubbleRequestId || "").trim();
      if (replyRequestId && !nextItems.some((item) => item.request_id === replyRequestId && hasFinalAskReply(item))) {
        setVoiceReplyBubbleRequestId("");
      }
    } catch {
      setAskFeedbackItems(previousItems);
      setLiveTranscriptPreview(previousLiveTranscriptPreview);
      setVoiceStreamItems(previousVoiceStreamItems);
      setDocumentActivityItems(previousDocumentActivityItems);
      setDiarizationActivityItems(previousDiarizationActivityItems);
      showError(t("voiceSecretaryClearRequestsFailed", { defaultValue: "Failed to clear Voice Secretary requests." }));
    } finally {
      setActionBusy("");
    }
  }, [
    askFeedbackItems,
    diarizationActivityItems,
    documentActivityItems,
    liveTranscriptPreview,
    selectedGroupId,
    showError,
    t,
    voiceReplyBubbleRequestId,
    voiceStreamItems,
  ]);

  const persistCurrentDocument = useCallback(async (): Promise<AssistantVoiceDocument | null> => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return null;
    const resp = await saveVoiceAssistantDocument(gid, {
      documentPath: activeDocumentWritePath || viewedDocumentPath || captureTargetDocumentPath,
      content: documentDraft,
      status: activeDocument?.status || "active",
      by: "user",
    });
    if (!resp.ok) {
      showError(resp.error.message);
      return null;
    }
    applyDocumentMutationResult(resp.result.document, resp.result.assistant);
    return resp.result.document || null;
  }, [
    activeDocumentWritePath,
    activeDocument?.status,
    viewedDocumentPath,
    applyDocumentMutationResult,
    captureTargetDocumentPath,
    documentDraft,
    selectedGroupId,
    showError,
  ]);

  const saveDocument = useCallback(async () => {
    setActionBusy("save_doc");
    try {
      const document = await persistCurrentDocument();
      if (!document) return;
      setDocumentEditing(false);
      showNotice({ message: t("voiceSecretaryDocumentSaved", { defaultValue: "Voice Secretary working document saved." }) });
    } catch {
      showError(t("voiceSecretaryDocumentSaveFailed", { defaultValue: "Failed to save Voice Secretary working document." }));
    } finally {
      setActionBusy("");
    }
  }, [
    persistCurrentDocument,
    showError,
    showNotice,
    t,
  ]);

  const startCreateDocument = useCallback(() => {
    if (documentHasUnsavedEdits) {
      const confirmed = window.confirm(t("voiceSecretaryNewDocumentConfirm", {
        defaultValue: "Create a new document and discard unsaved edits in this panel?",
      }));
      if (!confirmed) return;
    }
    setNewDocumentTitleDraft(t("voiceSecretaryDefaultDocumentTitle", { defaultValue: "Untitled document" }));
    setCreatingDocument(true);
  }, [documentHasUnsavedEdits, t]);

  const cancelCreateDocument = useCallback(() => {
    if (actionBusy === "new_doc") return;
    setCreatingDocument(false);
    setNewDocumentTitleDraft("");
  }, [actionBusy]);

  const createDocument = useCallback(async () => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    const title = newDocumentTitleDraft.trim() || t("voiceSecretaryDefaultDocumentTitle", { defaultValue: "Untitled document" });
    setActionBusy("new_doc");
    try {
      const resp = await saveVoiceAssistantDocument(gid, {
        title,
        content: "",
        createNew: true,
        by: "user",
      });
      if (!resp.ok) {
        showError(resp.error.message);
        return;
      }
      applyDocumentMutationResult(resp.result.document, resp.result.assistant);
      const docPath = voiceDocumentPath(resp.result.document);
      if (docPath) {
        setViewedDocumentPath(docPath);
        setCaptureTargetDocumentPath(docPath);
        captureTargetDocumentPathRef.current = docPath;
        loadDocumentDraft(resp.result.document || null);
      }
      setCreatingDocument(false);
      setNewDocumentTitleDraft("");
      setDocumentEditing(true);
      showNotice({ message: t("voiceSecretaryDocumentCreated", { defaultValue: "Voice Secretary working document created." }) });
    } catch {
      showError(t("voiceSecretaryDocumentCreateFailed", { defaultValue: "Failed to create Voice Secretary working document." }));
    } finally {
      setActionBusy("");
    }
  }, [
    applyDocumentMutationResult,
    loadDocumentDraft,
    newDocumentTitleDraft,
    selectedGroupId,
    showError,
    showNotice,
    t,
  ]);

  const sendPanelRequest = useCallback(async () => {
    const gid = String(selectedGroupId || "").trim();
    const instruction = documentInstruction.trim();
    if (!gid || !instruction) return;
    if (captureMode === "prompt") return;
    if (captureMode === "instruction") {
      setActionBusy("instruct_ask");
      try {
        const sent = await sendInstructionTranscript(instruction, { triggerKind: "typed_voice_instruction" });
        if (sent) setDocumentInstruction("");
      } finally {
        setActionBusy("");
      }
      return;
    }
    if (documentHasUnsavedEdits) {
      showError(t("voiceSecretaryDocumentUnsavedBeforeRequest", {
        defaultValue: "Save or discard local document edits before sending a request to Voice Secretary.",
      }));
      return;
    }
    const docPath = activeDocumentWritePath || viewedDocumentPath || captureTargetDocumentPath;
    const targetDocument = docPath
      ? findVoiceDocument(documents, docPath)
      : null;
    if (!docPath || !targetDocument || String(targetDocument.status || "active").trim().toLowerCase() === "archived") {
      showError(t("voiceSecretaryDocumentRequestStale", {
        defaultValue: "This document is no longer active. Refresh or choose another document before sending a request.",
      }));
      await refreshAssistant({ quiet: true });
      return;
    }
    setActionBusy("instruct_doc");
    try {
      const resp = await sendVoiceAssistantDocumentInstruction(gid, docPath, {
        instruction,
        documentPath: docPath,
        trigger: {
          trigger_kind: "user_instruction",
          mode: "meeting",
          recognition_backend: recognitionBackend,
          language: effectiveRecognitionLanguage,
        },
        by: "user",
      });
      if (!resp.ok) {
        showError(resp.error.message);
        return;
      }
      applyDocumentMutationResult(resp.result.document, resp.result.assistant);
      const requestId = String(resp.result.request_id || "").trim();
      if (requestId) {
        localVoiceReplyRequestIdsRef.current.add(requestId);
        pendingAskRequestIdRef.current = requestId;
        setPendingAskRequestId(requestId);
        setAskFeedbackItems((prev) => [
          {
            request_id: requestId,
            status: "pending",
            request_text: instruction,
            request_preview: instruction.slice(0, 240),
            document_path: docPath,
          },
          ...prev.filter((item) => item.request_id !== requestId),
        ].slice(0, 10));
      }
      setDocumentInstruction("");
      setDocumentEditing(false);
      showNotice({
        message: t("voiceSecretaryDocumentInstructionQueued", { defaultValue: "Request sent to Voice Secretary." }),
      });
      void refreshAssistant({ quiet: true });
    } catch {
      showError(t("voiceSecretaryDocumentInstructionFailed", { defaultValue: "Failed to send the request to Voice Secretary." }));
    } finally {
      setActionBusy("");
    }
  }, [
    activeDocumentWritePath,
    viewedDocumentPath,
    applyDocumentMutationResult,
    captureTargetDocumentPath,
    captureMode,
    documentHasUnsavedEdits,
    documentInstruction,
    documents,
    effectiveRecognitionLanguage,
    recognitionBackend,
    refreshAssistant,
    selectedGroupId,
    sendInstructionTranscript,
    showError,
    showNotice,
    t,
  ]);

  const selectDocument = useCallback(async (document: AssistantVoiceDocument) => {
    const nextPath = voiceDocumentPath(document);
    const currentPath = activeDocumentWritePath || viewedDocumentPath;
    if (!nextPath || nextPath === currentPath) return;
    if (documentHasUnsavedEdits) {
      const confirmed = window.confirm(t("voiceSecretarySwitchDocumentConfirm", {
        defaultValue: "Switch documents and discard unsaved edits in this panel?",
      }));
      if (!confirmed) return;
    }
    setViewedDocumentPath(nextPath);
    loadDocumentDraft(document);
    setDocumentEditing(false);
    setCreatingDocument(false);
    setNewDocumentTitleDraft("");
  }, [
    activeDocumentWritePath,
    viewedDocumentPath,
    documentHasUnsavedEdits,
    loadDocumentDraft,
    t,
  ]);

  const setCaptureTargetDocument = useVoiceCaptureTargetDocumentSelection({
    selectedGroupId,
    recording,
    captureTargetDocumentPathRef,
    getDocumentPath: voiceDocumentPath,
    setCaptureTargetDocumentPath,
    clearTranscriptFlushTimer,
    clearTranscriptMaxFlushTimer,
    flushBrowserTranscriptWindow,
    refreshAssistant,
    showError,
    showNotice,
    t,
  });

  const archiveDocument = useCallback(async (targetDocument?: AssistantVoiceDocument | null) => {
    const gid = String(selectedGroupId || "").trim();
    const docPath = targetDocument ? voiceDocumentPath(targetDocument) : (activeDocumentWritePath || viewedDocumentPath);
    if (!gid || !docPath) return;
    const title = String(targetDocument?.title || findVoiceDocument(documents, docPath)?.title || docPath).trim();
    const confirmed = window.confirm(t("voiceSecretaryArchiveDocumentConfirm", {
      title,
      defaultValue: "Archive document \"{{title}}\"?",
    }));
    if (!confirmed) return;
    const isActiveTarget = docPath === (activeDocumentWritePath || viewedDocumentPath);
    setActionBusy("archive_doc");
    try {
      const resp = await archiveVoiceAssistantDocument(gid, docPath, { by: "user" });
      if (!resp.ok) {
        showError(resp.error.message);
        return;
      }
      archivedDocumentIdsRef.current.add(docPath);
      setDocuments((prev) => prev.filter((item) => voiceDocumentPath(item) !== docPath));
      if (isActiveTarget) {
        setViewedDocumentPath("");
        loadDocumentDraft(null);
        setDocumentEditing(false);
      }
      if (captureTargetDocumentPathRef.current === docPath) {
        captureTargetDocumentPathRef.current = "";
        setCaptureTargetDocumentPath("");
      }
      showNotice({ message: t("voiceSecretaryDocumentArchived", { defaultValue: "Voice Secretary working document archived." }) });
      await refreshAssistant({ quiet: true });
    } catch {
      showError(t("voiceSecretaryDocumentArchiveFailed", { defaultValue: "Failed to archive the Voice Secretary document." }));
    } finally {
      setActionBusy("");
    }
  }, [viewedDocumentPath, activeDocumentWritePath, documents, loadDocumentDraft, refreshAssistant, selectedGroupId, showError, showNotice, t]);

  const downloadCurrentDocument = useCallback(() => {
    if (!activeDocument) return;
    const fileName = voiceDocumentDownloadFileName(activeDocument, documentDisplayTitle);
    downloadMarkdownDocument(fileName, documentDraft);
    showNotice({
      message: t("voiceSecretaryDocumentDownloaded", {
        fileName,
        defaultValue: "Downloaded {{fileName}}.",
      }),
    });
  }, [activeDocument, documentDisplayTitle, documentDraft, showNotice, t]);

  const workspaceRecordLabel = recording
    ? t("voiceSecretaryStopAndSaveShort", { defaultValue: "Stop & save" })
    : t("voiceSecretaryRecordShort", { defaultValue: "Record" });
  const captureStartTitle = captureMode === "prompt"
    ? t("voiceSecretaryPromptModeStartHint", { defaultValue: "Click to quickly polish speech into a ready-to-send prompt" })
    : captureMode === "instruction"
      ? t("voiceSecretaryInstructionModeStartHint", { defaultValue: "Record a request for Voice Secretary to handle directly" })
      : t("voiceSecretaryStartDictation", { defaultValue: "Start recording" });
  const assistantRowModeOptions: Array<{ key: VoiceSecretaryCaptureMode; label: string; description: string }> = useMemo(() => [
    {
      key: "document",
      label: t("voiceSecretaryModeDocument", { defaultValue: "Doc" }),
      description: t("voiceSecretaryModeDocumentDesc", { defaultValue: "Record into working docs" }),
    },
    {
      key: "instruction",
      label: t("voiceSecretaryModeInstruction", { defaultValue: "Ask" }),
      description: t("voiceSecretaryModeInstructionDesc", { defaultValue: "Handle directly" }),
    },
    {
      key: "prompt",
      label: t("voiceSecretaryModePrompt", { defaultValue: "Prompt" }),
      description: t("voiceSecretaryModePromptDesc", { defaultValue: "Polish composer" }),
    },
  ], [t]);
  const statusLabel = recording
    ? t("voiceSecretaryRecording", { defaultValue: "Recording" })
    : assistantEnabled
      ? t("voiceSecretaryEnabled", { defaultValue: "Enabled" })
      : t("voiceSecretaryNotEnabled", { defaultValue: "Not enabled" });

  const dictationSupported = browserSpeechReady
    ? !browserSpeechSupportIssue && speechSupported
    : serviceAsrReady
      ? serviceAudioSupportIssue
        ? false
        : serviceAudioSupported
      : false;
  const startDictation = serviceAsrReady ? startServiceAudio : startBrowserSpeech;
  const activeDocumentPath = voiceDocumentPath(activeDocument);
  const openButtonLabel = open
    ? t("voiceSecretaryClose", { defaultValue: "Close Voice Secretary" })
    : recording
      ? t("voiceSecretaryOpenRecordingWorkspace", { defaultValue: "Expand Voice Secretary workspace - recording" })
      : t("voiceSecretaryOpenWorkspace", { defaultValue: "Expand Voice Secretary workspace" });
  const openButtonIconSizePx = buttonClassName
    ? buttonSizePx
    : Math.max(20, Math.min(26, Math.round(buttonSizePx - 14)));
  const promptDraftWaiting = Boolean(pendingPromptRequestId && !pendingPromptDraft);
  const promptDraftWaitingTitle = t("voiceSecretaryPromptDraftWaitingShort", { defaultValue: "Polishing prompt..." });
  const promptDraftReadyTitle = t("voiceSecretaryPromptDraftReadyShort", { defaultValue: "Prompt ready" });
  const documentsCountLabel = t("voiceSecretaryDocumentsCount", { count: documents.length, defaultValue: "{{count}} docs" });
  const askFeedbackStatusLabel = useCallback((status: string) => {
    const key = String(status || "pending").trim().toLowerCase();
    if (key === "working") return t("voiceSecretaryAskStatusWorking", { defaultValue: "Working" });
    if (key === "needs_user") return t("voiceSecretaryAskStatusNeedsUser", { defaultValue: "Needs input" });
    if (key === "failed") return t("voiceSecretaryAskStatusFailed", { defaultValue: "Failed" });
    if (key === "handed_off") return t("voiceSecretaryAskStatusHandedOff", { defaultValue: "Handed off" });
    return t("voiceSecretaryAskStatusPending", { defaultValue: "Queued" });
  }, [t]);
  const askFeedbackStatusClassName = useCallback((status: string) => {
    const key = String(status || "pending").trim().toLowerCase();
    if (key === "needs_user") return isDark ? "bg-amber-400/14 text-amber-100" : "bg-amber-50 text-amber-800";
    if (key === "failed") return isDark ? "bg-rose-400/14 text-rose-100" : "bg-rose-50 text-rose-800";
    if (key === "handed_off") return isDark ? "bg-sky-400/14 text-sky-100" : "bg-sky-50 text-sky-800";
    return isDark ? "bg-white/10 text-slate-200" : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]";
  }, [isDark]);
  const voiceModeLabel = useCallback((mode: VoiceSecretaryCaptureMode) => {
    return assistantRowModeOptions.find((option) => option.key === mode)?.label || mode;
  }, [assistantRowModeOptions]);
  const documentActivityStatusLabel = useCallback((status: VoiceDocumentActivityStatus) => {
    if (status === "updated") return t("voiceSecretaryDocumentActivityUpdated", { defaultValue: "Updated" });
    return t("voiceSecretaryDocumentActivityQueued", { defaultValue: "Queued" });
  }, [t]);
  const currentLiveTranscript = liveTranscriptPreview
    && (recording || activityClockMs - liveTranscriptPreview.updatedAt <= VOICE_LIVE_TRANSCRIPT_VISIBLE_MS)
    ? liveTranscriptPreview
    : null;
  const liveTranscriptDisplayText = currentLiveTranscript
    ? normalizeBrowserTranscriptChunk(currentLiveTranscript.text)
    : "";
  const liveTranscriptSummaryPreview = currentLiveTranscript
    ? compactVoiceTranscriptSummaryText(
      liveTranscriptDisplayText,
    )
    : "";
  const liveTranscriptSummaryText = currentLiveTranscript
    && liveTranscriptSummaryPreview
    ? `${voiceModeLabel(currentLiveTranscript.mode)} · ${liveTranscriptSummaryPreview}`
    : "";
  const recentDocumentActivity = documentActivityItems.find(
    (item) => activityClockMs - item.createdAt <= VOICE_DOCUMENT_ACTIVITY_VISIBLE_MS,
  ) || null;
  const documentActivitySummaryText = recentDocumentActivity
    ? `${documentActivityStatusLabel(recentDocumentActivity.status)} · ${
      recentDocumentActivity.documentTitle || recentDocumentActivity.documentPath || voiceModeLabel(recentDocumentActivity.mode)
    }`
    : "";
  const promptOptimizePending = Boolean(pendingPromptRequestId && !pendingPromptDraft);
  const canOptimizeComposerPrompt = captureMode === "prompt" && !!composerText.trim() && !promptOptimizePending && !pendingPromptDraft;
  const panelRequestSending = actionBusy === "instruct_doc" || actionBusy === "instruct_ask";
  const panelRequestTitle = captureMode === "document"
    ? t("voiceSecretaryDocumentRequestLabel", { defaultValue: "Ask about this document" })
    : captureMode === "instruction"
      ? t("voiceSecretaryAskRequestLabel", { defaultValue: "Ask Voice Secretary" })
      : t("voiceSecretaryPromptRequestLabel", { defaultValue: "Prompt mode" });
  const panelRequestPlaceholder = captureMode === "document"
    ? t("voiceSecretaryDocumentRequestPlaceholder", {
      defaultValue: "Tell Voice Secretary how to refine, split, summarize, or send this document.",
    })
    : captureMode === "instruction"
      ? t("voiceSecretaryAskRequestPlaceholder", {
        defaultValue: "Ask a question or give Voice Secretary a task. This is not tied to the current document.",
      })
      : t("voiceSecretaryPromptRequestDisabledPlaceholder", {
        defaultValue: "Use the sparkle button in the composer capsule to optimize the current input box, or use the record button to add spoken context.",
      });
  const panelRequestButtonLabel = panelRequestSending
      ? t("voiceSecretaryApplyingInstruction", { defaultValue: "Sending..." })
      : captureMode === "instruction"
        ? t("voiceSecretaryAskRequestButton", { defaultValue: "Send ask" })
        : t("voiceSecretaryApplyInstruction", { defaultValue: "Send request" });
  const requestPanelCollapsed = recording && captureMode === "document";
  const requestPanelCollapsedHint = t("voiceSecretaryDocumentRequestCollapsedWhileRecording", {
    defaultValue: "Recording is writing into the document. Requests can be sent after capture stops.",
  });
  const pendingAskFeedback = pendingAskRequestId
    ? askFeedbackItems.find((item) => item.request_id === pendingAskRequestId) || null
    : askFeedbackItems.find((item) => isActiveAskFeedbackStatus(item.status)) || null;
  const canClearAskFeedbackHistory = askFeedbackItems.length > 0 || documentActivityItems.length > 0 || diarizationActivityItems.length > 0 || !!liveTranscriptPreview || voiceStreamItems.length > 0;
  const pendingAskFeedbackStatus = pendingAskFeedback
    ? displayAskFeedbackStatus(pendingAskFeedback, askFeedbackClockMs)
    : "";
  const pendingAskFeedbackText = pendingAskFeedback
    ? askFeedbackDisplayText(pendingAskFeedback)
    : "";
  const pendingAskFeedbackHasFinalReply = hasFinalAskReply(pendingAskFeedback);
  const pendingAskFeedbackStatusText = pendingAskFeedback
    ? pendingAskFeedbackStatus
      ? askFeedbackStatusLabel(pendingAskFeedbackStatus)
      : ""
    : "";
  const pendingAskFeedbackSummaryText = pendingAskFeedback
    ? pendingAskFeedbackHasFinalReply
      ? t("voiceSecretaryReplyReadyShort", { defaultValue: "Reply ready" })
      : pendingAskFeedbackStatusText
        ? pendingAskFeedbackText
          ? `${pendingAskFeedbackStatusText} · ${pendingAskFeedbackText}`
          : pendingAskFeedbackStatusText
        : ""
    : "";
  const showLiveTranscriptSummary = Boolean(
    !pendingPromptDraft
      && !promptDraftWaiting
      && !(pendingAskFeedback && pendingAskFeedbackSummaryText)
      && liveTranscriptSummaryText,
  );
  const showDocumentActivitySummary = Boolean(
    !pendingPromptDraft
      && !promptDraftWaiting
      && !(pendingAskFeedback && pendingAskFeedbackSummaryText)
      && !showLiveTranscriptSummary
      && documentActivitySummaryText,
  );
  const latestVoiceReplyFeedback = useMemo(
    () => askFeedbackItems.find((item) => hasFinalAskReply(item)) || null,
    [askFeedbackItems],
  );
  const latestVoiceReplyFeedbackId = latestVoiceReplyFeedback?.request_id || "";
  const latestVoiceReplyFeedbackText = latestVoiceReplyFeedback?.reply_text || "";
  const latestVoiceReplyDismissKey = voiceReplyDismissKey(latestVoiceReplyFeedback);
  const voiceReplyBubbleFeedback = useMemo(() => {
    const targetId = String(voiceReplyBubbleRequestId || "").trim();
    if (!targetId || !askFeedbackItems.length) return null;
    return askFeedbackItems.find((item) => item.request_id === targetId && hasFinalAskReply(item)) || null;
  }, [askFeedbackItems, voiceReplyBubbleRequestId]);
  const voiceReplyBubbleText = String(voiceReplyBubbleFeedback?.reply_text || "").trim();
  const openVoiceReplyBubble = useCallback((item?: AssistantVoiceAskFeedback | null) => {
    const requestId = String(item?.request_id || "").trim();
    const dismissKey = voiceReplyDismissKey(item);
    if (!requestId || !dismissKey) return;
    dismissedVoiceReplyKeysRef.current.delete(dismissKey);
    setVoiceReplyBubbleRequestId(requestId);
  }, []);
  const closeVoiceReplyBubble = useCallback(() => {
    const dismissKey = voiceReplyDismissKey(voiceReplyBubbleFeedback);
    if (dismissKey) dismissedVoiceReplyKeysRef.current.add(dismissKey);
    setVoiceReplyBubbleRequestId("");
  }, [voiceReplyBubbleFeedback]);
  const copyVoiceReplyBubble = useCallback(async () => {
    const requestId = String(voiceReplyBubbleFeedback?.request_id || "").trim();
    if (!voiceReplyBubbleText || !requestId) return;
    const ok = await copyTextToClipboard(voiceReplyBubbleText);
    if (!ok) {
      showError(t("voiceSecretaryReplyCopyFailed", { defaultValue: "Failed to copy Voice Secretary reply." }));
      return;
    }
    setCopiedVoiceReplyRequestId(requestId);
    showNotice({ message: t("voiceSecretaryReplyCopied", { defaultValue: "Voice Secretary reply copied." }) });
    if (typeof window !== "undefined") {
      window.setTimeout(() => {
        setCopiedVoiceReplyRequestId((current) => current === requestId ? "" : current);
      }, 1800);
    }
  }, [showError, showNotice, t, voiceReplyBubbleFeedback?.request_id, voiceReplyBubbleText]);
  useEffect(() => {
    const requestId = String(latestVoiceReplyFeedbackId || "").trim();
    if (!requestId || !String(latestVoiceReplyFeedbackText || "").trim()) return;
    if (!localVoiceReplyRequestIdsRef.current.has(requestId)) return;
    if (latestVoiceReplyDismissKey && dismissedVoiceReplyKeysRef.current.has(latestVoiceReplyDismissKey)) return;
    setVoiceReplyBubbleRequestId(requestId);
  }, [latestVoiceReplyDismissKey, latestVoiceReplyFeedbackId, latestVoiceReplyFeedbackText]);
  const headerStatusHint = !assistantEnabled
    ? t("voiceSecretaryDisabledHint", {
        defaultValue: "Voice Secretary is off for this group. Enable the assistant here or in Settings > Assistants before recording.",
      })
    : speechError.trim();
  const startAfterEnableRef = useRef(false);
  const assistantRowCurrentMode = assistantRowModeOptions.find((option) => option.key === captureMode) || assistantRowModeOptions[0];
  const modeChangeDisabledReason = recording
    ? t("voiceSecretaryModeChangeDisabledRecording", { defaultValue: "Stop recording before changing mode." })
    : "";
  const workspaceModeHint = captureMode === "prompt"
    ? t("voiceSecretaryWorkspaceHintPrompt", { defaultValue: "Speech is refined into the message composer." })
    : captureMode === "instruction"
      ? t("voiceSecretaryWorkspaceHintInstruction", { defaultValue: "Speech is sent as a request to Voice Secretary." })
      : t("voiceSecretaryWorkspaceHintDocument", { defaultValue: "Speech is written into the default document." });
  const assistantRowControlLabel = recording
    ? t("voiceSecretaryStopAndSave", { defaultValue: "Stop and save recording" })
    : !assistantEnabled
      ? t("voiceSecretaryTurnOnAndRecord", { defaultValue: "Turn on and start recording" })
      : captureStartTitle;
  const promptOptimizeTitle = !assistantEnabled
    ? t("voiceSecretaryEnableFirst", { defaultValue: "Enable Voice Secretary first." })
    : promptOptimizePending
      ? t("voiceSecretaryPromptOptimizingButton", { defaultValue: "Optimizing..." })
      : !composerText.trim()
        ? t("voiceSecretaryPromptOptimizeNeedsText", { defaultValue: "Type a prompt first" })
        : t("voiceSecretaryPromptOptimizeButton", { defaultValue: "Optimize current prompt" });
  useEffect(() => {
    if (!assistantEnabled || !startAfterEnableRef.current) return;
    startAfterEnableRef.current = false;
    void startDictation();
  }, [assistantEnabled, startDictation]);
  const handleAssistantRowModeChange = useCallback((nextMode: VoiceSecretaryCaptureMode) => {
    if (recording) return;
    onCaptureModeChange?.(nextMode);
    setShowAssistantModeMenu(false);
    if (nextMode === "document") {
      setOpen(true);
    } else if (nextMode === "prompt") {
      setOpen(false);
    }
  }, [onCaptureModeChange, recording]);
  const handleAssistantRowRecordClick = useCallback(async (event?: ReactMouseEvent<HTMLButtonElement>) => {
    event?.preventDefault();
    if (recording) {
      stopCurrentRecording();
      return;
    }
    if (captureMode === "document") {
      setOpen(true);
    }
    if (!assistantEnabled) {
      startAfterEnableRef.current = true;
      const enabled = await setAssistantEnabledForGroup(true);
      if (!enabled) startAfterEnableRef.current = false;
      return;
    }
    void startDictation();
  }, [
    assistantEnabled,
    captureMode,
    recording,
    setAssistantEnabledForGroup,
    startDictation,
    stopCurrentRecording,
  ]);
  const handlePromptOptimizeClick = useCallback((event?: ReactMouseEvent<HTMLButtonElement>) => {
    event?.preventDefault();
    if (!canOptimizeComposerPrompt || controlDisabled || !assistantEnabled || !!actionBusy) return;
    void requestPromptRefine("", "composer_prompt_refine", { operation: "replace_with_refined_prompt" });
  }, [
    actionBusy,
    assistantEnabled,
    canOptimizeComposerPrompt,
    controlDisabled,
    requestPromptRefine,
  ]);
  return (
    <div ref={rootRef} className={classNames("relative", isAssistantRow ? "w-auto shrink-0" : "self-end")}>
      {open && typeof document !== "undefined"
        ? createPortal(
          <div
            className="fixed inset-0 z-[180] flex items-end justify-center p-0 sm:items-center sm:p-4"
            aria-hidden={undefined}
          >
            <div
              className="absolute inset-0 glass-overlay"
              onPointerDown={(event) => {
                if (event.target === event.currentTarget) closePanel();
              }}
              aria-hidden="true"
            />
            <section
              ref={modalRef}
              role="dialog"
              aria-modal="true"
              aria-labelledby="voice-secretary-sheet-title"
              aria-describedby="voice-secretary-sheet-description"
              className={classNames(
                "relative z-[181] flex h-[min(92vh,58rem)] w-full max-w-[88rem] flex-col overflow-hidden rounded-t-[28px] border p-3 pt-6 shadow-2xl glass-modal sm:w-[min(94vw,88rem)] sm:rounded-[30px]",
                isDark ? "border-white/10 bg-slate-950/96" : "border-black/10 bg-white/96",
              )}
              onPointerDown={(event) => event.stopPropagation()}
            >
            <div id="voice-secretary-sheet-title" className="sr-only">
              {t("voiceSecretaryTitle", { defaultValue: "Voice Secretary" })}
            </div>
            <div id="voice-secretary-sheet-description" className="sr-only">
              {t("voiceSecretaryWorkspaceHint", {
                defaultValue: "Capture speech, maintain working documents, and ask the secretary to refine or send them.",
              })}
            </div>
            <div className={classNames(
              "shrink-0 border-b px-4 pb-3 pt-2 sm:px-5 sm:pb-3 sm:pt-3",
              isDark ? "border-white/10" : "border-black/10",
            )}>
              <div className="grid gap-3 pr-8 lg:grid-cols-[minmax(16rem,1fr)_auto_auto] lg:items-end">
                <div className="min-w-0">
                  <div className={classNames("text-lg font-semibold tracking-[-0.02em]", isDark ? "text-slate-100" : "text-gray-900")}>
                    {t("voiceSecretaryTitle", { defaultValue: "Voice Secretary" })}
                  </div>
                  <div className={classNames("mt-1 text-xs leading-5", isDark ? "text-slate-400" : "text-gray-500")}>
                    {workspaceModeHint}
                  </div>
                  <div className="mt-3 flex flex-wrap items-center gap-2">
                    <button
                      type="button"
                      role="switch"
                      aria-checked={assistantEnabled}
                      className={classNames(
                        "inline-flex min-h-[34px] items-center gap-2 rounded-full border px-2.5 py-1.5 text-[11px] font-semibold whitespace-nowrap transition-colors disabled:opacity-60",
                        isDark
                          ? "border-white/10 bg-white/[0.04] text-slate-200 hover:bg-white/10"
                          : "border-black/10 bg-white text-gray-700 hover:bg-black/5",
                      )}
                      onClick={() => void setAssistantEnabledForGroup(!assistantEnabled)}
                      disabled={actionBusy === "enable" || !selectedGroupId}
                      title={assistantEnabled
                        ? t("voiceSecretaryTurnOff", { defaultValue: "Turn off" })
                        : t("voiceSecretaryTurnOn", { defaultValue: "Turn on" })}
                    >
                      <span
                        aria-hidden="true"
                        className={classNames(
                          "relative inline-flex h-5 w-9 shrink-0 rounded-full transition-colors",
                          assistantEnabled
                            ? isDark ? "bg-white" : "bg-[rgb(35,36,37)]"
                            : isDark ? "bg-white/15" : "bg-gray-300",
                        )}
                      >
                        <span
                          className={classNames(
                            "absolute top-0.5 h-4 w-4 rounded-full shadow-sm transition-transform",
                            assistantEnabled
                              ? isDark ? "bg-slate-950" : "bg-white"
                              : "bg-white",
                            assistantEnabled ? "translate-x-4" : "translate-x-0.5",
                          )}
                        />
                      </span>
                      <span>
                        {actionBusy === "enable"
                          ? t("voiceSecretarySavingState", { defaultValue: "Saving..." })
                          : assistantEnabled
                            ? t("voiceSecretaryEnabledShort", { defaultValue: "On" })
                            : t("voiceSecretaryDisabledShort", { defaultValue: "Off" })}
                      </span>
                    </button>
                    <span
                      className={classNames(
                        "inline-flex min-h-[34px] items-center rounded-full px-2.5 py-1 text-[11px] font-semibold whitespace-nowrap",
                        isDark ? "bg-white/10 text-slate-200" : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]",
                      )}
                    >
                      {loading ? t("loadingContext", { defaultValue: "Loading context..." }) : statusLabel}
                    </span>
                  </div>
                </div>
                {onCaptureModeChange ? (
                  <div
                    className={classNames(
                      "inline-flex min-h-[38px] w-full items-center rounded-full border p-0.5 sm:w-auto lg:justify-self-center",
                      isDark ? "border-white/10 bg-white/[0.04]" : "border-black/10 bg-white",
                    )}
                    role="group"
                    aria-label={t("voiceSecretaryModeSelector", { defaultValue: "Voice Secretary capture mode" })}
                  >
                    {assistantRowModeOptions.map((option) => {
                      const active = option.key === captureMode;
                      return (
                        <button
                          key={option.key}
                          type="button"
                          className={classNames(
                            "min-w-0 flex-1 rounded-full px-3 py-1.5 text-[11px] font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-45 sm:flex-none",
                            active
                              ? isDark
                                ? "bg-white text-slate-950 shadow-sm"
                                : "bg-[rgb(35,36,37)] text-white shadow-sm"
                              : isDark
                                ? "text-slate-300 hover:bg-white/10 hover:text-white"
                                : "text-gray-600 hover:bg-black/5 hover:text-gray-900",
                          )}
                          onClick={() => handleAssistantRowModeChange(option.key)}
                          disabled={recording || controlDisabled}
                          aria-pressed={active}
                          title={modeChangeDisabledReason || option.description}
                        >
                          {option.label}
                        </button>
                      );
                    })}
                  </div>
                ) : null}
                <div className="flex min-w-0 flex-wrap items-center justify-start gap-2 lg:justify-end">
                  {assistantEnabled ? (
                    <>
                      <label className="inline-flex items-center gap-2 text-[11px] font-semibold text-[var(--color-text-secondary)]">
                        <span>{t("voiceSecretaryLanguage", { defaultValue: "Language" })}</span>
                        <GroupCombobox
                          items={voiceLanguageOptions.map((optionValue) => ({
                            value: optionValue,
                            label: voiceLanguageLabel(optionValue),
                          }))}
                          value={configuredRecognitionLanguage}
                          onChange={(nextValue) => void updateRecognitionLanguage(nextValue)}
                          placeholder={t("voiceSecretaryLanguage", { defaultValue: "Language" })}
                          searchPlaceholder={t("voiceSecretaryLanguage", { defaultValue: "Language" })}
                          emptyText={t("common:noResults", { defaultValue: "No matching results" })}
                          ariaLabel={t("voiceSecretaryLanguage", { defaultValue: "Language" })}
                          triggerClassName={classNames(
                            "min-h-[38px] min-w-[7.5rem] rounded-full border px-3 py-2 text-xs font-semibold transition-colors",
                            isDark
                              ? "border-white/10 bg-white/[0.06] text-slate-100 focus:border-white/30"
                              : "border-black/10 bg-white text-gray-800 focus:border-black/25",
                          )}
                          contentClassName="p-0"
                          disabled={recording || recognitionLanguageSaving}
                          searchable={false}
                          matchTriggerWidth
                        />
                      </label>
                      {serviceAsrReady ? (
                        <>
                          <label className="inline-flex min-w-0 items-center gap-1.5 text-[11px] font-semibold text-[var(--color-text-secondary)]">
                            <span className="hidden xl:inline">{t("voiceSecretaryMicDevice", { defaultValue: "Microphone" })}</span>
                            <select
                              value={selectedAudioDeviceId}
                              onChange={(event) => setSelectedAudioDeviceId(event.target.value)}
                              className={classNames(
                                "h-[38px] w-[11.5rem] truncate rounded-full border px-3 py-1.5 text-xs font-semibold outline-none transition-colors sm:w-[13rem] lg:w-[14rem]",
                                isDark
                                  ? "border-white/10 bg-white/[0.06] text-slate-100 focus:border-white/30"
                                  : "border-black/10 bg-white text-gray-800 focus:border-black/25",
                              )}
                              disabled={recording || !!actionBusy}
                              aria-label={t("voiceSecretaryMicDevice", { defaultValue: "Microphone" })}
                              title={selectedAudioDeviceLabel}
                            >
                              <option value="">
                                {t("voiceSecretaryDefaultMic", { defaultValue: "System default microphone" })}
                              </option>
                              {audioDevices.map((device, index) => (
                                <option key={device.deviceId || `audio-${index}`} value={device.deviceId}>
                                  {device.label || t("voiceSecretaryMicDeviceFallback", {
                                    index: index + 1,
                                    defaultValue: "Microphone {{index}}",
                                  })}
                                </option>
                              ))}
                            </select>
                          </label>
                          <button
                            type="button"
                            className={classNames(
                              "inline-flex h-[38px] w-[38px] items-center justify-center rounded-full border text-[var(--color-text-secondary)] transition-colors disabled:opacity-60",
                              isDark ? "border-white/10 text-slate-300 hover:bg-white/10" : "border-black/10 bg-white text-gray-700 hover:bg-black/5",
                            )}
                            onClick={() => void loadAudioDevices()}
                            disabled={recording || !!actionBusy}
                            aria-label={t("voiceSecretaryRefreshDevices", { defaultValue: "Refresh devices" })}
                            title={t("voiceSecretaryRefreshDevices", { defaultValue: "Refresh devices" })}
                          >
                            <RefreshIcon size={15} aria-hidden="true" />
                          </button>
                        </>
                      ) : null}
                      <button
                        type="button"
                        className={classNames(
                          "inline-flex min-h-[38px] items-center gap-2 rounded-full border px-3 py-2 text-xs font-semibold whitespace-nowrap transition-colors disabled:opacity-60",
                          recording
                            ? isDark
                              ? "border-rose-300/35 bg-rose-500/15 text-rose-100 hover:bg-rose-500/22"
                              : "border-rose-200 bg-rose-50 text-rose-800 hover:bg-rose-100"
                            : isDark
                              ? "border-white/10 bg-white/[0.06] text-slate-100 hover:bg-white/10"
                              : "border-black/10 bg-white text-gray-800 hover:bg-black/5",
                        )}
                        onClick={(event) => {
                          event.preventDefault();
                          if (recording) stopCurrentRecording();
                          else void startDictation();
                        }}
                        disabled={!!actionBusy || (!recording && !dictationSupported)}
                        title={recording
                          ? t("voiceSecretaryStopAndSave", { defaultValue: "Stop and save recording" })
                          : captureStartTitle}
                      >
                        <span
                          aria-hidden="true"
                          className={classNames(
                            "inline-flex h-5 w-5 items-center justify-center rounded-full",
                            recording
                              ? isDark ? "bg-rose-300/15" : "bg-white"
                              : isDark ? "bg-rose-400/15 text-rose-100" : "bg-rose-50 text-rose-700",
                          )}
                        >
                          {recording ? <StopIcon size={12} /> : <span className="h-2.5 w-2.5 rounded-full bg-rose-600" />}
                        </span>
                        {recording
                          ? t("voiceSecretaryStopAndSaveShort", { defaultValue: "Stop & save" })
                          : workspaceRecordLabel}
                      </button>
                    </>
                  ) : null}
                </div>
              </div>

              <div className="mt-2">
                {headerStatusHint ? (
                  <div className={classNames("text-[11px] leading-5", isDark ? "text-slate-400" : "text-gray-500")}>
                    {headerStatusHint}
                  </div>
                ) : null}
              </div>
            </div>

            {promptDraftWaiting ? (
              <div
                className={classNames(
                  "mx-4 mt-4 flex shrink-0 flex-wrap items-center justify-between gap-2 rounded-2xl border px-3 py-2 sm:mx-5",
                  isDark
                    ? "border-amber-200/15 bg-[linear-gradient(135deg,rgba(245,158,11,0.14),rgba(251,191,36,0.07),rgba(255,255,255,0.03))] text-amber-50"
                    : "border-amber-200 bg-[linear-gradient(135deg,rgba(255,251,235,1),rgba(255,247,237,0.96),rgba(250,250,249,0.94))] text-amber-950",
                )}
              >
                <div className="min-w-0 flex-1 truncate text-xs font-semibold">
                  <AnimatedShinyText
                    className={classNames(
                      isDark
                        ? "bg-[linear-gradient(110deg,rgba(254,243,199,0.78)_18%,rgba(255,255,255,0.98)_48%,rgba(251,191,36,0.92)_68%,rgba(254,243,199,0.78)_84%)]"
                        : "bg-[linear-gradient(110deg,rgb(120,53,15)_18%,rgb(245,158,11)_44%,rgb(255,255,255)_52%,rgb(180,83,9)_66%,rgb(120,53,15)_84%)]",
                    )}
                  >
                    {promptDraftWaitingTitle}
                  </AnimatedShinyText>
                </div>
              </div>
            ) : null}

            <div className="grid min-h-0 flex-1 grid-cols-1 gap-4 overflow-auto scrollbar-hide px-4 py-4 sm:px-5 sm:py-5 lg:grid-cols-[15rem_minmax(0,1fr)_18rem] lg:overflow-hidden">
            <VoiceSecretaryDocumentListPanel
              actionBusy={actionBusy}
              activeDocumentPath={String(activeDocumentWritePath || viewedDocumentPath || "").trim()}
              captureTargetDocumentPath={String(captureTargetDocumentPath || "").trim()}
              creatingDocument={creatingDocument}
              documents={documents}
              documentsCountLabel={documentsCountLabel}
              isDark={isDark}
              newDocumentTitleDraft={newDocumentTitleDraft}
              t={t}
              documentKey={voiceDocumentKey}
              documentPath={voiceDocumentPath}
              onArchiveDocument={(document) => void archiveDocument(document)}
              onCancelCreateDocument={cancelCreateDocument}
              onCreateDocument={() => void createDocument()}
              onNewDocumentTitleChange={setNewDocumentTitleDraft}
              onSelectDocument={(document) => void selectDocument(document)}
              onSetCaptureTargetDocument={(document) => void setCaptureTargetDocument(document)}
              onStartCreateDocument={startCreateDocument}
            />

            <VoiceSecretaryWorkspacePanel
              activeDocumentPath={activeDocumentPath}
              activeDocumentWritePath={activeDocumentWritePath}
              actionBusy={actionBusy}
              canClearActivity={canClearAskFeedbackHistory}
              captureTargetDocumentPath={captureTargetDocumentPath}
              documentDisplayTitle={documentDisplayTitle}
              documentDraft={documentDraft}
              documentEditing={documentEditing}
              documentHasUnsavedEdits={documentHasUnsavedEdits}
              documentRemoteChanged={documentRemoteChanged}
              isDark={isDark}
              latestDiarization={diarizationActivityItems[0] || null}
              liveTranscriptId={liveTranscriptPreview?.id || ""}
              recording={recording}
              t={t}
              voiceStreamItems={voiceStreamItems}
              view={voiceWorkspaceView}
              onChangeView={setVoiceWorkspaceView}
              onClearActivity={() => void clearAskFeedbackHistory()}
              onDownloadDocument={downloadCurrentDocument}
              onEditDocumentChange={updateDocumentDraft}
              onLoadLatestDocument={() => loadDocumentDraft(activeDocument)}
              onSaveDocument={() => void saveDocument()}
              onToggleDocumentEditing={() => setDocumentEditing((value) => !value)}
              voiceModeLabel={voiceModeLabel}
              formatTime={formatVoiceActivityTimeMs}
              formatFullTime={formatVoiceActivityFullTimeMs}
              formatOffset={formatVoiceOffsetMs}
              normalizeTranscriptText={normalizeBrowserTranscriptChunk}
            />

            <VoiceSecretaryRequestPanel
              actionBusy={actionBusy}
              captureMode={captureMode}
              documentInstruction={documentInstruction}
              isDark={isDark}
              panelRequestButtonLabel={panelRequestButtonLabel}
              panelRequestCollapsed={requestPanelCollapsed}
              panelRequestCollapsedHint={requestPanelCollapsedHint}
              panelRequestPlaceholder={panelRequestPlaceholder}
              panelRequestTitle={panelRequestTitle}
              t={t}
              onDocumentInstructionChange={setDocumentInstruction}
              onSendPanelRequest={() => void sendPanelRequest()}
            />
          </div>
          <button
            type="button"
            onClick={closePanel}
            className="absolute right-3 top-3 rounded-md p-1 text-[var(--color-text-muted)] transition-colors hover:text-[var(--color-text-primary)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]/45 sm:right-4 sm:top-4"
            aria-label={t("voiceSecretaryClose", { defaultValue: "Close Voice Secretary" })}
          >
            <span aria-hidden="true">×</span>
          </button>
            </section>
          </div>,
          document.body,
        )
        : null}

      {isAssistantRow ? (
        <div className="relative inline-flex max-w-full items-center gap-1.5">
          {voiceReplyBubbleFeedback && voiceReplyBubbleText ? (
            <div
              className={classNames(
                "absolute bottom-full left-0 z-[80] mb-2 w-[min(28rem,calc(100vw-1.5rem))] overflow-hidden rounded-2xl border shadow-2xl",
                isDark
                  ? "border-white/10 bg-[rgb(18,22,28)] text-slate-100 shadow-black/40"
                  : "border-black/10 bg-white text-gray-900 shadow-black/15",
              )}
              role="dialog"
              aria-label={t("voiceSecretaryReplyBubbleTitle", { defaultValue: "Voice Secretary reply" })}
            >
              <div
                className={classNames(
                  "flex items-center justify-between gap-2 border-b px-3 py-2",
                  isDark ? "border-white/[0.08]" : "border-black/[0.06]",
                )}
              >
                <div className="min-w-0">
                  <div className="truncate text-[11px] font-semibold">
                    {t("voiceSecretaryReplyBubbleTitle", { defaultValue: "Voice Secretary reply" })}
                  </div>
                  <div className={classNames("mt-0.5 text-[10px]", isDark ? "text-slate-400" : "text-gray-500")}>
                    {displayAskFeedbackStatus(voiceReplyBubbleFeedback, askFeedbackClockMs)
                      ? askFeedbackStatusLabel(displayAskFeedbackStatus(voiceReplyBubbleFeedback, askFeedbackClockMs))
                      : t("voiceSecretaryReplyReadyShort", { defaultValue: "Reply ready" })}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  <button
                    type="button"
                    onClick={() => void copyVoiceReplyBubble()}
                    className={classNames(
                      "inline-flex h-7 w-7 items-center justify-center rounded-md transition-colors",
                      isDark ? "text-slate-300 hover:bg-white/10 hover:text-white" : "text-gray-500 hover:bg-black/5 hover:text-gray-900",
                    )}
                    aria-label={t("copy", { defaultValue: "Copy" })}
                    title={t("copy", { defaultValue: "Copy" })}
                  >
                    {copiedVoiceReplyRequestId === voiceReplyBubbleFeedback.request_id ? (
                      <span className="text-[11px] font-bold" aria-hidden="true">✓</span>
                    ) : (
                      <CopyIcon size={14} aria-hidden="true" />
                    )}
                  </button>
                  <button
                    type="button"
                    onClick={closeVoiceReplyBubble}
                    className={classNames(
                      "inline-flex h-7 w-7 items-center justify-center rounded-md transition-colors",
                      isDark ? "text-slate-300 hover:bg-white/10 hover:text-white" : "text-gray-500 hover:bg-black/5 hover:text-gray-900",
                    )}
                    aria-label={t("close", { defaultValue: "Close" })}
                    title={t("close", { defaultValue: "Close" })}
                  >
                    <CloseIcon size={14} aria-hidden="true" />
                  </button>
                </div>
              </div>
              <div className="max-h-[18rem] overflow-auto px-3 py-2.5 text-[12px] leading-5">
                <LazyMarkdownRenderer
                  content={voiceReplyBubbleText}
                  isDark={isDark}
                  className="break-words [overflow-wrap:anywhere] max-w-full [&_p]:my-1 [&_li]:my-0.5 [&_pre]:my-2"
                  fallback={<div className="whitespace-pre-wrap break-words">{voiceReplyBubbleText}</div>}
                />
              </div>
            </div>
          ) : null}
          <div
            className={classNames(
              "inline-flex h-9 shrink-0 items-center gap-0.5 rounded-lg border p-0.5 transition-colors",
              recording
                ? isDark
                  ? "border-rose-400/30 bg-rose-500/10"
                  : "border-rose-200 bg-rose-50/70"
                : "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)]",
            )}
            role="group"
            aria-label={t("voiceSecretaryTitle", { defaultValue: "Voice Secretary" })}
          >
            <button
              type="button"
              className={classNames(
                "relative inline-flex h-8 w-8 items-center justify-center rounded-md transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                recording
                  ? isDark
                    ? "bg-rose-500/20 text-rose-200 hover:bg-rose-500/28"
                    : "bg-rose-500 text-white shadow-sm hover:bg-rose-600"
                  : !dictationSupported
                    ? "text-[var(--color-text-tertiary)]"
                    : isDark
                      ? "text-[var(--color-text-secondary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                      : "text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-gray-900",
                !controlDisabled && !actionBusy && "active:scale-[0.96]",
              )}
              onClick={(event) => void handleAssistantRowRecordClick(event)}
              disabled={!!actionBusy || controlDisabled || (!recording && !dictationSupported)}
              aria-pressed={recording}
              aria-label={assistantRowControlLabel}
              title={`${assistantRowControlLabel} · ${assistantRowCurrentMode.label}`}
            >
              {recording ? (
                <StopIcon size={13} aria-hidden="true" />
              ) : (
                <MicrophoneIcon size={15} aria-hidden="true" />
              )}
            </button>
            {onCaptureModeChange ? (
              <Popover open={showAssistantModeMenu} onOpenChange={setShowAssistantModeMenu}>
                <PopoverTrigger asChild>
                  <button
                    type="button"
                    className={classNames(
                      "inline-flex h-8 shrink-0 items-center justify-center gap-1 rounded-md px-2 text-[11px] font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                      isDark
                        ? "text-[var(--color-text-secondary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                        : "text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-gray-900",
                    )}
                    disabled={controlDisabled || recording}
                    title={modeChangeDisabledReason || `${t("voiceSecretaryModeSelector", { defaultValue: "Voice Secretary capture mode" })}: ${assistantRowCurrentMode.label}`}
                    aria-label={t("voiceSecretaryModeSelector", { defaultValue: "Voice Secretary capture mode" })}
                  >
                    <span className="min-w-0 truncate">{assistantRowCurrentMode.label}</span>
                    <ChevronDownIcon size={12} aria-hidden="true" />
                  </button>
                </PopoverTrigger>
                <PopoverContent
                  align="start"
                  sideOffset={6}
                  className="w-56 rounded-2xl p-1.5"
                >
                  <div
                    role="menu"
                    aria-label={t("voiceSecretaryModeSelector", { defaultValue: "Voice Secretary capture mode" })}
                  >
                    {assistantRowModeOptions.map((option) => {
                      const active = option.key === captureMode;
                      return (
                        <button
                          key={option.key}
                          type="button"
                          className={classNames(
                            "w-full rounded-xl px-3 py-2.5 text-left flex items-center gap-2.5 transition-colors",
                            active
                              ? isDark
                                ? "bg-white/10"
                                : "bg-black/5"
                              : isDark
                                ? "hover:bg-white/5"
                                : "hover:bg-black/5",
                          )}
                          role="menuitemradio"
                          aria-checked={active}
                          disabled={recording}
                          title={modeChangeDisabledReason || option.description}
                          onPointerDown={(event) => {
                            event.preventDefault();
                            handleAssistantRowModeChange(option.key);
                          }}
                        >
                          <span
                            className={classNames(
                              "w-6 h-6 rounded-md flex items-center justify-center flex-shrink-0",
                              option.key === "document"
                                ? isDark
                                  ? "bg-slate-700 text-slate-200"
                                  : "bg-gray-100 text-gray-700"
                                : option.key === "instruction"
                                  ? isDark
                                    ? "bg-emerald-500/20 text-emerald-100"
                                    : "bg-emerald-50 text-emerald-700"
                                : isDark
                                  ? "bg-indigo-500/25 text-indigo-200"
                                  : "bg-indigo-100 text-indigo-700",
                            )}
                          >
                            {option.key === "document" ? (
                              <MicrophoneIcon size={13} />
                            ) : option.key === "instruction" ? (
                              <span className="text-[12px] font-black leading-none">?</span>
                            ) : (
                              <span className="text-[11px] font-black italic leading-none">P</span>
                            )}
                          </span>
                          <span className="min-w-0 flex-1">
                            <span className={classNames("block text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>
                              {option.label}
                            </span>
                            <span className={classNames("block text-[11px]", isDark ? "text-[var(--color-text-tertiary)]" : "text-gray-500")}>
                              {option.description}
                            </span>
                          </span>
                          {active ? (
                            <span className={classNames("text-xs font-semibold", isDark ? "text-slate-200" : "text-[rgb(35,36,37)]")}>✓</span>
                          ) : null}
                        </button>
                      );
                    })}
                  </div>
                </PopoverContent>
              </Popover>
            ) : null}
            {captureMode === "prompt" ? (
              <button
                type="button"
                className={classNames(
                  "inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                  promptOptimizePending
                    ? isDark
                      ? "bg-amber-400/12 text-amber-100"
                      : "bg-amber-50 text-amber-800"
                    : isDark
                      ? "text-[var(--color-text-secondary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                      : "text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-gray-900",
                  !controlDisabled && !actionBusy && canOptimizeComposerPrompt && "active:scale-[0.96]",
                )}
                onClick={(event) => handlePromptOptimizeClick(event)}
                disabled={controlDisabled || !!actionBusy || !assistantEnabled || !canOptimizeComposerPrompt}
                aria-label={promptOptimizeTitle}
                title={promptOptimizeTitle}
              >
                <SparklesIcon size={15} aria-hidden="true" />
              </button>
            ) : null}
            <Popover open={showAssistantLanguageMenu} onOpenChange={setShowAssistantLanguageMenu}>
              <PopoverTrigger asChild>
                <button
                  type="button"
                  className={classNames(
                    "inline-flex h-8 shrink-0 items-center justify-center rounded-md px-1.5 text-[10px] font-bold tracking-[0.08em] transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                    isDark
                      ? "text-[var(--color-text-secondary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                      : "text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-gray-900",
                  )}
                  disabled={controlDisabled || !assistantEnabled || recording || recognitionLanguageSaving}
                  title={`${t("voiceSecretaryLanguage", { defaultValue: "Language" })}: ${configuredRecognitionLanguageLabel}`}
                  aria-label={`${t("voiceSecretaryLanguage", { defaultValue: "Language" })}: ${configuredRecognitionLanguageLabel}`}
                >
                  {configuredRecognitionLanguageShortLabel}
                </button>
              </PopoverTrigger>
              <PopoverContent
                align="start"
                sideOffset={6}
                className="w-52 rounded-2xl p-1.5"
              >
                <div
                  role="menu"
                  aria-label={t("voiceSecretaryLanguage", { defaultValue: "Language" })}
                >
                  {voiceLanguageOptions.map((optionValue) => {
                    const active = optionValue === configuredRecognitionLanguage;
                    return (
                      <button
                        key={optionValue}
                        type="button"
                        className={classNames(
                          "flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left transition-colors",
                          active
                            ? isDark
                              ? "bg-white/10"
                              : "bg-black/5"
                            : isDark
                              ? "hover:bg-white/5"
                              : "hover:bg-black/5",
                        )}
                        role="menuitemradio"
                        aria-checked={active}
                        onPointerDown={(event) => {
                          event.preventDefault();
                          setShowAssistantLanguageMenu(false);
                          void updateRecognitionLanguage(optionValue);
                        }}
                      >
                        <span
                          className={classNames(
                            "flex h-6 w-8 shrink-0 items-center justify-center rounded-md text-[10px] font-bold tracking-[0.08em]",
                            isDark ? "bg-white/10 text-slate-200" : "bg-gray-100 text-gray-700",
                          )}
                        >
                          {voiceLanguageShortLabel(optionValue)}
                        </span>
                        <span className={classNames("min-w-0 flex-1 truncate text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>
                          {voiceLanguageLabel(optionValue)}
                        </span>
                        {active ? (
                          <span className={classNames("text-xs font-semibold", isDark ? "text-slate-200" : "text-[rgb(35,36,37)]")}>✓</span>
                        ) : null}
                      </button>
                    );
                  })}
                </div>
              </PopoverContent>
            </Popover>
            <button
              type="button"
              className={classNames(
                "inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                open
                  ? isDark
                    ? "bg-white/10 text-[var(--color-text-primary)]"
                    : "bg-black/5 text-gray-900"
                  : isDark
                    ? "text-[var(--color-text-secondary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                    : "text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-gray-900",
                !controlDisabled && "active:scale-[0.96]",
              )}
              onClick={() => {
                if (open) closePanel();
                else setOpen(true);
              }}
              disabled={controlDisabled}
              aria-pressed={open}
              aria-label={openButtonLabel}
              title={openButtonLabel}
            >
              <MaximizeIcon size={15} />
            </button>
          </div>
          {pendingPromptDraft || promptDraftWaiting ? (
            <div
              className={classNames(
                "inline-flex max-w-[min(34rem,calc(100vw-12rem))] items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px]",
                pendingPromptDraft
                  ? isDark
                    ? "bg-emerald-400/12 text-emerald-100"
                    : "bg-emerald-50 text-emerald-900"
                  : isDark
                    ? "bg-amber-400/12 text-amber-100"
                    : "bg-amber-50 text-amber-900",
              )}
            >
              <div
                className="min-w-0 whitespace-normal break-words font-semibold leading-4"
                style={TWO_LINE_STATUS_STYLE}
              >
                {promptDraftWaiting ? (
                  <AnimatedShinyText
                    className={classNames(
                      isDark
                        ? "bg-[linear-gradient(110deg,rgba(254,243,199,0.82)_18%,rgba(255,255,255,0.98)_48%,rgba(251,191,36,0.94)_68%,rgba(254,243,199,0.82)_84%)]"
                        : "bg-[linear-gradient(110deg,rgb(120,53,15)_18%,rgb(217,119,6)_42%,rgb(255,255,255)_52%,rgb(146,64,14)_66%,rgb(120,53,15)_84%)]",
                    )}
                  >
                    {promptDraftWaitingTitle}
                  </AnimatedShinyText>
                ) : (
                  promptDraftReadyTitle
                )}
              </div>
            </div>
          ) : null}
          {pendingAskFeedback && pendingAskFeedbackSummaryText ? (
            <button
              type="button"
              className={classNames(
                "inline-flex max-w-[min(34rem,calc(100vw-12rem))] items-start rounded-full px-2.5 py-1 text-left text-[11px] transition-opacity",
                askFeedbackStatusClassName(pendingAskFeedbackStatus),
                pendingAskFeedbackHasFinalReply
                  ? "cursor-pointer hover:opacity-85"
                  : "cursor-default",
              )}
              aria-live="polite"
              onClick={() => openVoiceReplyBubble(pendingAskFeedback)}
              disabled={!pendingAskFeedbackHasFinalReply}
              title={pendingAskFeedbackHasFinalReply
                ? t("voiceSecretaryOpenReply", { defaultValue: "Open Voice Secretary reply" })
                : undefined}
            >
              <span
                className="min-w-0 whitespace-normal break-words font-semibold leading-4"
                style={TWO_LINE_STATUS_STYLE}
              >
                {pendingAskFeedbackSummaryText}
              </span>
            </button>
          ) : null}
          {showLiveTranscriptSummary && currentLiveTranscript ? (
            <div
              className={classNames(
                "inline-flex max-w-[min(40rem,calc(100vw-10rem))] items-start rounded-full px-2.5 py-1 text-left text-[11px]",
                isDark ? "bg-cyan-400/12 text-cyan-100" : "bg-cyan-50 text-cyan-900",
              )}
              aria-live="polite"
            >
              <span
                className="min-w-0 whitespace-normal break-words font-semibold leading-4"
                style={TWO_LINE_STATUS_STYLE}
              >
                {liveTranscriptSummaryText}
              </span>
            </div>
          ) : null}
          {showDocumentActivitySummary ? (
            <div
              className={classNames(
                "inline-flex max-w-[min(34rem,calc(100vw-12rem))] items-start rounded-full px-2.5 py-1 text-left text-[11px]",
                isDark ? "bg-white/10 text-slate-200" : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]",
              )}
              aria-live="polite"
            >
              <span
                className="min-w-0 whitespace-normal break-words font-semibold leading-4"
                style={TWO_LINE_STATUS_STYLE}
              >
                {documentActivitySummaryText}
              </span>
            </div>
          ) : null}
        </div>
      ) : (
        <button
          type="button"
          className={classNames(
            buttonClassName || classNames(
              "glass-btn flex items-center justify-center rounded-lg transition-colors disabled:cursor-not-allowed disabled:opacity-60",
              controlDisabled
                ? "text-[var(--color-text-tertiary)]"
                : recording
                  ? isDark
                    ? "border-rose-400/25 bg-rose-500/15 text-rose-100 hover:bg-rose-500/22"
                    : "border-rose-200 bg-rose-50 text-rose-700 hover:bg-rose-100"
                  : isDark
                    ? "text-[var(--color-text-secondary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                    : "text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-gray-800",
            ),
            "relative",
            recording && buttonClassName
              ? isDark
                ? "!text-rose-300"
                : "!text-rose-600"
              : "",
          )}
          style={buttonClassName ? undefined : { width: `${buttonSizePx}px`, height: `${buttonSizePx}px` }}
          onClick={() => {
            if (open) closePanel();
            else setOpen(true);
          }}
          disabled={controlDisabled}
          aria-pressed={open}
          aria-label={openButtonLabel}
          title={openButtonLabel}
        >
          <MicrophoneIcon size={openButtonIconSizePx} className="transition-transform" />
          {recording ? (
            <span
              aria-hidden="true"
              className={classNames(
                "absolute right-1.5 top-1.5 h-2 w-2 rounded-full",
                "bg-rose-500 shadow-[0_0_0_3px_rgba(244,63,94,0.16)]",
              )}
            />
          ) : null}
        </button>
      )}
    </div>
  );
}
