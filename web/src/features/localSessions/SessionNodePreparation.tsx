import { useState } from "react";
import { Terminal } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { SelectMenu } from "../../components/ui/select-menu";
import { SessionCommand } from "./SessionCommand";
import { SessionNodeProgress } from "./SessionNodeProgress";
import type { useSessionNodes } from "./useSessionNodes";

export function SessionNodePreparation({ state }: { state: ReturnType<typeof useSessionNodes> }) {
  const { t, i18n } = useTranslation("layout");
  const [runtime, setRuntime] = useState("codex");
  const [prompted, setPrompted] = useState(false);
  const ready = prompted || !!state.ticket;
  return (
    <section aria-label={t("sessionNodes.prepareTitle")} className="min-w-0" aria-busy={state.busy}>
      <SessionNodeProgress step={ready ? 2 : 1} ticket />
      <div className="space-y-3">
        <div>
          <h3 className="text-base font-semibold">{t("sessionNodes.joinTicketFallback")}</h3>
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
            onChange={(value) => {
              setRuntime(value);
              setPrompted(false);
            }}
            disabled={state.busy}
            options={[
              { value: "codex", label: "Codex" },
              { value: "claude", label: "Claude Code" },
            ]}
            className="min-h-10 max-sm:min-h-11 max-w-full text-sm"
          />
        </div>
        <SessionCommand
          key={runtime}
          emphasis={ready ? "secondary" : "primary"}
          command={`cccc sessions node connect --runtime ${runtime} --ticket-stdin --name laptop`}
          copyLabel={t("sessionNodes.copyConnectCommand")}
          disabled={state.busy}
        />
        {state.supported === false ? (
          <div
            role="status"
            className="space-y-1 rounded-lg border border-[var(--glass-border-subtle)] p-3 text-sm"
          >
            <p className="font-semibold">{t("sessionNodes.backendUpgradeTitle")}</p>
            <p>{t("sessionNodes.upgradeRequired")}</p>
          </div>
        ) : !ready ? (
          <Button
            type="button"
            variant="outline"
            className="min-h-10 max-sm:min-h-11 whitespace-normal"
            disabled={state.supported !== true || state.busy}
            onClick={() => setPrompted(true)}
          >
            <Terminal aria-hidden="true" size={15} />
            {t("sessionNodes.ticketPromptReady")}
          </Button>
        ) : (
          <div className="space-y-2 border-t border-[var(--glass-border-subtle)] pt-3">
            <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
              {t("sessionNodes.ticketGenerateHint")}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button
                className="min-h-10 max-sm:min-h-11 whitespace-normal text-sm"
                disabled={state.busy || state.supported !== true}
                onClick={() => void state.copy()}
              >
                {t(
                  state.creating
                    ? "sessionNodes.preparing"
                    : state.copying
                      ? "sessionNodes.copyingTicket"
                      : state.ticket && !state.ticketExpired
                        ? "sessionNodes.copyTicket"
                        : "sessionNodes.create",
                )}
              </Button>
              {state.ticket && !state.ticketExpired && (
                <Button
                  variant="outline"
                  className="min-h-10 max-sm:min-h-11 whitespace-normal text-sm"
                  disabled={state.busy}
                  onClick={() => void state.create()}
                >
                  {t("sessionNodes.createAnother")}
                </Button>
              )}
            </div>
            {state.copied && !state.ticketExpired && (
              <p role="status" className="text-xs text-[var(--color-text-secondary)]">
                {t("common:copied")}
              </p>
            )}
            {state.ticket && (
              <p role="status" className="text-xs text-[var(--color-text-secondary)]">
                {state.ticketExpired
                  ? t("sessionNodes.ticketExpired")
                  : t("sessionNodes.expires", {
                      time: new Date(state.ticket.expires_at).toLocaleTimeString(i18n.language),
                    })}
              </p>
            )}
            {state.ticket && (
              <p className="text-xs text-[var(--color-text-secondary)]">
                {t("sessionNodes.ticketDirectoryHint")}
              </p>
            )}
          </div>
        )}
        <details className="border-t border-[var(--glass-border-subtle)] pt-1">
          <summary className="cursor-pointer py-2 text-sm text-[var(--color-text-secondary)]">
            {t("sessionNodes.legacyTitle")}
          </summary>
          <div className="space-y-2 text-sm leading-relaxed text-[var(--color-text-secondary)]">
            <p>{t("sessionNodes.runtimeHint")}</p>
            <p>{t("sessionNodes.legacyHint")}</p>
            <SessionCommand
              key={`legacy-${runtime}`}
              command={`cccc sessions enable --runtime ${runtime}\ncccc sessions node connect --ticket-stdin --name laptop`}
              copyLabel={t("sessionNodes.copyLegacyCommands")}
              emphasis="secondary"
              disabled={state.busy}
            />
            <p>{t("sessionNodes.stdinHint")}</p>
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
