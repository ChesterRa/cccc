import { stripUncertainSpeakerPrefix } from "./voiceComposerUtils";
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

export function shouldSettleLiveVoiceActivityStream(
  currentPreview: VoiceTranscriptPreview | null,
  nextText: string,
  nextPhase: VoiceTranscriptPreview["phase"],
): boolean {
  if (!currentPreview) return false;
  const currentText = stripUncertainSpeakerPrefix(currentPreview.text);
  const text = stripUncertainSpeakerPrefix(nextText);
  if (!currentText || !text) return false;
  if (currentText === text || currentText.endsWith(text) || text.startsWith(currentText)) return false;
  if (currentPreview.phase === "final" && nextPhase === "interim") return true;
  if (text.length < Math.max(8, currentText.length * 0.55)) return true;
  return false;
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

export function newestVoiceActivityItemsFirst<T extends { sortAt: number }>(items: T[], limit: number): T[] {
  return [...items]
    .sort((left, right) => right.sortAt - left.sortAt)
    .slice(0, limit);
}
