export type VoiceStreamCaptureMode = "document" | "instruction" | "prompt";

export type VoiceTranscriptPreviewPhase = "interim" | "final";
export type VoiceTranscriptProcessingPhase = "separating_speakers" | "failed";

export type VoiceTranscriptPreview = {
  id: string;
  sessionId?: string;
  phase: VoiceTranscriptPreviewPhase;
  text: string;
  pendingFinalText?: string;
  interimText?: string;
  mode: VoiceStreamCaptureMode;
  documentTitle?: string;
  documentPath?: string;
  language?: string;
  source?: string;
  sourceLabel?: string;
  sourceDetail?: string;
  startMs?: number;
  endMs?: number;
  speakerLabel?: string;
  speakerIndex?: number;
  processingPhase?: VoiceTranscriptProcessingPhase;
  updatedAt: number;
};

export type VoiceTranscriptItem = VoiceTranscriptPreview & {
  createdAt: number;
};

export type VoiceStreamMetadata = {
  mode: VoiceStreamCaptureMode;
  sessionId?: string;
  documentTitle?: string;
  documentPath?: string;
  language?: string;
  source?: string;
  sourceLabel?: string;
  sourceDetail?: string;
};

export type VoiceStreamTiming = {
  startMs?: number;
  endMs?: number;
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
    sessionId: params.metadata.sessionId,
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

export function createVoiceTranscriptItem(params: {
  id: string;
  cleanText: string;
  metadata: VoiceStreamMetadata;
  timing?: VoiceStreamTiming;
  now: number;
}): VoiceTranscriptItem | null {
  const cleanText = String(params.cleanText || "").trim();
  const documentPath = String(params.metadata.documentPath || "").trim();
  if (!cleanText || params.metadata.mode !== "document" || !documentPath) return null;
  return {
    id: params.id,
    sessionId: params.metadata.sessionId,
    phase: "final",
    text: cleanText,
    ...params.metadata,
    documentPath,
    startMs: params.timing?.startMs,
    endMs: params.timing?.endMs,
    createdAt: params.now,
    updatedAt: params.now,
  };
}

export function upsertLiveVoiceTranscriptItem(
  currentItems: VoiceTranscriptItem[],
  preview: VoiceTranscriptPreview | null,
  maxItems = 240,
): VoiceTranscriptItem[] {
  if (!preview || preview.mode !== "document" || !String(preview.documentPath || "").trim()) {
    return currentItems;
  }
  const text = String(preview.text || "").trim();
  if (!text) return currentItems.filter((item) => item.id !== preview.id);
  const item: VoiceTranscriptItem = {
    ...preview,
    phase: preview.phase,
    text,
    documentPath: String(preview.documentPath || "").trim(),
    createdAt: Number(currentItems.find((existing) => existing.id === preview.id)?.createdAt || preview.updatedAt),
    updatedAt: preview.updatedAt,
  };
  return [
    item,
    ...currentItems.filter((existing) => existing.id !== preview.id),
  ].slice(0, maxItems);
}

export function appendFinalVoiceTranscriptItem(
  currentItems: VoiceTranscriptItem[],
  item: VoiceTranscriptItem | null,
  liveItemId = "",
  maxItems = 240,
): VoiceTranscriptItem[] {
  if (!item) return currentItems;
  return [
    item,
    ...currentItems.filter((existing) => (
      existing.id !== liveItemId
      && existing.id !== item.id
      && !voiceTranscriptItemsLookDuplicated(existing, item)
    )),
  ].slice(0, maxItems);
}

export function mergeVoiceTranscriptItems(
  currentItems: VoiceTranscriptItem[],
  incomingItems: VoiceTranscriptItem[],
  maxItems = 240,
): VoiceTranscriptItem[] {
  return incomingItems.reduce(
    (items, item) => appendFinalVoiceTranscriptItem(items, item, "", maxItems),
    currentItems,
  ).slice(0, maxItems);
}

export function replaceVoiceTranscriptSessionItems(
  currentItems: VoiceTranscriptItem[],
  incomingItems: VoiceTranscriptItem[],
  maxItems = 240,
): VoiceTranscriptItem[] {
  const sessionIds = new Set(incomingItems.map((item) => String(item.sessionId || "").trim()).filter(Boolean));
  const filtered = currentItems.filter((item) => {
    const sessionId = String(item.sessionId || "").trim();
    if (sessionId && sessionIds.has(sessionId)) return false;
    return true;
  });
  return mergeVoiceTranscriptItems(filtered, incomingItems, maxItems);
}

export function replaceVoiceTranscriptProcessingItem(
  currentItems: VoiceTranscriptItem[],
  incomingItem: VoiceTranscriptItem,
  maxItems = 240,
): VoiceTranscriptItem[] {
  const sessionId = String(incomingItem.sessionId || "").trim();
  const nextItems = currentItems.filter((item) => {
    if (item.id === incomingItem.id) return false;
    if (sessionId && String(item.sessionId || "").trim() === sessionId && item.processingPhase) return false;
    return true;
  });
  return [incomingItem, ...nextItems].slice(0, maxItems);
}

export function isDisplayableFinalVoiceTranscriptItem(item: VoiceTranscriptItem): boolean {
  return (
    item.phase === "final"
    && !item.processingPhase
    && !String(item.id || "").startsWith("voice-stream-")
  );
}

export function filterVoiceTranscriptItemsForDocument(
  items: VoiceTranscriptItem[],
  documentPath: string,
): VoiceTranscriptItem[] {
  const targetPath = String(documentPath || "").trim();
  if (!targetPath) return [];
  return items
    .filter((item) => (
      item.mode === "document"
      && String(item.documentPath || "").trim() === targetPath
      && String(item.text || "").trim()
    ))
    .sort((left, right) => (
      Number(right.updatedAt || 0) - Number(left.updatedAt || 0)
      || Number(right.createdAt || 0) - Number(left.createdAt || 0)
    ));
}

export function annotateVoiceTranscriptItemsWithSpeakers(
  items: VoiceTranscriptItem[],
  speakerSegments: Record<string, unknown>[],
): VoiceTranscriptItem[] {
  if (!items.length || !speakerSegments.length) return items;
  let changed = false;
  const next = items.map((item) => {
    const speaker = speakerForTranscriptRange(item.startMs, item.endMs, speakerSegments);
    if (!speaker) {
      if (!item.speakerLabel && item.speakerIndex === undefined) return item;
      changed = true;
      return {
        ...item,
        speakerLabel: undefined,
        speakerIndex: undefined,
      };
    }
    if (item.speakerLabel === speaker.label && item.speakerIndex === speaker.index) return item;
    changed = true;
    return {
      ...item,
      speakerLabel: speaker.label,
      speakerIndex: speaker.index,
    };
  });
  return changed ? next : items;
}

function speakerForTranscriptRange(
  startMs: number | undefined,
  endMs: number | undefined,
  speakerSegments: Record<string, unknown>[],
): { label: string; index?: number } | null {
  const start = Number(startMs);
  const end = Number(endMs);
  if (!Number.isFinite(start) || !Number.isFinite(end) || end <= start) return null;
  const overlaps = new Map<string, { label: string; index?: number; overlap: number }>();
  for (const segment of speakerSegments) {
    const label = String(segment.speaker_label || "").trim();
    if (!label) continue;
    const segmentStart = Number(segment.start_ms);
    const segmentEnd = Number(segment.end_ms);
    if (!Number.isFinite(segmentStart) || !Number.isFinite(segmentEnd) || segmentEnd <= segmentStart) continue;
    const overlap = Math.max(0, Math.min(end, segmentEnd) - Math.max(start, segmentStart));
    if (overlap <= 0) continue;
    const rawIndex = Number(segment.speaker_index);
    const index = Number.isFinite(rawIndex) ? rawIndex : undefined;
    const key = `${label}\t${index ?? ""}`;
    const existing = overlaps.get(key);
    overlaps.set(key, {
      label,
      index,
      overlap: (existing?.overlap || 0) + overlap,
    });
  }
  const ranked = Array.from(overlaps.values()).sort((left, right) => right.overlap - left.overlap);
  const best = ranked[0];
  if (!best || best.overlap <= 0) return null;
  const totalOverlap = ranked.reduce((sum, item) => sum + item.overlap, 0);
  if (ranked.length > 1 && totalOverlap > 0 && best.overlap / totalOverlap < 0.72) return null;
  return { label: best.label, index: best.index };
}

function voiceTranscriptItemsLookDuplicated(left: VoiceTranscriptItem, right: VoiceTranscriptItem): boolean {
  if (left.id && right.id && left.id === right.id) return true;
  if (normalizedComparableTranscriptText(left.text) !== normalizedComparableTranscriptText(right.text)) return false;
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

function normalizedComparableTranscriptText(value: string): string {
  return String(value || "").replace(/\s+/g, " ").trim();
}
