import { Check, Layers2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { shareableSessionNodeOrigin } from "./sessionNodeJoin";

export function SessionNodeHubLabel({
  instanceName,
  advertisedUrl,
}: {
  instanceName?: string;
  advertisedUrl?: string | null;
}) {
  const { t } = useTranslation("layout");
  const origin = shareableSessionNodeOrigin(advertisedUrl);
  const identity = instanceName?.trim() || (origin ? new URL(origin).hostname : "");
  return (
    <p className="mb-1 flex min-w-0 items-center gap-1.5 text-xs font-normal text-[var(--color-text-secondary)]">
      <Layers2 size={14} aria-hidden="true" />
      {t("sessionNodes.sessionsHub")}
      {identity && <bdi className="truncate">{identity}</bdi>}
    </p>
  );
}

export function SessionNodeProgress({
  step,
  ticket = false,
}: {
  step: 1 | 2 | 3 | 4;
  ticket?: boolean;
}) {
  const { t } = useTranslation("layout");
  const labels = [
    t("sessionNodes.joinRunStage"),
    t(ticket ? "sessionNodes.ticketStage" : "sessionNodes.joinReviewStage"),
    t("sessionNodes.joinConnectStage"),
  ];
  return (
    <ol
      aria-label={t("sessionNodes.joinProgress")}
      className="mb-4 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1.5 border-b border-[var(--glass-border-subtle)] pb-4 text-xs"
    >
      {labels.map((label, index) => {
        const position = index + 1;
        const current = position === step;
        return (
          <li
            key={label}
            aria-current={current ? "step" : undefined}
            className="flex min-w-0 items-center gap-1.5"
          >
            {index > 0 && (
              <span
                aria-hidden="true"
                className="w-3 border-t border-[var(--color-border-primary)] sm:w-6"
              />
            )}
            <span
              aria-hidden="true"
              className={`grid h-5 w-5 shrink-0 place-items-center rounded-full border font-mono text-[10px] ${current ? "border-[var(--primary)] bg-[var(--primary)] text-[var(--primary-foreground)]" : "border-[var(--color-border-primary)] text-[var(--color-text-secondary)]"}`}
            >
              {position < step ? <Check size={12} /> : position}
            </span>
            <span className={current ? "font-medium" : "text-[var(--color-text-secondary)]"}>
              {label}
            </span>
          </li>
        );
      })}
    </ol>
  );
}
