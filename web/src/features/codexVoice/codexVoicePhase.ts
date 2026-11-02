import type { CodexVoicePhase } from "./codexVoiceSession";

// What a Voice status dot reports through data-status-dot.
export type VoiceDotState = "active" | "attention" | "failed" | "inactive";

/** A connected or external call alone does not establish activity on this page. */
export function voicePhaseHasLocalActivity(
  phase: CodexVoicePhase,
  options: { microphoneMuted: boolean; externalCall: boolean; playbackBlocked: boolean },
): boolean {
  if (options.externalCall) return false;
  if (phase === "listening") return !options.microphoneMuted;
  if (phase === "speaking") return !options.playbackBlocked;
  return ["responding", "analysing"].includes(phase);
}

export function voicePhaseDotState(phase: CodexVoicePhase, external: boolean): VoiceDotState {
  if (external || ["listening", "responding", "speaking", "analysing"].includes(phase)) {
    return "active";
  }
  if (phase === "preparing" || phase === "connecting" || phase === "stopping") {
    return "attention";
  }
  if (phase === "failed") return "failed";
  return "inactive";
}
