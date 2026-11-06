import { useTranslation } from "react-i18next";
import type { ChatMessageData, LedgerEvent } from "../../types";
import { classNames } from "../../utils/classNames";
import type { WebModelDeliveryStatus } from "../../utils/webModelDeliveryStatus";
import { WebModelDeliveryRecovery } from "../webModel/WebModelDeliveryRecovery";

// A framed label in the status tone: no fill, no rounding.
const STATUS_LABEL_CLASS =
  "inline-flex min-w-0 max-w-full items-center gap-1.5 border px-2 py-0.5 text-xs font-semibold tracking-tight";
// A reply still owed reads like any status; one cancelled without a reply steps back.
const OBLIGATION_TONE = { replied: "ok", open: "neutral", closed: "muted" } as const;

export function MessageFooter({
  readOnly,
  obligationSummary,
  visibleReadStatusEntries,
  webModelDeliveryStatus,
  readPreviewEntries,
  readPreviewOverflow,
  displayNameMap,
  isDark,
  onShowRecipients,
  event,
}: {
  readOnly?: boolean;
  obligationSummary: { replied: number; cancelled: number; total: number } | null;
  visibleReadStatusEntries: readonly (readonly [string, boolean])[];
  webModelDeliveryStatus?: WebModelDeliveryStatus;
  readPreviewEntries: readonly (readonly [string, boolean])[];
  readPreviewOverflow: number;
  displayNameMap: Map<string, string>;
  isDark: boolean;
  onShowRecipients: () => void;
  event: LedgerEvent;
}) {
  const { t } = useTranslation(["chat", "common"]);

  const renderRecipientStatus = () => (
    <div className="flex min-w-0 items-center gap-2">
      {readPreviewEntries.map(([id, cleared]) => (
        <span key={id} className="inline-flex min-w-0 items-center gap-1">
          <span className="max-w-[10ch] truncate">{displayNameMap.get(id) || id}</span>
          <span
            className={classNames(
              "text-xs font-semibold tracking-tight",
              cleared ? "text-[var(--color-text-primary)]" : "text-[var(--color-text-tertiary)]",
            )}
            aria-label={cleared ? t("read") : t("pending")}
          >
            {cleared ? "✓✓" : "✓"}
          </span>
        </span>
      ))}
      {readPreviewOverflow > 0 ? (
        <span className={classNames("text-xs", "text-[var(--color-text-tertiary)]")}>
          +{readPreviewOverflow}
        </span>
      ) : null}
    </div>
  );

  const connectDelivery =
    event._connect_delivery ??
    ((event.data as ChatMessageData | undefined)?.dst_instance_id
      ? { state: "queued" as const }
      : undefined);
  const connectCancellation = event._connect_cancellation;
  // Mail stays in the Inbox whatever became of an extra push. A Web Model shows
  // its own delivery above, with its way to recover.
  const isMail = (event.data as ChatMessageData | undefined)?.message_mode === "mail";
  const undelivered = Object.entries(isMail ? {} : (event._obligation_status ?? {})).filter(
    ([id, status]) => status.delivery_state === "failed" && id !== webModelDeliveryStatus?.actorId,
  );
  const undeliveredLabel = undelivered.length
    ? t("deliveryFailedFor", {
        names: undelivered.map(([id]) => displayNameMap.get(id) || id).join(", "),
      })
    : "";
  const undeliveredReason = undelivered.find(([, status]) => status.delivery_reason)?.[1]
    .delivery_reason;
  const deliveryLabel = webModelDeliveryStatus
    ? t(`webModelDelivery.${webModelDeliveryStatus.state}`)
    : "";
  const deliveryDetail = String(webModelDeliveryStatus?.detail || "").trim();
  // Tones, not palette colours: the waiting colour is kept for what waits on the user.
  const deliveryTone =
    webModelDeliveryStatus?.state === "failed"
      ? "error"
      : webModelDeliveryStatus?.state === "submitted" || webModelDeliveryStatus?.state === "bound"
        ? "ok"
        : "muted";
  // A cancellation closes an obligation without a reply, so it must not look like one.
  const obligationState = obligationSummary
    ? obligationSummary.replied >= obligationSummary.total
      ? "replied"
      : obligationSummary.replied + obligationSummary.cancelled >= obligationSummary.total
        ? "closed"
        : "open"
    : null;
  const obligationText = obligationSummary
    ? `${t("reply")} ${obligationSummary.replied}/${obligationSummary.total}${
        obligationSummary.cancelled
          ? ` · ${t("replyCancelledCount", { cancelled: obligationSummary.cancelled })}`
          : ""
      }`
    : "";
  const allRead =
    visibleReadStatusEntries.length > 0 &&
    visibleReadStatusEntries.every(([_, cleared]) => cleared);
  const hasStatus = Boolean(
    connectDelivery ||
    connectCancellation ||
    webModelDeliveryStatus ||
    undelivered.length > 0 ||
    obligationSummary ||
    visibleReadStatusEntries.length > 0,
  );
  if (!hasStatus) return null;

  return (
    <div
      className={classNames(
        "mt-1 flex w-full min-w-0 flex-wrap items-center gap-2 px-1 text-xs",
        event.by === "user" ? "flex-row-reverse" : "justify-start",
        "text-[var(--color-text-tertiary)]",
      )}
    >
      <div
        className={classNames(
          "flex min-w-0 flex-wrap items-center gap-1.5",
          event.by === "user" ? "mr-auto" : "ml-auto",
        )}
      >
        {connectDelivery ? (
          <span
            className={STATUS_LABEL_CLASS}
            data-status-tone={connectDelivery.state === "failed" ? "error" : "muted"}
            title={connectDelivery.error || t(`connectDelivery.${connectDelivery.state}Hint`)}
          >
            {t(`connectDelivery.${connectDelivery.state}`)}
          </span>
        ) : null}
        {connectCancellation ? (
          <span
            className={STATUS_LABEL_CLASS}
            data-status-tone="muted"
            title={
              connectCancellation.error || t(`connectCancellation.${connectCancellation.state}Hint`)
            }
          >
            {t(`connectCancellation.${connectCancellation.state}`)}
          </span>
        ) : null}
        {webModelDeliveryStatus ? (
          <span
            className={STATUS_LABEL_CLASS}
            data-status-tone={deliveryTone}
            title={deliveryDetail || undefined}
          >
            {webModelDeliveryStatus.state === "submitting" ? (
              <span
                className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-current"
                data-status-motion="working"
                aria-hidden="true"
              />
            ) : null}
            <span className="truncate">{deliveryLabel}</span>
          </span>
        ) : null}
        {webModelDeliveryStatus ? (
          <WebModelDeliveryRecovery
            groupId={event.group_id}
            status={webModelDeliveryStatus}
            actorLabel={
              displayNameMap.get(webModelDeliveryStatus.actorId) || webModelDeliveryStatus.actorId
            }
            isDark={isDark}
            readOnly={readOnly}
          />
        ) : null}
        {undelivered.length > 0 ? (
          readOnly ? (
            <span className={STATUS_LABEL_CLASS} data-status-tone="error">
              <span className="truncate">{undeliveredLabel}</span>
            </span>
          ) : (
            <button
              type="button"
              className={classNames("touch-target-sm", STATUS_LABEL_CLASS)}
              data-status-tone="error"
              title={undeliveredReason}
              onClick={onShowRecipients}
            >
              <span className="truncate">{undeliveredLabel}</span>
            </button>
          )
        ) : null}
        {obligationSummary ? (
          readOnly ? (
            <div
              className={STATUS_LABEL_CLASS}
              data-status-tone={OBLIGATION_TONE[obligationState!]}
              data-obligation-state={obligationState}
            >
              <span className="text-xs font-semibold tracking-tight">{obligationText}</span>
            </div>
          ) : (
            <button
              type="button"
              className={classNames("touch-target-sm", STATUS_LABEL_CLASS)}
              data-status-tone={OBLIGATION_TONE[obligationState!]}
              data-obligation-state={obligationState}
              onClick={onShowRecipients}
              aria-label={t(
                obligationSummary.cancelled
                  ? "showObligationStatusCancelled"
                  : "showObligationStatusCounts",
                {
                  replied: obligationSummary.replied,
                  cancelled: obligationSummary.cancelled,
                  total: obligationSummary.total,
                },
              )}
            >
              <span className="text-xs font-semibold tracking-tight">{obligationText}</span>
            </button>
          )
        ) : visibleReadStatusEntries.length > 0 ? (
          readOnly ? (
            <div className={STATUS_LABEL_CLASS} data-status-tone={allRead ? "ok" : "muted"}>
              {renderRecipientStatus()}
            </div>
          ) : (
            <button
              type="button"
              className={classNames("touch-target-sm", STATUS_LABEL_CLASS)}
              data-status-tone={allRead ? "ok" : "muted"}
              onClick={onShowRecipients}
              aria-label={t("showRecipientStatus")}
            >
              {renderRecipientStatus()}
            </button>
          )
        ) : null}
      </div>
    </div>
  );
}
