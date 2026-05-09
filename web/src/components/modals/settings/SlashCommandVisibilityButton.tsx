import { Eye, EyeOff } from "lucide-react";

import { secondaryButtonClass } from "./types";

export function SlashCommandVisibilityButton(props: {
  hidden: boolean;
  busy: boolean;
  visibleLabel: string;
  hiddenLabel: string;
  showActionLabel: string;
  hideActionLabel: string;
  onToggle: (nextVisible: boolean) => void;
}) {
  const { hidden, busy, visibleLabel, hiddenLabel, showActionLabel, hideActionLabel, onToggle } = props;
  const Icon = hidden ? EyeOff : Eye;
  const label = hidden ? hiddenLabel : visibleLabel;
  const actionLabel = hidden ? showActionLabel : hideActionLabel;
  const hiddenClass = "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300";
  const visibleClass = secondaryButtonClass("sm");

  return (
    <button
      type="button"
      className={`inline-flex min-h-[32px] items-center gap-1.5 rounded border px-2.5 py-1.5 text-xs ${hidden ? hiddenClass : visibleClass} ${busy ? "cursor-not-allowed opacity-60" : ""}`}
      disabled={busy}
      title={actionLabel}
      aria-label={actionLabel}
      onClick={() => onToggle(hidden)}
    >
      <Icon size={14} aria-hidden="true" />
      <span>{label}</span>
    </button>
  );
}
