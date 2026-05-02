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
  speakerLabel?: string;
  speakerIndex?: number;
  updatedAt: number;
};

export type VoiceTranscriptItem = VoiceTranscriptPreview & {
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
  preview: VoiceTranscriptPreview,
  maxItems = 120,
): VoiceTranscriptItem[] {
  if (preview.mode !== "document" || !String(preview.documentPath || "").trim()) return currentItems;
  const existing = currentItems.find((item) => item.id === preview.id);
  const nextItem: VoiceTranscriptItem = {
    ...preview,
    documentPath: String(preview.documentPath || "").trim(),
    createdAt: existing?.createdAt || preview.updatedAt,
  };
  return [
    nextItem,
    ...currentItems.filter((item) => item.id !== preview.id),
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
    if (!speaker || (item.speakerLabel === speaker.label && item.speakerIndex === speaker.index)) return item;
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
  let best: { label: string; index?: number; overlap: number } | null = null;
  for (const segment of speakerSegments) {
    const label = String(segment.speaker_label || "").trim();
    if (!label) continue;
    const segmentStart = Number(segment.start_ms);
    const segmentEnd = Number(segment.end_ms);
    if (!Number.isFinite(segmentStart) || !Number.isFinite(segmentEnd) || segmentEnd <= segmentStart) continue;
    const overlap = Math.max(0, Math.min(end, segmentEnd) - Math.max(start, segmentStart));
    if (!best || overlap > best.overlap) {
      const rawIndex = Number(segment.speaker_index);
      best = {
        label,
        index: Number.isFinite(rawIndex) ? rawIndex : undefined,
        overlap,
      };
    }
  }
  return best && best.overlap > 0 ? { label: best.label, index: best.index } : null;
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
