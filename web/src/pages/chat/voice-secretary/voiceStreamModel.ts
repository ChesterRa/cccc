export type VoiceStreamCaptureMode = "document" | "instruction" | "prompt";

export type VoiceTranscriptPreviewPhase = "interim" | "final";

export type VoiceTranscriptPreview = {
  id: string;
  phase: VoiceTranscriptPreviewPhase;
  text: string;
  pendingFinalText?: string;
  interimText?: string;
  mode: VoiceStreamCaptureMode;
  documentTitle?: string;
  documentPath?: string;
  language?: string;
  startMs?: number;
  endMs?: number;
  updatedAt: number;
};

export type VoiceStreamItem = VoiceTranscriptPreview & {
  createdAt: number;
};

export type VoiceSpeakerSegment = {
  speaker_label?: string;
  start_ms?: number;
  end_ms?: number;
};

export type VoiceSpeakerTranscriptSegment = VoiceSpeakerSegment & {
  text?: string;
};

export type VoiceDiarizationStreamItem = {
  id: string;
  status: "working" | "provisional" | "ready" | "failed";
  speakerCount: number;
  preview: string;
  segments?: VoiceSpeakerSegment[];
  speakerTranscriptSegments?: VoiceSpeakerTranscriptSegment[];
  artifactPath?: string;
  error?: string;
  createdAt: number;
};

export type VoiceStreamMetadata = {
  mode: VoiceStreamCaptureMode;
  documentTitle?: string;
  documentPath?: string;
  language?: string;
};

export type VoiceStreamTiming = {
  startMs?: number;
  endMs?: number;
};

export type SpeakerConversationItem = {
  id: string;
  sourceItemId: string;
  phase: VoiceTranscriptPreviewPhase;
  text: string;
  speakerLabel: string;
  mode: VoiceStreamCaptureMode;
  documentTitle?: string;
  documentPath?: string;
  language?: string;
  startMs?: number;
  endMs?: number;
  createdAt: number;
  updatedAt: number;
};

type TimedSpeakerSlice = {
  speakerLabel: string;
  startMs: number;
  endMs: number;
};

export function createVoiceTranscriptPreview(params: {
  id: string;
  cleanText: string;
  phase: VoiceTranscriptPreviewPhase;
  pendingFinalText: string;
  metadata: VoiceStreamMetadata;
  timing?: VoiceStreamTiming;
  now: number;
}): VoiceTranscriptPreview {
  const interimText = params.phase === "interim" ? params.cleanText : "";
  const text = params.pendingFinalText
    ? interimText
      ? `${params.pendingFinalText}\n${interimText}`
      : params.pendingFinalText
    : params.cleanText;
  return {
    id: params.id,
    phase: params.phase,
    text,
    pendingFinalText: params.pendingFinalText,
    interimText,
    ...params.metadata,
    startMs: params.timing?.startMs,
    endMs: params.timing?.endMs,
    updatedAt: params.now,
  };
}

export function createVoiceStreamMessage(params: {
  id: string;
  cleanText: string;
  metadata: VoiceStreamMetadata;
  timing?: VoiceStreamTiming;
  now: number;
}): VoiceStreamItem {
  return {
    id: params.id,
    phase: "final",
    text: params.cleanText,
    ...params.metadata,
    startMs: params.timing?.startMs,
    endMs: params.timing?.endMs,
    createdAt: params.now,
    updatedAt: params.now,
  };
}

export function upsertLiveVoiceStreamItem(
  currentItems: VoiceStreamItem[],
  preview: VoiceTranscriptPreview,
  maxItems = 30,
): VoiceStreamItem[] {
  const existing = currentItems.find((item) => item.id === preview.id);
  const nextItem: VoiceStreamItem = {
    ...preview,
    createdAt: existing?.createdAt || preview.updatedAt,
  };
  return [
    nextItem,
    ...currentItems.filter((item) => item.id !== preview.id),
  ].slice(0, maxItems);
}

