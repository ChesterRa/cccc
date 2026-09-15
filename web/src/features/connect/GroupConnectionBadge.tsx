import { Link2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { GroupConnectionCount } from "./protocol";

export function GroupConnectionBadge({
  connection,
  onClick,
  disabled = false,
}: {
  connection?: GroupConnectionCount;
  onClick?: () => void;
  disabled?: boolean;
}) {
  const { t } = useTranslation("layout");
  if (!connection || connection.count === 0 || !onClick) return null;
  const current = connection.count !== null && Date.parse(connection.expires_at) > Date.now();
  const label = t(current ? "groupConnections.count" : "groupConnections.countUnknown", {
    count: connection.count,
  });
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      className="flex min-h-8 shrink-0 items-center gap-1 rounded-md px-1.5 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--glass-panel-bg)] focus-visible:outline-2 disabled:opacity-50"
      onPointerDown={(event) => event.stopPropagation()}
      onMouseDown={(event) => event.stopPropagation()}
      onTouchStart={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
    >
      <Link2 size={13} />
      <span>{current ? connection.count : "?"}</span>
    </button>
  );
}
