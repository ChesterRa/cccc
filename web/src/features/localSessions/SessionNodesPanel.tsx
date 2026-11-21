import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { SessionNodeDevices } from "./SessionNodeDevices";
import { SessionNodePreparation } from "./SessionNodePreparation";
import { SessionNodeJoinPreparation } from "./SessionNodeJoinPreparation";
import { SessionNodeJoinDialog } from "./SessionNodeJoinDialog";
import { useSessionNodes } from "./useSessionNodes";

export function SessionNodesPanel({ onChooseSessions }: { onChooseSessions: () => void }) {
  const { t } = useTranslation("layout");
  const state = useSessionNodes();
  const [reviewId, setReviewId] = useState<string | null>(null);
  return (
    <div className="min-w-0 space-y-6">
      {state.join ? (
        <>
          <SessionNodeJoinPreparation join={state.join} onReview={setReviewId} />
          <details className="min-w-0 border border-[var(--glass-border-subtle)] p-4 sm:p-5">
            <summary className="cursor-pointer text-sm font-semibold">
              {t("sessionNodes.joinTicketFallback")}
            </summary>
            <div className="mt-3">
              <SessionNodePreparation state={state} />
            </div>
          </details>
        </>
      ) : (
        <SessionNodePreparation state={state} />
      )}
      {reviewId !== null && (
        <SessionNodeJoinDialog requestId={reviewId} onClose={() => setReviewId(null)} />
      )}
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
