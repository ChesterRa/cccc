import { Button } from "../ui/button";

interface GroupMenuActionProps {
  label: string;
  onClick: () => void;
}

export function GroupMenuAction({ label, onClick }: GroupMenuActionProps) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      role="menuitem"
      className="w-full justify-start text-left text-sm text-[var(--color-text-primary)]"
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
    >
      {label}
    </Button>
  );
}
