import type { CodexVoicePhase } from "./codexVoiceSession";

export function voicePhaseDotClass(phase: CodexVoicePhase, external: boolean): string {
  if (external || ["listening", "responding", "speaking", "analysing"].includes(phase)) {
    return "bg-emerald-400 shadow-[0_0_0_4px_rgba(52,211,153,0.12)]";
  }
  if (phase === "preparing" || phase === "connecting" || phase === "stopping") {
    return "animate-pulse bg-amber-400";
  }
  if (phase === "failed") return "bg-rose-500";
  return "bg-[var(--color-text-tertiary)]";
}
