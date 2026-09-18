import type { ReactNode } from "react";
import { classNames } from "../../utils/classNames";
import { Button } from "../ui/button";

interface GroupMenuActionProps {
  label: string;
  icon?: ReactNode;
  /** Reserve the icon column so labels line up when siblings carry icons. */
  iconSlot?: boolean;
  disabled?: boolean;
  tone?: "default" | "danger";
  onClick: () => void;
}

export function GroupMenuAction({
  label,
  icon,
  iconSlot,
  disabled,
  tone = "default",
  onClick,
}: GroupMenuActionProps) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      role="menuitem"
      disabled={disabled}
      className={classNames(
        "w-full justify-start gap-2.5 text-left text-sm",
        tone === "danger"
          ? "text-rose-600 hover:text-rose-700 dark:text-rose-400 dark:hover:text-rose-300"
          : "text-[var(--color-text-primary)]",
      )}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
    >
      {icon || iconSlot ? (
        <span
          className={classNames(
            "inline-flex size-4 shrink-0 items-center justify-center",
            tone === "danger" ? "text-current" : "text-[var(--color-text-secondary)]",
          )}
          aria-hidden="true"
        >
          {icon}
        </span>
      ) : null}
      <span className="truncate">{label}</span>
    </Button>
  );
}
