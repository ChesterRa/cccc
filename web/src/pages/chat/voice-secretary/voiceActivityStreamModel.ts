import { stripUncertainSpeakerPrefix } from "./voiceComposerUtils";
import type { VoiceTranscriptPreview } from "./voiceStreamModel";

export function shouldSettleLiveVoiceActivityStream(
  currentPreview: VoiceTranscriptPreview | null,
  nextText: string,
  nextPhase: VoiceTranscriptPreview["phase"],
): boolean {
  if (!currentPreview) return false;
  const currentText = stripUncertainSpeakerPrefix(currentPreview.text);
  const text = stripUncertainSpeakerPrefix(nextText);
  if (!currentText || !text) return false;
  if (currentText === text || currentText.endsWith(text) || text.startsWith(currentText))
    return false;
  if (currentPreview.phase === "final" && nextPhase === "interim") return true;
  if (text.length < Math.max(8, currentText.length * 0.55)) return true;
  return false;
}
