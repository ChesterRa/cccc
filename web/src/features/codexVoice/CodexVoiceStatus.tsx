import { classNames } from "../../utils/classNames";
import type { VoiceDotState } from "./codexVoicePhase";

export function VoiceStatusDot({ active, attention }: { active: boolean; attention: boolean }) {
  const state: VoiceDotState = attention ? "attention" : active ? "active" : "inactive";
  return (
    <span
      className={classNames(
        "h-2.5 w-2.5 flex-none rounded-full",
        state === "attention"
          ? "bg-amber-400 shadow-[0_0_0_4px_rgba(251,191,36,0.12)]"
          : state === "active"
            ? "bg-emerald-400 shadow-[0_0_0_4px_rgba(52,211,153,0.12)]"
            : "bg-[var(--color-text-tertiary)]",
      )}
      data-status-dot={state}
      aria-hidden="true"
    />
  );
}
