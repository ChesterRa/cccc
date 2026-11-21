import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { useSessionNodeJoinReview } from "./useSessionNodeJoinReview";

export function SessionNodeJoinReview({ requestId }: { requestId: string }) {
  const { t, i18n } = useTranslation("layout");
  const { view, busy, uncertain, decide, refresh } = useSessionNodeJoinReview(requestId);
  if (view.kind !== "request") {
    const messages = {
      loading: "common:loading",
      missing: "sessionNodes.joinUnavailable",
      expired: "sessionNodes.joinExpired",
      forbidden: "sessionNodes.joinForbidden",
      failed: "sessionNodes.joinReadFailed",
    } as const;
    return (
      <div className="space-y-3">
        <p role={view.kind === "failed" || view.kind === "forbidden" ? "alert" : "status"}>
          {t(messages[view.kind])}
        </p>
        {view.kind === "failed" && (
          <Button type="button" variant="outline" onClick={refresh}>
            {t("common:retry")}
          </Button>
        )}
      </div>
    );
  }
  const { request } = view;
  return (
    <div className="min-w-0 space-y-4">
      <p className="break-words text-base font-semibold" dir="auto">
        {request.name}
      </p>
      {request.status === "pending" ? (
        <>
          <div className="space-y-1">
            <p className="text-sm text-[var(--color-text-secondary)]">
              {t("sessionNodes.joinCode")}
            </p>
            <code dir="ltr" className="select-all break-all text-2xl font-semibold tracking-widest">
              {request.user_code}
            </code>
          </div>
          <p className="text-sm leading-relaxed">{t("sessionNodes.joinVerifyHint")}</p>
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
          <div className="flex flex-wrap gap-2" aria-busy={busy}>
            <Button
              type="button"
              variant="outline"
              className="min-h-11"
              disabled={busy}
              onClick={() => void decide("deny")}
            >
              {t("sessionNodes.joinDeny")}
            </Button>
            <Button
              type="button"
              className="min-h-11"
              disabled={busy}
              onClick={() => void decide("approve")}
            >
              {t("sessionNodes.joinAllow")}
            </Button>
          </div>
        </>
      ) : (
        <p role="status" className="text-sm leading-relaxed">
          {t(
            request.status === "approved"
              ? "sessionNodes.joinApproved"
              : request.status === "denied"
                ? "sessionNodes.joinDenied"
                : "sessionNodes.joinExpired",
          )}
        </p>
      )}
    </div>
  );
}
