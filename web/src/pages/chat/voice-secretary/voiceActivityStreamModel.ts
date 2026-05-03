import { normalizeBrowserTranscriptChunk, stripUncertainSpeakerPrefix } from "./voiceComposerUtils";
import type { VoiceTranscriptPreview } from "./voiceStreamModel";

export type VoiceActivityStreamItem = {
  id: string;
  streamId: string;
  phase: VoiceTranscriptPreview["phase"];
  text: string;
  mode: VoiceTranscriptPreview["mode"];
  documentTitle?: string;
  documentPath?: string;
  updatedAt: number;
  createdAt: number;
};

function mergeActivityStreamText(previousText: string, nextText: string): string {
  const previous = normalizeBrowserTranscriptChunk(previousText);
  const next = normalizeBrowserTranscriptChunk(nextText);
  if (!next) return previous;
  if (!previous) return next;
  if (previous === next || previous.endsWith(next)) return previous;
  if (next.startsWith(previous) || next.length >= previous.length) return next;
  return previous;
}

function shouldUpdateLastStreamItem(lastItem: VoiceActivityStreamItem | undefined, streamId: string, text: string): boolean {
  if (!lastItem) return false;
  if (lastItem.streamId === streamId) return true;
  const previous = normalizeBrowserTranscriptChunk(lastItem.text);
  const next = normalizeBrowserTranscriptChunk(text);
  return Boolean(previous && next && (next.startsWith(previous) || previous.startsWith(next)));
}

export function shouldSettleLiveVoiceActivityStream(
  currentPreview: VoiceTranscriptPreview | null,
  nextText: string,
  nextPhase: VoiceTranscriptPreview["phase"],
): boolean {
  if (!currentPreview) return false;
  const currentText = stripUncertainSpeakerPrefix(currentPreview.text);
  const text = stripUncertainSpeakerPrefix(nextText);
  if (!currentText || !text) return false;
  if (currentPreview.phase === "final" && nextPhase === "interim") return true;
  if (currentText === text || currentText.endsWith(text) || text.startsWith(currentText)) return false;
  if (text.length < Math.max(8, currentText.length * 0.55)) return true;
  return false;
}

export function appendVoiceActivityStreamItem(
  currentItems: VoiceActivityStreamItem[],
  preview: VoiceTranscriptPreview | null,
  maxItems = 20,
): VoiceActivityStreamItem[] {
  if (!preview) return currentItems;
  const streamId = String(preview.id || "").trim();
  const text = stripUncertainSpeakerPrefix(preview.text);
  if (!streamId || !text) return currentItems;

  const updatedAt = Number.isFinite(preview.updatedAt) ? preview.updatedAt : Date.now();
  const lastItem = currentItems.length ? currentItems[currentItems.length - 1] : undefined;
  if (shouldUpdateLastStreamItem(lastItem, streamId, text)) {
    return currentItems.map((item) => {
      if (item.id !== lastItem?.id) return item;
      return {
        ...item,
        streamId,
        phase: preview.phase,
        text: mergeActivityStreamText(item.text, text),
        mode: preview.mode,
        documentTitle: preview.documentTitle,
        documentPath: preview.documentPath,
        updatedAt,
      };
    });
  }

  const item: VoiceActivityStreamItem = {
    id: `${streamId}-${updatedAt.toString(36)}`,
    streamId,
    phase: preview.phase,
    text,
    mode: preview.mode,
    documentTitle: preview.documentTitle,
    documentPath: preview.documentPath,
    updatedAt,
    createdAt: updatedAt,
  };

  return [...currentItems, item].slice(-maxItems);
}

export function voiceActivityStreamItemFromPreview(
  preview: VoiceTranscriptPreview | null,
): VoiceActivityStreamItem | null {
  if (!preview) return null;
  const streamId = String(preview.id || "").trim();
  const text = stripUncertainSpeakerPrefix(preview.text);
  if (!streamId || !text) return null;
  const updatedAt = Number.isFinite(preview.updatedAt) ? preview.updatedAt : Date.now();
  return {
    id: `${streamId}-live`,
    streamId,
    phase: preview.phase,
    text,
    mode: preview.mode,
    documentTitle: preview.documentTitle,
    documentPath: preview.documentPath,
    updatedAt,
    createdAt: updatedAt,
  };
}

export function filterVisibleVoiceActivityStreamItems(
  items: VoiceActivityStreamItem[],
  nowMs: number,
  visibleMs: number,
  recording: boolean,
): VoiceActivityStreamItem[] {
  if (recording) return items;
  return items.filter((item) => nowMs - item.updatedAt <= visibleMs);
}

export function removeVoiceActivityStreamItemsForSubmittedText(
  items: VoiceActivityStreamItem[],
  params: {
    mode: VoiceActivityStreamItem["mode"];
    text: string;
  },
): VoiceActivityStreamItem[] {
  const submitted = stripUncertainSpeakerPrefix(params.text);
  if (!submitted) return items;
  return items.filter((item) => {
    if (item.mode !== params.mode) return true;
    const heard = stripUncertainSpeakerPrefix(item.text);
    return !(heard === submitted || submitted.startsWith(heard) || heard.startsWith(submitted));
  });
}

export function newestVoiceActivityItemsFirst<T extends { sortAt: number }>(items: T[], limit: number): T[] {
  return [...items]
    .sort((left, right) => right.sortAt - left.sortAt)
    .slice(0, limit);
}
