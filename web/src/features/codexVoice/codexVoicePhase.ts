import type { CodexVoicePhase } from "./codexVoiceSession";

// What a Voice status dot reports. Classic paints it with the classes below; the LCD
// look reads the same state from data-status-dot.
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

const PHASE_DOT_CLASS: Record<VoiceDotState, string> = {
  active: "bg-emerald-400 shadow-[0_0_0_4px_rgba(52,211,153,0.12)]",
  attention: "animate-pulse bg-amber-400",
  failed: "bg-rose-500",
  inactive: "bg-[var(--color-text-tertiary)]",
};

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

export function voicePhaseDotClass(phase: CodexVoicePhase, external: boolean): string {
  return PHASE_DOT_CLASS[voicePhaseDotState(phase, external)];
}
