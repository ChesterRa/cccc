import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { SelectMenu } from "../../components/ui/select-menu";
import { SessionCommand } from "./SessionCommand";
import { SessionNodeProgress } from "./SessionNodeProgress";
import type { useSessionNodeTicket } from "./useSessionNodeTicket";

export function SessionNodePreparation({
  invitation,
  busy,
}: {
  invitation: ReturnType<typeof useSessionNodeTicket>;
  busy: boolean;
}) {
  const { t, i18n } = useTranslation("layout");
  const [runtime, setRuntime] = useState("codex");
  const { ticket, ticketExpired, creating, copying, copied, copy, create } = invitation;
  const hasCurrentTicket = !!ticket && !ticketExpired;
  return (
    <section aria-label={t("sessionNodes.prepareTitle")} aria-busy={busy} className="min-w-0">
      <SessionNodeProgress ticketIssued={hasCurrentTicket} />
      <div className="space-y-3">
        <div>
          <h3 className="text-base font-semibold">{t("sessionNodes.runRemote")}</h3>
          <p className="mt-1 text-sm leading-relaxed text-[var(--color-text-secondary)]">
            {t("sessionNodes.prepareHint")}
          </p>
        </div>
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <span className="text-sm text-[var(--color-text-secondary)]">
            {t("sessionNodes.enableApp")}
          </span>
          <SelectMenu
            ariaLabel={t("sessionNodes.enableApp")}
            value={runtime}
            onChange={setRuntime}
            disabled={busy}
            options={[
              { value: "codex", label: "Codex" },
              { value: "claude", label: "Claude Code" },
            ]}
            className="min-h-10 max-sm:min-h-11 max-w-full text-sm"
          />
        </div>
        <SessionCommand
          key={runtime}
          emphasis="secondary"
          command={`cccc sessions node connect --runtime ${runtime} --ticket-stdin`}
          copyLabel={t("sessionNodes.copyConnectCommand")}
          disabled={busy}
        />
        <div className="space-y-2 border-t border-[var(--glass-border-subtle)] pt-3">
          <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
            {t("sessionNodes.ticketGenerateHint")}
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              className="min-h-10 max-sm:min-h-11 whitespace-normal text-sm"
              disabled={busy}
              onClick={() => void copy()}
            >
              {t(
                creating
                  ? "sessionNodes.preparing"
                  : copying
                    ? "sessionNodes.copyingTicket"
                    : hasCurrentTicket
                      ? "sessionNodes.copyTicket"
                      : "sessionNodes.create",
              )}
            </Button>
            {hasCurrentTicket && (
              <Button
                type="button"
                variant="outline"
                className="min-h-10 max-sm:min-h-11 whitespace-normal text-sm"
                disabled={busy}
                onClick={() => void create()}
              >
                {t("sessionNodes.createAnother")}
              </Button>
            )}
          </div>
          {copied && hasCurrentTicket && (
            <p role="status" className="text-xs text-[var(--color-text-secondary)]">
              {t("common:copied")}
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
            <p className="text-xs text-[var(--color-text-secondary)]">
              {t("sessionNodes.ticketDirectoryHint")}
            </p>
          )}
        </div>
        <details className="border-t border-[var(--glass-border-subtle)] pt-1">
          <summary className="cursor-pointer py-2 text-sm text-[var(--color-text-secondary)]">
            {t("sessionNodes.legacyTitle")}
          </summary>
          <div className="space-y-2 text-sm leading-relaxed text-[var(--color-text-secondary)]">
            <p>{t("sessionNodes.runtimeHint")}</p>
            <p>{t("sessionNodes.legacyHint")}</p>
            {runtime === "codex" && (
              <p>
                <strong>{t("sessionNodes.codexRequirements")}</strong>{" "}
                {t("sessionNodes.codexRequirementHint")}
              </p>
            )}
          </div>
        </details>
      </div>
    </section>
  );
}
