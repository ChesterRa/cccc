import { useEffect, useLayoutEffect, useRef } from "react";
import { ArrowLeft, Check } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import type { SessionNodeJoinDetail } from "../../services/api/sessionNodes";
import {
  SessionNodeApprovalStatus,
  SessionNodeIdentity,
  LinkedSessionNodeStatus,
  type DeviceDirectory,
} from "./SessionNodeApprovalStatus";
import { SessionNodeProgress } from "./SessionNodeProgress";
import { useSessionNodeJoinReview } from "./useSessionNodeJoinReview";

export function SessionNodeJoinReview({
  requestId,
  directory,
  onChooseSessions,
  onBack,
  remaining = 0,
  onReviewed,
}: {
  requestId: string;
  directory?: DeviceDirectory;
  onChooseSessions: () => void;
  onBack?: () => void;
  remaining?: number;
  onReviewed?: (request: SessionNodeJoinDetail) => void;
}) {
  const { t, i18n } = useTranslation("layout");
  const { view, busy, uncertain, decide, refresh } = useSessionNodeJoinReview(requestId);
  const focus = useRef<HTMLDivElement>(null);
  const inspected = view.kind === "request" ? view.request : null;
  const status = inspected?.status ?? view.kind;
  useEffect(() => {
    focus.current?.focus({ preventScroll: true });
  }, [requestId, status]);
  useLayoutEffect(() => {
    if (inspected && inspected.status !== "pending") onReviewed?.(inspected);
  }, [inspected, onReviewed]);

  const back = onBack && (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      className="-ml-2 min-h-10 max-sm:min-h-11"
      onClick={onBack}
      disabled={busy}
    >
      <ArrowLeft aria-hidden="true" size={15} />
      {t(
        remaining && inspected?.status !== "pending"
          ? "sessionNodes.joinOthers"
          : "sessionNodes.joinBack",
        { count: remaining },
      )}
    </Button>
  );
  if (view.kind !== "request") {
    const messages = {
      loading: "common:loading",
      missing: "sessionNodes.joinUnavailable",
      expired: "sessionNodes.joinExpired",
      forbidden: "sessionNodes.joinForbidden",
      failed: "sessionNodes.joinReadFailed",
    } as const;
    return (
      <div ref={focus} tabIndex={-1} className="outline-none">
        <SessionNodeProgress step={2} />
        <p role={view.kind === "failed" || view.kind === "forbidden" ? "alert" : "status"}>
          {t(messages[view.kind])}
        </p>
        {view.kind === "failed" && (
          <Button variant="outline" size="sm" onClick={refresh}>
            {t("common:retry")}
          </Button>
        )}
        {back}
      </div>
    );
  }
  const { request } = view;
  return (
    <div
      ref={focus}
      tabIndex={-1}
      aria-label={t("sessionNodes.joinReviewTitle")}
      className="outline-none"
    >
      {request.status === "approved" ? (
        directory ? (
          <SessionNodeApprovalStatus
            request={request}
            directory={directory}
            onChooseSessions={onChooseSessions}
          />
        ) : (
          <LinkedSessionNodeStatus request={request} onChooseSessions={onChooseSessions} />
        )
      ) : (
        <>
          <SessionNodeProgress step={2} />
          {request.status === "pending" ? (
            <div className="space-y-3">
              {back}
              <h3 className="text-base font-semibold">{t("sessionNodes.joinReviewTitle")}</h3>
              <p className="text-sm leading-relaxed text-[var(--color-text-secondary)]">
                {t("sessionNodes.joinVerifyHint")}
              </p>
              <SessionNodeIdentity request={request} />
              <p className="text-xs text-[var(--color-text-secondary)]">
                {t("sessionNodes.joinExpires", {
                  time: new Date(request.expires_at).toLocaleTimeString(i18n.language),
                })}
              </p>
              {uncertain && (
                <p role="alert" className="text-sm text-[var(--color-danger)]">
                  {t("sessionNodes.joinDecideFailed")}
                </p>
              )}
              <div className="flex flex-wrap items-center gap-2" aria-busy={busy}>
                <Button type="button" disabled={busy} onClick={() => void decide("approve")}>
                  <Check aria-hidden="true" size={16} />
                  {t("sessionNodes.joinAllow")}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  className="text-[var(--color-danger)]"
                  disabled={busy}
                  onClick={() => void decide("deny")}
                >
                  {t("sessionNodes.joinDeny")}
                </Button>
              </div>
            </div>
          ) : (
            <div className="space-y-3">
              <p role="status" className="font-medium">
                {t(
                  request.status === "denied"
                    ? "sessionNodes.joinDenied"
                    : "sessionNodes.joinExpired",
                )}
              </p>
              <SessionNodeIdentity request={request} />
            </div>
          )}
        </>
      )}
      {request.status !== "pending" && back && <div className="mt-2">{back}</div>}
    </div>
  );
}
