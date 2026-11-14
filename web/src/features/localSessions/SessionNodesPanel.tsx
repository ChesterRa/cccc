import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { SelectMenu } from "../../components/ui/select-menu";
import { SessionCommand } from "./SessionCommand";
import { SessionNodeDevices } from "./SessionNodeDevices";
import { useSessionNodes } from "./useSessionNodes";

function Step({ number, title, children }: { number: number; title: string; children: ReactNode }) {
  return (
    <section className="min-w-0 space-y-4 rounded-xl border border-[var(--glass-border-subtle)] p-4 sm:p-5">
      <div className="flex items-center gap-3">
        <span
          className="flex h-6 w-6 shrink-0 items-center justify-center border border-[var(--glass-border)] text-sm font-medium"
          aria-hidden="true"
        >
          {number}
        </span>
        <h4 className="text-sm font-semibold">{title}</h4>
      </div>
      <div className="min-w-0 space-y-3 pl-9 text-sm leading-relaxed">{children}</div>
    </section>
  );
}

export function SessionNodesPanel({ onChooseSessions }: { onChooseSessions: () => void }) {
  const { t, i18n } = useTranslation("layout");
  const [runtime, setRuntime] = useState("codex");
  const state = useSessionNodes();
  return (
    <div className="grid min-w-0 grid-cols-1 gap-6 lg:grid-cols-2">
      <Step number={1} title={t("sessionNodes.hubStep")}>
        {state.supported === false ? (
          <div
            role="status"
            className="space-y-2 border border-[var(--glass-border)] bg-[var(--color-bg-primary)] p-3"
          >
            <p className="font-medium">{t("sessionNodes.backendUpgradeTitle")}</p>
            <p className="text-[var(--color-text-secondary)]">
              {t("sessionNodes.upgradeRequired")}
            </p>
          </div>
        ) : (
          <>
            <Button
              variant="outline"
              className="min-h-11 text-sm"
              disabled={state.busy || state.supported !== true}
              onClick={() => void state.create()}
            >
              {t(state.ticket ? "sessionNodes.createAnother" : "sessionNodes.create")}
            </Button>
            {state.supported === null && <p role="status">{t("sessionNodes.checkingBackend")}</p>}
          </>
        )}
        {state.ticket && (
          <div className="space-y-2">
            <Button className="min-h-11 text-sm" onClick={() => void state.copy()}>
              {t(state.copied ? "common:copied" : "sessionNodes.copyTicket")}
            </Button>
            <p role="status" className="text-sm text-[var(--color-text-secondary)]">
              {t("sessionNodes.expires", {
                time: new Date(state.ticket.expires_at).toLocaleTimeString(i18n.language),
              })}
            </p>
          </div>
        )}
      </Step>
      <Step number={2} title={t("sessionNodes.remoteStep")}>
        <SelectMenu
          ariaLabel={t("localSessions.setupRuntime")}
          value={runtime}
          onChange={setRuntime}
          options={[
            { value: "codex", label: "Codex" },
            { value: "claude", label: "Claude Code" },
          ]}
          className="min-h-11 w-full text-sm"
        />
        <SessionCommand
          key={runtime}
          command={`cccc sessions enable --runtime ${runtime}\ncccc sessions node connect --ticket-stdin --name laptop`}
        />
        <p className="text-[var(--color-text-secondary)]">{t("sessionNodes.remoteStepHint")}</p>
      </Step>
      <div className="space-y-6 lg:col-span-2">
        <Step number={3} title={t("sessionNodes.selectStep")}>
          <p className="text-[var(--color-text-secondary)]">{t("sessionNodes.selectStepHint")}</p>
          <Button variant="outline" className="min-h-11 text-sm" onClick={onChooseSessions}>
            {t("localSessions.showSessions")}
          </Button>
        </Step>
        <SessionNodeDevices
          devices={state.devices}
          busy={state.busy}
          revokeId={state.revokeId}
          setRevokeId={state.setRevokeId}
          revoke={state.revoke}
        />
        {state.error && (
          <p role="alert" className="break-words text-sm text-[var(--color-danger)]">
            {state.error}
          </p>
        )}
      </div>
    </div>
  );
}