export function appendFinalVoiceStreamItem(
  currentItems: VoiceStreamItem[],
  item: VoiceStreamItem,
  liveItemId = "",
  maxItems = 80,
): VoiceStreamItem[] {
  return [
    item,
    ...currentItems.filter((existing) => (
      existing.id !== liveItemId
      && existing.id !== item.id
      && !voiceStreamItemsLookDuplicated(existing, item)
    )),
  ].slice(0, maxItems);
}

export function buildSpeakerConversationItems(
  voiceStreamItems: VoiceStreamItem[],
  _speakerSegments: VoiceSpeakerSegment[],
  speakerTranscriptSegments: VoiceSpeakerTranscriptSegment[] = [],
): SpeakerConversationItem[] {
  const speakerTranscriptItems = buildSpeakerTranscriptConversationItems(speakerTranscriptSegments);
  if (speakerTranscriptItems.length) return coalesceConversationItems(speakerTranscriptItems);
  const streamItems = sortVoiceStreamItemsForDisplay(voiceStreamItems);
  if (!streamItems.length) return [];
  return coalesceConversationItems(buildRawStreamConversationItems(streamItems));
}

export function filterVoiceStreamItemsForDocument(
  voiceStreamItems: VoiceStreamItem[],
  documentPath: string,
): VoiceStreamItem[] {
  const targetPath = String(documentPath || "").trim();
  if (!targetPath) return voiceStreamItems;
  return voiceStreamItems.filter((item) => String(item.documentPath || "").trim() === targetPath);
}

function buildSpeakerTranscriptConversationItems(
  speakerTranscriptSegments: VoiceSpeakerTranscriptSegment[],
): SpeakerConversationItem[] {
  return speakerTranscriptSegments
    .map((segment, index): SpeakerConversationItem | null => {
      const text = String(segment.text || "").trim();
      const startMs = Number(segment.start_ms);
      const endMs = Number(segment.end_ms);
      const speakerLabel = String(segment.speaker_label || "").trim();
      if (!text || !speakerLabel || !Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs <= startMs) return null;
      return {
        id: `speaker-transcript:${index}:${speakerLabel}:${startMs}:${endMs}`,
        sourceItemId: `speaker-transcript:${index}`,
        phase: "final" as const,
        text,
        speakerLabel,
        mode: "document",
        startMs,
        endMs,
        createdAt: endMs,
        updatedAt: endMs,
      };
    })
    .filter((item): item is SpeakerConversationItem => item !== null)
    .sort((left, right) => Number(right.startMs || 0) - Number(left.startMs || 0));
}

function sortVoiceStreamItemsForDisplay(items: VoiceStreamItem[]): VoiceStreamItem[] {
  const sorted = [...items].sort((left, right) => {
    const leftLive = left.phase === "interim" ? 1 : 0;
    const rightLive = right.phase === "interim" ? 1 : 0;
    if (leftLive !== rightLive) return rightLive - leftLive;
    return right.updatedAt - left.updatedAt || right.createdAt - left.createdAt;
  });
  return dedupeVoiceStreamItemsForDisplay(sorted);
}

function buildRawStreamConversationItems(items: VoiceStreamItem[]): SpeakerConversationItem[] {
  return items
    .map((item, index) => {
      const text = String(item.text || "").trim();
      if (!text) return null;
      return toConversationItem(item, text, "", item.startMs, item.endMs, index);
    })
    .filter((item): item is SpeakerConversationItem => item !== null);
}

export function splitVoiceStreamItemBySpeakers(
  item: VoiceStreamItem,
  speakerSegments: VoiceSpeakerSegment[],
): SpeakerConversationItem[] {
  const text = String(item.text || "").trim();
  if (!text) return [];
  const startMs = Number(item.startMs);
  const endMs = Number(item.endMs);
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || endMs <= startMs) {
    return [toConversationItem(item, text, "", item.startMs, item.endMs, 0)];
  }
  const timedSlices = buildTimedSpeakerSlices(startMs, endMs, speakerSegments);
  if (!timedSlices.length) return [toConversationItem(item, text, "", startMs, endMs, 0)];
  return splitTextAcrossTimedSlices(text, startMs, endMs, timedSlices)
    .map((slice, index) => toConversationItem(
      item,
      slice.text,
      slice.speakerLabel,
      slice.startMs,
      slice.endMs,
      index,
    ))
    .filter((slice) => slice.text.length > 0);
}

