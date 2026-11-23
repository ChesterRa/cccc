import { useState } from "react";
import { ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";
import { LcdIcon } from "../../components/Icons";
import { Button } from "../../components/ui/button";
import { SelectMenu } from "../../components/ui/select-menu";
import { Textarea } from "../../components/ui/textarea";
import type { LocalSessionSource } from "../../types";
import { classNames } from "../../utils/classNames";
import { SessionCommand } from "./SessionCommand";
import { SessionNodeProgress } from "./SessionNodeProgress";
import type { useSessionNodeTicket } from "./useSessionNodeTicket";

const RUNTIME_LABELS: Record<LocalSessionSource["runtime"], string> = {
  codex: "Codex",
  claude: "Claude Code",
  pi: "Pi",
  opencode: "OpenCode",
};

export function SessionNodePreparation({
  invitation,
  supportedRuntimes,
  ticketError = "",
  busy,
  directoryHintKey,
}: {
  invitation: ReturnType<typeof useSessionNodeTicket>;
  supportedRuntimes: LocalSessionSource["runtime"][];
  ticketError?: string;
  busy: boolean;
  /** Where the paired computer's sessions appear next: the picker list or a group's Add Agent. */
  directoryHintKey: string;
}) {
  const { t, i18n } = useTranslation("layout");
  const runtimes = supportedRuntimes;
  const [chosenRuntime, setRuntime] = useState<LocalSessionSource["runtime"]>(
    runtimes[0] || "codex",
  );
  const runtime: LocalSessionSource["runtime"] | undefined = runtimes.includes(chosenRuntime)
    ? chosenRuntime
    : runtimes[0];
  const [revealedTicket, setRevealedTicket] = useState<string | null>(null);
  const { ticket, ticketExpired, creating, copying, copied, copy, create } = invitation;
  const hasCurrentTicket = !!ticket && !ticketExpired;
  const ticketVisible = hasCurrentTicket && revealedTicket === ticket.ticket;

  return (
    <section aria-label={t("sessionNodes.prepareTitle")} aria-busy={busy} className="min-w-0">
      <SessionNodeProgress ticketIssued={hasCurrentTicket} />
      <div className="space-y-3">
        <div>
          <h3 className="text-base font-semibold">{t("sessionNodes.runRemote")}</h3>
          <p className="mt-1 text-sm leading-relaxed text-[var(--color-text-secondary)]">
            {t("sessionNodes.prepareHint")}
          </p>
          <p className="mt-1 text-xs leading-relaxed text-[var(--color-text-secondary)]">
            {t("sessionNodes.scopeHint")}
          </p>
        </div>
        {runtime && (
          <>
            <div className="flex min-w-0 flex-wrap items-center gap-2">
              <span className="text-sm text-[var(--color-text-secondary)]">
                {t("sessionNodes.enableApp")}
              </span>
              <SelectMenu
                ariaLabel={t("sessionNodes.enableApp")}
                value={runtime}
                onChange={(value) => setRuntime(value as LocalSessionSource["runtime"])}
                disabled={busy}
                options={runtimes.map((value) => ({
                  value,
                  label: RUNTIME_LABELS[value] || value,
                }))}
                className="min-h-10 max-sm:min-h-11 max-w-full text-sm"
              />
            </div>
            <p className="text-xs leading-relaxed text-[var(--color-text-secondary)]">
              {t("sessionNodes.runtimeHint")}
            </p>
            <SessionCommand
              key={runtime}
              emphasis="secondary"
              command={`cccc sessions node connect --runtime ${runtime} --ticket-stdin`}
              copyLabel={t("sessionNodes.copyConnectCommand")}
              disabled={busy}
            />
          </>
        )}
        <div className="space-y-2 border-t border-[var(--glass-border-subtle)] pt-3">
          <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
            {t("sessionNodes.ticketGenerateHint")}
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              className="min-h-10 max-sm:min-h-11 whitespace-normal text-sm"
              disabled={busy || !runtime}
              title={hasCurrentTicket && copied && !busy ? t("sessionNodes.copyTicket") : undefined}
              onClick={() => void copy()}
            >
              {t(
                creating
                  ? "sessionNodes.preparing"
                  : copying
                    ? "sessionNodes.copyingTicket"
                    : hasCurrentTicket
                      ? copied
                        ? "common:copied"
                        : "sessionNodes.copyTicket"
                      : "sessionNodes.create",
              )}
            </Button>
            {hasCurrentTicket && (
              <>
                <Button
                  type="button"
                  variant="outline"
                  className="min-h-10 max-sm:min-h-11 whitespace-normal text-sm"
                  disabled={busy}
                  onClick={() => {
                    setRevealedTicket(null);
                    void create();
                  }}
                >
                  {t("sessionNodes.createAnother")}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  aria-expanded={ticketVisible}
                  className="min-h-10 max-sm:min-h-11 gap-1.5 whitespace-normal text-sm"
                  disabled={busy}
                  onClick={() => setRevealedTicket(ticketVisible ? null : ticket.ticket)}
                >
                  {t(ticketVisible ? "sessionNodes.hideTicket" : "sessionNodes.showTicket")}
                  <LcdIcon
                    icon={ChevronDown}
                    glyph="chevron-down"
                    aria-hidden="true"
                    className={classNames(
                      "h-4 w-4 transition-transform motion-reduce:transition-none",
                      ticketVisible && "rotate-180",
                    )}
                  />
                </Button>
              </>
            )}
          </div>
          {ticketError && (
            <p role="alert" className="text-sm text-[var(--color-accent-danger)]">
              {ticketError}
            </p>
          )}
          {ticketVisible && hasCurrentTicket && ticket && (
            <Textarea
              aria-label={t("sessionNodes.ticketLabel")}
              rows={3}
              readOnly
              value={ticket.ticket}
              className="w-full break-all font-mono text-xs"
              onFocus={(event) => event.currentTarget.select()}
            />
          )}
          {copied && hasCurrentTicket && (
            <p role="status" className="text-xs text-[var(--color-text-secondary)]">
              {t("sessionNodes.ticketCopiedHint")}
            </p>
          )}
          {ticket && (
            <p role="status" className="text-xs text-[var(--color-text-secondary)]">
              {ticketExpired
                ? t("sessionNodes.ticketExpired")
                : t("sessionNodes.expires", {
                    time: new Date(ticket.expires_at).toLocaleTimeString(i18n.language),
                  })}
            </p>
          )}
          {ticket && (
            <p className="text-xs text-[var(--color-text-secondary)]">{t(directoryHintKey)}</p>
          )}
        </div>
        {runtime === "codex" && (
          <details className="border-t border-[var(--glass-border-subtle)] pt-1">
            <summary className="cursor-pointer py-2 text-sm text-[var(--color-text-secondary)]">
              {t("sessionNodes.codexRequirements")}
            </summary>
            <p className="pb-2 text-sm leading-relaxed text-[var(--color-text-secondary)]">
              {t("sessionNodes.codexRequirementHint")}
            </p>
          </details>
        )}
      </div>
    </section>
  );
}
