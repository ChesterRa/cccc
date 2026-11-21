import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowLeft, ChevronDown, Ticket } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import type { SessionNodeJoinDetail } from "../../services/api/sessionNodes";
import { SessionNodeDevices } from "./SessionNodeDevices";
import { SessionNodePreparation } from "./SessionNodePreparation";
import { SessionNodeJoinPreparation } from "./SessionNodeJoinPreparation";
import { SessionNodeJoinReview } from "./SessionNodeJoinReview";
import { useSessionNodes } from "./useSessionNodes";

export function SessionNodesPanel({
  onChooseSessions,
  onAdvertisedUrl,
}: {
  onChooseSessions: () => void;
  onAdvertisedUrl?: (url: string | null) => void;
}) {
  const { t } = useTranslation("layout");
  const state = useSessionNodes();
  const advertisedUrl = state.join?.url ?? null;
  useEffect(() => {
    onAdvertisedUrl?.(advertisedUrl);
    return () => onAdvertisedUrl?.(null);
  }, [advertisedUrl, onAdvertisedUrl]);
  const [reviewId, setReviewId] = useState<string | null>(null);
  const [ticketMode, setTicketMode] = useState(false);
  const [managed, setManaged] = useState(false);
  const [handled, setHandled] = useState<SessionNodeJoinDetail[]>([]);
  const returnFocus = useRef<HTMLDivElement>(null);
  const { refreshDirectory } = state;
  const remember = useCallback(
    (request: SessionNodeJoinDetail) => {
      if (
        handled.some(
          (item) => item.request_id === request.request_id && item.status === request.status,
        )
      )
        return;
      if (request.status === "approved") refreshDirectory();
      setHandled((previous) => [
        request,
        ...previous.filter((item) => item.request_id !== request.request_id),
      ]);
    },
    [handled, refreshDirectory],
  );
  const showTicket =
    ticketMode || state.supported === false || (state.directoryStatus === "ready" && !state.join);
  const otherRequests =
    state.join?.pending?.filter(
      (r) => r.request_id !== reviewId && !handled.some((done) => done.request_id === r.request_id),
    ).length || 0;
  function back() {
    setReviewId(null);
    setTicketMode(false);
    returnFocus.current?.focus({ preventScroll: true });
  }

  if (state.directoryStatus === "forbidden" || state.join?.error === "forbidden")
    return <p role="alert">{t("sessionNodes.joinForbidden")}</p>;
  return (
    <div ref={returnFocus} tabIndex={-1} className="min-w-0 space-y-3 outline-none">
      {reviewId ? (
        <SessionNodeJoinReview
          key={reviewId}
          requestId={reviewId}
          directory={state}
          onChooseSessions={onChooseSessions}
          onBack={back}
          remaining={otherRequests}
          onReviewed={remember}
        />
      ) : showTicket ? (
        <>
          <SessionNodePreparation state={state} />
          {state.join && (
            <Button type="button" variant="ghost" size="sm" className="-ml-2" onClick={back}>
              <ArrowLeft size={14} aria-hidden="true" />
              {t("sessionNodes.joinReturnCommand")}
            </Button>
          )}
        </>
      ) : state.join ? (
        <>
          <SessionNodeJoinPreparation
            join={state.join}
            handled={handled}
            devices={state.devices}
            onReview={setReviewId}
          />
          <div className="border-t border-[var(--glass-border-subtle)] pt-1.5">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="-ml-2 min-h-10 max-sm:min-h-11 whitespace-normal text-[var(--color-text-secondary)]"
              onClick={() => setTicketMode(true)}
            >
              <Ticket size={15} aria-hidden="true" />
              {t("sessionNodes.joinTicketFallback")}
            </Button>
          </div>
        </>
      ) : state.directoryStatus === "loading" ? (
        <p role="status">{t("sessionNodes.checkingBackend")}</p>
      ) : null}
      {!reviewId && state.devices.length > 0 && (
        <div className="border-t border-[var(--glass-border-subtle)]">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            aria-expanded={managed}
            className="-ml-2 mt-1 min-h-10 max-sm:min-h-11"
            onClick={() => setManaged(!managed)}
          >
            <ChevronDown size={14} aria-hidden="true" className={managed ? "rotate-180" : ""} />
            {t("sessionNodes.ticketManage")}
          </Button>
          {managed && (
            <SessionNodeDevices
              devices={state.devices}
              busy={state.busy}
              revokeId={state.revokeId}
              setRevokeId={state.setRevokeId}
              revoke={state.revoke}
              onChooseSessions={onChooseSessions}
            />
          )}
        </div>
      )}
      {state.error && (
        <p role="alert" className="break-words text-sm text-[var(--color-danger)]">
          {state.error}
        </p>
      )}
    </div>
  );
}
