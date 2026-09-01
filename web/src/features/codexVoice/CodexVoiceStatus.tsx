import { classNames } from "../../utils/classNames";

export function VoiceStatusDot({ active, attention }: { active: boolean; attention: boolean }) {
  return (
    <span
      className={classNames(
        "h-2.5 w-2.5 flex-none rounded-full",
        attention
          ? "bg-amber-400 shadow-[0_0_0_4px_rgba(251,191,36,0.12)]"
          : active
            ? "bg-emerald-400 shadow-[0_0_0_4px_rgba(52,211,153,0.12)]"
            : "bg-[var(--color-text-tertiary)]",
      )}
      aria-hidden="true"
    />
  );
}