function buildTimedSpeakerSlices(
  itemStartMs: number,
  itemEndMs: number,
  speakerSegments: VoiceSpeakerSegment[],
): TimedSpeakerSlice[] {
  const clippedSegments = speakerSegments
    .map((segment) => {
      const startMs = Number(segment.start_ms);
      const endMs = Number(segment.end_ms);
      const speakerLabel = String(segment.speaker_label || "").trim();
      if (!speakerLabel || !Number.isFinite(startMs) || !Number.isFinite(endMs)) return null;
      const clippedStartMs = Math.max(itemStartMs, startMs);
      const clippedEndMs = Math.min(itemEndMs, endMs);
      if (clippedEndMs <= clippedStartMs) return null;
      return { speakerLabel, startMs: clippedStartMs, endMs: clippedEndMs };
    })
    .filter((segment): segment is TimedSpeakerSlice => Boolean(segment))
    .sort((left, right) => left.startMs - right.startMs || left.endMs - right.endMs);

  const slices: TimedSpeakerSlice[] = [];
  let cursorMs = itemStartMs;
  clippedSegments.forEach((segment) => {
    if (segment.startMs > cursorMs) {
      slices.push({ speakerLabel: "", startMs: cursorMs, endMs: segment.startMs });
    }
    const startMs = Math.max(cursorMs, segment.startMs);
    if (segment.endMs > startMs) {
      slices.push({ ...segment, startMs });
      cursorMs = segment.endMs;
    }
  });
  if (cursorMs < itemEndMs) {
    slices.push({ speakerLabel: "", startMs: cursorMs, endMs: itemEndMs });
  }
  return slices;
}

function splitTextAcrossTimedSlices(
  text: string,
  itemStartMs: number,
  itemEndMs: number,
  timedSlices: TimedSpeakerSlice[],
): Array<TimedSpeakerSlice & { text: string }> {
  const characters = Array.from(text);
  const durationMs = itemEndMs - itemStartMs;
  if (!characters.length || durationMs <= 0) return [];
  let previousIndex = 0;
  return timedSlices.map((slice, index) => {
    const isLast = index === timedSlices.length - 1;
    const nextIndex = isLast
      ? characters.length
      : clamp(
        Math.round(((slice.endMs - itemStartMs) / durationMs) * characters.length),
        previousIndex,
        characters.length,
      );
    const sliceText = characters.slice(previousIndex, nextIndex).join("");
    previousIndex = nextIndex;
    return { ...slice, text: sliceText };
  });
}

