import { useTranslation } from "react-i18next";

import { useSseErrorDetailText } from "../../hooks/useSseErrorDetailText";
import type { GroupStatus, GroupStatusKey } from "../../utils/groupStatus";
import { GROUP_STATUS_DOT_BASE_CLASS, GROUP_STATUS_LABEL_KEYS } from "../../utils/groupStatus";
import { classNames } from "../../utils/classNames";
import { useLcdStyle } from "../../utils/uiStyle";

/**
 * LCD preview: the same shapes as member states. Offline shares the stopped shape,
 * as it shares the dot. Paused is drawn in CSS as two bars, which no font
 * renders legibly at this size.
 */
const LCD_GROUP_SYMBOLS: Record<GroupStatusKey, string> = {
  run: "\u25CF",
  idle: "\u25CB",
  paused: "",
  offline: "\u25A1",
  stop: "\u25A1",
};

interface GroupStatusIndicatorProps {
  status: GroupStatus;
  variant?: "dot" | "badge";
  className?: string;
  subtle?: boolean;
  connectionStatus?: "connected" | "connecting" | "disconnected";
}

export function GroupStatusIndicator({
  status,
  variant = "dot",
  className,
  subtle = false,
  connectionStatus,
}: GroupStatusIndicatorProps) {
  const { t } = useTranslation("layout");
  const lcd = useLcdStyle();
  const label = t(GROUP_STATUS_LABEL_KEYS[status.key]);
  const interrupted = connectionStatus && connectionStatus !== "connected";
  const errorDetail = useSseErrorDetailText();
  const connectionLabel = interrupted
    ? t(connectionStatus === "connecting" ? "reconnecting" : "disconnected")
    : "";
  const description = interrupted
    ? `${label} · ${connectionLabel}${errorDetail ? ` · ${errorDetail}` : ""}. ${t("connectionInterruptedHint")}`
    : label;
  const dotClass = interrupted
    ? connectionStatus === "connecting"
      ? "bg-amber-400 animate-pulse motion-reduce:animate-none"
      : "bg-rose-500"
    : status.dotClass;

  const lcdSymbol = interrupted
    ? connectionStatus === "connecting"
      ? "\u2026"
      : "\u00D7"
    : LCD_GROUP_SYMBOLS[status.key];
  const lcdShape = !interrupted && status.key === "paused" ? "paused" : undefined;

  if (variant === "dot" && lcd) {
    return (
      <span
        className={classNames("lcd-group-symbol", className)}
        data-lcd-shape={lcdShape}
        role="img"
        aria-label={description}
        title={description}
      >
        {lcdSymbol}
      </span>
    );
  }

  if (variant === "dot") {
    return (
      <span
        className={classNames(
          subtle ? "size-[6px] shrink-0 rounded-full opacity-75" : GROUP_STATUS_DOT_BASE_CLASS,
          dotClass,
          className,
        )}
        role="img"
        aria-label={description}
        title={description}
      />
    );
  }

  return (
    <span
      className={classNames(
        "inline-flex min-h-6 shrink-0 items-center gap-1.5 rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-2 text-xs font-medium text-[var(--color-text-secondary)]",
        className,
      )}
      title={description}
      aria-label={description}
      role={connectionStatus ? "status" : undefined}
      data-connection-state={connectionStatus}
    >
      {lcd ? (
        <span className="lcd-group-symbol" data-lcd-shape={lcdShape} aria-hidden="true">
          {lcdSymbol}
        </span>
      ) : (
        <span className={classNames(GROUP_STATUS_DOT_BASE_CLASS, dotClass)} aria-hidden="true" />
      )}
      {/* While the page is cut off the run state may be stale, so the visible text
          names what the dot shows; the run state stays in the description. */}
      <span>{interrupted ? connectionLabel : label}</span>
    </span>
  );
}
