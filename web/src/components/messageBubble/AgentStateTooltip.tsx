import type { CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { classNames } from "../../utils/classNames";
import { formatFullTime, formatTime } from "../../utils/time";

export function AgentStateTooltip({
  isOpen,
  canShow,
  isPositioned,
  setFloating,
  floatingStyles,
  senderDisplayName,
  updatedAt,
  agentStateDisplay,
  stateTask,
  blockerCount,
  stateNext,
  stateChanged,
}: {
  isOpen: boolean;
  canShow: boolean;
  isPositioned: boolean;
  setFloating: (node: HTMLElement | null) => void;
  floatingStyles: CSSProperties;
  senderDisplayName: string;
  updatedAt?: string;
  agentStateDisplay: string;
  stateTask: string;
  blockerCount: number;
  stateNext: string;
  stateChanged: string;
}) {
  const { t } = useTranslation("chat");
  if (!isOpen || !canShow) return null;

  return (
    <div
      ref={setFloating}
      style={floatingStyles}
      className={classNames(
        "pointer-events-none z-[80] w-[min(360px,calc(100vw-32px))] rounded-2xl px-3 py-2 shadow-2xl transition-opacity duration-150",
        "glass-modal text-[var(--color-text-primary)]",
        isPositioned ? "opacity-100" : "opacity-0",
      )}
      role="status"
    >
      <div className="flex items-center gap-2">
        <div className="text-xs font-semibold text-[var(--color-text-primary)]">
          {senderDisplayName}
        </div>
        {updatedAt ? (
          <div
            className="ml-auto text-xs tabular-nums text-[var(--color-text-tertiary)]"
            title={formatFullTime(updatedAt)}
          >
            {t("updated", { time: formatTime(updatedAt) })}
          </div>
        ) : null}
      </div>
      <div className="mt-1 whitespace-pre-wrap text-xs text-[var(--color-text-secondary)]">
        {agentStateDisplay}
      </div>
      {stateTask || blockerCount > 0 || stateNext || stateChanged ? (
        <div className="mt-2 space-y-1">
          <div className="flex flex-wrap items-center gap-1.5">
            {stateTask ? (
              <span className="rounded bg-[var(--glass-tab-bg)] px-2 py-0.5 text-[11px] text-[var(--color-text-secondary)]">
                {t("taskShort", { id: stateTask })}
              </span>
            ) : null}
            {blockerCount > 0 ? (
              <span className="rounded bg-rose-500/15 px-2 py-0.5 text-[11px] text-rose-600 dark:text-rose-300">
                {t("blockersShort", { count: blockerCount })}
              </span>
            ) : null}
          </div>
          {stateNext ? (
            <div className="text-[11px] text-[var(--color-text-tertiary)]">
              {t("nextShort", { value: stateNext })}
            </div>
          ) : null}
          {stateChanged ? (
            <div className="text-[11px] text-[var(--color-text-tertiary)]">
              {t("changedShort", { value: stateChanged })}
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
