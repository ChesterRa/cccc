import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { SelectMenu } from "../../components/ui/select-menu";
import { SessionCommand } from "./SessionCommand";
import type { useSessionNodes } from "./useSessionNodes";

export function SessionNodePreparation({ state }: { state: ReturnType<typeof useSessionNodes> }) {
  const { t, i18n } = useTranslation("layout");
  const [runtime, setRuntime] = useState("codex");
  return (
    <section
      aria-label={t("sessionNodes.prepareTitle")}
      className="min-w-0 space-y-5 border border-[var(--glass-border-subtle)] p-4 font-mono sm:p-5"
      aria-busy={state.busy}
    >
      <div className="space-y-2">
        <h4 className="text-sm font-semibold">{t("sessionNodes.prepareTitle")}</h4>
        <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
          {t("sessionNodes.prepareHint")}
        </p>
      </div>
      <div className="space-y-2">
        <p className="text-sm">{t("sessionNodes.enableApp")}</p>
        <SelectMenu
          ariaLabel={t("sessionNodes.enableApp")}
          value={runtime}
          onChange={setRuntime}
          disabled={state.busy}
          options={[
            { value: "codex", label: "Codex" },
            { value: "claude", label: "Claude Code" },
          ]}
          className="min-h-11 w-full text-sm"
        />
        <p className="text-xs leading-relaxed text-[var(--color-text-secondary)]">
          {t("sessionNodes.runtimeHint")}
        </p>
      </div>
      <div className="min-w-0 space-y-2">
        <p className="text-sm">{t("sessionNodes.runRemote")}</p>
        <SessionCommand
          key={runtime}
          command={`cccc sessions node connect --runtime ${runtime} --ticket-stdin --name laptop`}
          copyLabel={t("sessionNodes.copyConnectCommand")}
          disabled={state.busy}
        />
        <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
          {t("sessionNodes.stdinHint")}
        </p>
      </div>
      <details className="min-w-0 border-t border-[var(--glass-border-subtle)] pt-3 text-sm">
        <summary className="cursor-pointer leading-relaxed">
          {t("sessionNodes.legacyTitle")}
        </summary>
        <div className="min-w-0 space-y-3 pt-3">
          <p className="leading-relaxed text-[var(--color-text-secondary)]">
            {t("sessionNodes.legacyHint")}
          </p>
          <SessionCommand
            key={`legacy-${runtime}`}
            command={`cccc sessions enable --runtime ${runtime}\ncccc sessions node connect --ticket-stdin --name laptop`}
            copyLabel={t("sessionNodes.copyLegacyCommands")}
            disabled={state.busy}
          />
        </div>
      </details>
      {state.supported === false ? (
        <div role="status" className="space-y-2 border border-[var(--glass-border)] p-3 text-sm">
          <p className="font-medium">{t("sessionNodes.backendUpgradeTitle")}</p>
          <p>{t("sessionNodes.upgradeRequired")}</p>
        </div>
      ) : (
        <div className="space-y-2">
          <div className="flex flex-wrap gap-2">
            <Button
              className="min-h-11 whitespace-normal text-sm"
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
                className="min-h-11 whitespace-normal text-sm"
                disabled={state.busy}
                onClick={() => void state.create()}
              >
                {t("sessionNodes.createAnother")}
              </Button>
            )}
          </div>
          {state.copied && !state.ticketExpired && (
            <p role="status" className="text-sm text-[var(--color-text-secondary)]">
              {t("common:copied")}
            </p>
          )}
          {state.supported === null && (
            <p role="status" className="text-sm">
              {t("sessionNodes.checkingBackend")}
            </p>
          )}
          {state.ticket && (
            <p role="status" className="break-words text-sm text-[var(--color-text-secondary)]">
              {state.ticketExpired
                ? t("sessionNodes.ticketExpired")
                : t("sessionNodes.expires", {
                    time: new Date(state.ticket.expires_at).toLocaleTimeString(i18n.language),
                  })}
            </p>
          )}
        </div>
      )}
      {runtime === "codex" && (
        <details className="border-t border-[var(--glass-border-subtle)] pt-3 text-sm">
          <summary className="cursor-pointer leading-relaxed">
            {t("sessionNodes.codexRequirements")}
          </summary>
          <p className="pt-3 leading-relaxed text-[var(--color-text-secondary)]">
            {t("sessionNodes.codexRequirementHint")}
          </p>
        </details>
      )}
    </section>
  );
}
