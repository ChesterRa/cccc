import { ConnectLinkIcon } from "../../components/Icons";
import { useTranslation } from "react-i18next";
import { groupConnectionCountText, type GroupConnectionCount } from "./protocol";

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
  const text = groupConnectionCountText(connection);
  if (!text || !onClick) return null;
  const label = t(text.current ? "groupConnections.count" : "groupConnections.countUnknown", {
    count: connection?.count,
  });
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      className={`flex min-h-8 shrink-0 items-center gap-1 rounded-md text-xs text-[var(--color-text-secondary)] hover:bg-[var(--glass-panel-bg)] focus-visible:outline-2 disabled:opacity-50 px-1.5`}
      onPointerDown={(event) => event.stopPropagation()}
      onMouseDown={(event) => event.stopPropagation()}
      onTouchStart={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.stopPropagation();
        onClick();
      }}
    >
      <ConnectLinkIcon size={13} />
      <span>{text.value}</span>
    </button>
  );
}
