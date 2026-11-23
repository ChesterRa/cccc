import { Layers2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { LcdIcon } from "../../components/Icons";

export function SessionNodeHubLabel({ instanceName }: { instanceName?: string }) {
  const { t } = useTranslation("layout");
  const identity = instanceName?.trim();
  return (
    <p className="mb-1 flex min-w-0 items-center gap-1.5 text-xs font-normal text-[var(--color-text-secondary)]">
      <LcdIcon icon={Layers2} glyph="server" size={14} aria-hidden="true" />
      {t("sessionNodes.sessionsHub")}
      {identity && <bdi className="truncate">{identity}</bdi>}
    </p>
  );
}

export function SessionNodeProgress({ ticketIssued }: { ticketIssued: boolean }) {
  const { t } = useTranslation("layout");
  const step = ticketIssued ? 2 : 1;
  const labels = [
    t("sessionNodes.runStage"),
    t("sessionNodes.ticketStage"),
    t("sessionNodes.connectStage"),
  ];
  return (
    <ol
      aria-label={t("sessionNodes.progress")}
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
                className="hidden w-4 border-t border-[var(--color-border-primary)] sm:inline-block sm:w-6"
              />
            )}
            <span
              aria-hidden="true"
              className={`grid h-5 w-5 shrink-0 place-items-center rounded-full border font-mono text-[10px] ${current ? "border-[var(--primary)] bg-[var(--primary)] text-[var(--primary-foreground)]" : "border-[var(--color-border-primary)] text-[var(--color-text-secondary)]"}`}
            >
              {position}
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
