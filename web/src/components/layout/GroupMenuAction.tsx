import type { ReactNode } from "react";
import { Button } from "../ui/button";

export interface GroupMenuActionProps {
  label: string;
  icon?: ReactNode;
  description?: string;
  disabled?: boolean;
  destructive?: boolean;
  onClick: () => void;
}

export function GroupMenuAction({
  label,
  icon,
  description,
  disabled,
  destructive,
  onClick,
}: GroupMenuActionProps) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      role="menuitem"
      disabled={disabled}
      className={`w-full justify-start text-left text-sm ${description ? "h-auto min-h-10 items-start whitespace-normal py-2" : ""} ${destructive ? "text-rose-600 dark:text-rose-400" : "text-[var(--color-text-primary)]"}`}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
    >
      {icon && (
        <span className="mt-0.5 shrink-0" aria-hidden="true">
          {icon}
        </span>
      )}
      <span className="min-w-0">
        <span className="block">{label}</span>
        {description && (
          <span className="mt-0.5 block text-xs font-normal text-[var(--color-text-secondary)]">
            {description}
          </span>
        )}
      </span>
    </Button>
  );
}