function toConversationItem(
  item: VoiceStreamItem,
  text: string,
  speakerLabel: string,
  startMs: number | undefined,
  endMs: number | undefined,
  index: number,
): SpeakerConversationItem {
  return {
    id: `${item.id}:${index}`,
    sourceItemId: item.id,
    phase: item.phase,
    text,
    speakerLabel,
    mode: item.mode,
    documentTitle: item.documentTitle,
    documentPath: item.documentPath,
    language: item.language,
    startMs,
    endMs,
    createdAt: item.createdAt,
    updatedAt: item.updatedAt,
  };
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

function dedupeVoiceStreamItemsForDisplay(items: VoiceStreamItem[]): VoiceStreamItem[] {
  const next: VoiceStreamItem[] = [];
  items.forEach((item) => {
    if (!next.some((existing) => voiceStreamItemsLookDuplicated(existing, item))) {
      next.push(item);
    }
  });
  return next;
}

function voiceStreamItemsLookDuplicated(left: VoiceStreamItem, right: VoiceStreamItem): boolean {
  if (left.id && right.id && left.id === right.id) return true;
  if (normalizedComparableText(left.text) !== normalizedComparableText(right.text)) return false;
  if (left.mode !== right.mode) return false;
  if (String(left.documentPath || "") !== String(right.documentPath || "")) return false;
  if (String(left.language || "") !== String(right.language || "")) return false;

  const leftStartMs = Number(left.startMs);
  const leftEndMs = Number(left.endMs);
  const rightStartMs = Number(right.startMs);
  const rightEndMs = Number(right.endMs);
  const leftTimed = Number.isFinite(leftStartMs) && Number.isFinite(leftEndMs) && leftEndMs > leftStartMs;
  const rightTimed = Number.isFinite(rightStartMs) && Number.isFinite(rightEndMs) && rightEndMs > rightStartMs;
  if (leftTimed && rightTimed) {
    return Math.abs(leftStartMs - rightStartMs) <= 250 && Math.abs(leftEndMs - rightEndMs) <= 250;
  }
  if (leftTimed || rightTimed) return false;
  return Math.abs(Number(left.updatedAt || 0) - Number(right.updatedAt || 0)) <= 1500;
}

function normalizedComparableText(value: string): string {
  return String(value || "").replace(/\s+/g, " ").trim();
}

function coalesceConversationItems(items: SpeakerConversationItem[]): SpeakerConversationItem[] {
  if (items.length <= 1) return items;
  const chronological = [...items].sort((left, right) => (
    Number(left.startMs ?? left.createdAt ?? 0) - Number(right.startMs ?? right.createdAt ?? 0)
    || Number(left.endMs ?? left.updatedAt ?? 0) - Number(right.endMs ?? right.updatedAt ?? 0)
  ));
  const merged: SpeakerConversationItem[] = [];
  chronological.forEach((item) => {
    const previous = merged[merged.length - 1];
    if (!previous || !conversationItemsCanMerge(previous, item)) {
      merged.push({ ...item });
      return;
    }
    previous.id = `${previous.id}+${item.id}`;
    previous.text = joinConversationText(previous.text, item.text);
    previous.endMs = item.endMs ?? previous.endMs;
    previous.updatedAt = Math.max(previous.updatedAt, item.updatedAt);
  });
  return merged.sort((left, right) => (
    Number(right.startMs ?? right.createdAt ?? 0) - Number(left.startMs ?? left.createdAt ?? 0)
    || Number(right.updatedAt ?? 0) - Number(left.updatedAt ?? 0)
  ));
}

function conversationItemsCanMerge(left: SpeakerConversationItem, right: SpeakerConversationItem): boolean {
  if (left.phase !== right.phase || left.mode !== right.mode) return false;
  if (left.speakerLabel !== right.speakerLabel) return false;
  if (String(left.documentPath || "") !== String(right.documentPath || "")) return false;
  if (String(left.language || "") !== String(right.language || "")) return false;
  const leftEndMs = Number(left.endMs);
  const rightStartMs = Number(right.startMs);
  const leftStartMs = Number(left.startMs);
  const rightEndMs = Number(right.endMs);
  if (
    Number.isFinite(leftEndMs)
    && Number.isFinite(rightStartMs)
    && Number.isFinite(leftStartMs)
    && Number.isFinite(rightEndMs)
  ) {
    const gapMs = rightStartMs - leftEndMs;
    const mergedDurationMs = rightEndMs - leftStartMs;
    return gapMs >= 0 && gapMs <= 1200 && mergedDurationMs <= 18_000;
  }
  return Math.abs(Number(right.createdAt || 0) - Number(left.updatedAt || 0)) <= 1500;
}

function joinConversationText(left: string, right: string): string {
  const prev = String(left || "").trim();
  const next = String(right || "").trim();
  if (!prev) return next;
  if (!next) return prev;
  const cjkBoundary = /[\u3040-\u30ff\u3400-\u9fff]$/.test(prev) && /^[\u3040-\u30ff\u3400-\u9fff]/.test(next);
  return cjkBoundary ? `${prev}${next}` : `${prev} ${next}`;
}
