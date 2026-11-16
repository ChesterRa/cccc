import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { SessionNodeDevices } from "./SessionNodeDevices";
import { SessionNodePreparation } from "./SessionNodePreparation";
import { useSessionNodes } from "./useSessionNodes";

export function SessionNodesPanel({ onChooseSessions }: { onChooseSessions: () => void }) {
  const { t } = useTranslation("layout");
  const state = useSessionNodes();
  return (
    <div className="min-w-0 space-y-6">
      <SessionNodePreparation state={state} />
      <section className="min-w-0 space-y-3 border border-[var(--glass-border-subtle)] p-4 sm:p-5">
        <h4 className="text-sm font-semibold">{t("sessionNodes.selectStep")}</h4>
        <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
          {t("sessionNodes.selectStepHint")}
        </p>
        <Button variant="outline" className="min-h-11 text-sm" onClick={onChooseSessions}>
          {t("localSessions.showSessions")}
        </Button>
      </section>
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
  );
}
