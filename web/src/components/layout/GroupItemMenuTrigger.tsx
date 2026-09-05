import { classNames } from "../../utils/classNames";
import { MoreIcon } from "../Icons";
import { IconButton } from "../ui/icon-button";

interface GroupItemMenuTriggerProps {
  isActive: boolean;
  label: string;
  open: boolean;
  onToggle: (button: HTMLButtonElement) => void;
}

export function GroupItemMenuTrigger({
  isActive,
  label,
  open,
  onToggle,
}: GroupItemMenuTriggerProps) {
  return (
    <IconButton
      type="button"
      variant="ghost"
      size="sm"
      label={label}
      className={classNames(
        // Touch devices have no hover state to reveal the trigger from, so it
        // stays visible whenever the primary pointer is coarse.
        "pointer-events-none shrink-0 text-[var(--color-text-tertiary)] opacity-0 pointer-coarse:pointer-events-auto pointer-coarse:opacity-100 group-hover/item:pointer-events-auto group-hover/item:opacity-100 focus-visible:pointer-events-auto focus-visible:opacity-100",
        open &&
          "pointer-events-auto opacity-100 bg-[var(--glass-tab-bg)] border-[var(--glass-border-subtle)] text-[var(--color-text-primary)] shadow-sm",
        !open && isActive && "pointer-events-auto opacity-100 text-[rgb(35,36,37)] dark:text-white",
        !open &&
          "hover:bg-[var(--glass-tab-bg-hover)] hover:border-[var(--glass-border-subtle)] hover:text-[var(--color-text-primary)]",
      )}
      aria-haspopup="menu"
      aria-expanded={open}
      onMouseDown={(event) => event.stopPropagation()}
      onPointerDown={(event) => event.stopPropagation()}
      onTouchStart={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.stopPropagation();
        onToggle(event.currentTarget);
      }}
    >
      <MoreIcon size={16} />
    </IconButton>
  );
}
