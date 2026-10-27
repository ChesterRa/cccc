export { MessageMetadataHeader } from "./MessageMetadataHeader";
import { MessageActions } from "./MessageActions";
import { useTranslation } from "react-i18next";
import type { ChatMessageData, LedgerEvent } from "../../types";
import { classNames } from "../../utils/classNames";
import type { WebModelDeliveryStatus } from "../../utils/webModelDeliveryStatus";
import { WebModelDeliveryRecovery } from "../webModel/WebModelDeliveryRecovery";

export function MessageFooter({
  readOnly,
  obligationSummary,
  visibleReadStatusEntries,
  webModelDeliveryStatus,
  readPreviewEntries,
  readPreviewOverflow,
  displayNameMap,
  isDark,
  replyRequested,
  copiedMessageText,
  copyableMessageText,
  onCopyMessageText,
  onShowRecipients,
  onCopyLink,
  onViewContext,
  onRelay,
  onReply,
  canReply,
  eventId,
  event,
}: {
  readOnly?: boolean;
  obligationSummary: { done: number; total: number } | null;
  visibleReadStatusEntries: readonly (readonly [string, boolean])[];
  webModelDeliveryStatus?: WebModelDeliveryStatus;
  readPreviewEntries: readonly (readonly [string, boolean])[];
  readPreviewOverflow: number;
  displayNameMap: Map<string, string>;
  isDark: boolean;
  isMail: boolean;
  replyRequested: boolean;
  copiedMessageText: boolean;
  copyableMessageText: string;
  onCopyMessageText: () => void;
  onShowRecipients: () => void;
  onCopyLink?: (eventId: string) => void;
  onViewContext?: (eventId: string) => void;
  onRelay?: (ev: LedgerEvent) => void;
  onReply: () => void;
  canReply: boolean;
  eventId?: string;
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
              cleared
                ? isDark
                  ? "text-emerald-400"
                  : "text-emerald-600"
                : isDark
                  ? "text-slate-500"
                  : "text-gray-500",
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
  const deliveryLabel = webModelDeliveryStatus
    ? t(`webModelDelivery.${webModelDeliveryStatus.state}`)
    : "";
  const deliveryDetail = String(webModelDeliveryStatus?.detail || "").trim();
  const deliveryToneClass =
    webModelDeliveryStatus?.state === "failed"
      ? "border-rose-500/20 bg-rose-500/8 text-rose-700 dark:text-rose-300"
      : webModelDeliveryStatus?.state === "submitted" || webModelDeliveryStatus?.state === "bound"
        ? "border-emerald-500/20 bg-emerald-500/8 text-emerald-700 dark:text-emerald-300"
        : "border-amber-500/20 bg-amber-500/8 text-amber-700 dark:text-amber-300";
  const allRead =
    visibleReadStatusEntries.length > 0 &&
    visibleReadStatusEntries.every(([_, cleared]) => cleared);

  return (
    <div
      className={classNames(
        "mt-1 flex w-full min-w-0 flex-wrap items-center gap-2 px-1 text-xs",
        event.by === "user" ? "flex-row-reverse" : "justify-start",

        "text-[var(--color-text-tertiary)]",
      )}
    >
      <MessageActions
        readOnly={readOnly}
        copiedMessageText={copiedMessageText}
        copyableMessageText={copyableMessageText}
        onCopyMessageText={onCopyMessageText}
        onViewContext={onViewContext}
        onCopyLink={onCopyLink}
        onRelay={onRelay}
        onReply={onReply}
        canReply={canReply}
        eventId={eventId}
        event={event}
      />
      <div
        className={classNames(
          "flex min-w-0 flex-wrap items-center gap-1.5",
          event.by === "user" ? "mr-auto" : "ml-auto",
        )}
      >
        {connectDelivery ? (
          <span
            className={classNames(
              "max-w-full rounded-full border px-2.5 py-1 text-xs font-semibold",
              connectDelivery.state === "failed"
                ? "border-rose-500/20 text-rose-700 dark:text-rose-300"
                : "border-[var(--glass-border-subtle)]",
            )}
            title={connectDelivery.error || t(`connectDelivery.${connectDelivery.state}Hint`)}
          >
            {t(`connectDelivery.${connectDelivery.state}`)}
          </span>
        ) : null}
        {connectCancellation ? (
          <span
            className="max-w-full rounded-full border border-[var(--glass-border-subtle)] px-2.5 py-1 text-xs font-semibold"
            title={
              connectCancellation.error || t(`connectCancellation.${connectCancellation.state}Hint`)
            }
          >
            {t(`connectCancellation.${connectCancellation.state}`)}
          </span>
        ) : null}
        {webModelDeliveryStatus ? (
          <span
            className={classNames(
              "inline-flex max-w-full items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-semibold tracking-tight",
              deliveryToneClass,
            )}
            title={deliveryDetail || undefined}
          >
            {webModelDeliveryStatus.state === "submitting" ? (
              <span
                className="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-current"
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
        {obligationSummary ? (
          readOnly ? (
            <div
              className={classNames(
                "flex min-w-0 items-center gap-2 rounded-full border px-2.5 py-1",
                obligationSummary.done >= obligationSummary.total
                  ? "border-emerald-500/20 bg-emerald-500/5 text-emerald-600 dark:text-emerald-400 dark:border-emerald-500/20 dark:bg-emerald-500/5"
                  : "border-amber-500/20 bg-amber-500/5 text-amber-600 dark:text-amber-400 dark:border-amber-500/20 dark:bg-amber-500/5",
              )}
            >
              <span className="text-xs font-semibold tracking-tight">
                {t("reply")} {obligationSummary.done}/{obligationSummary.total}
              </span>
            </div>
          ) : (
            <button
              type="button"
              className={classNames(
                "touch-target-sm flex min-w-0 items-center gap-2 rounded-full border px-2.5 py-1 transition-all duration-150",
                obligationSummary.done >= obligationSummary.total
                  ? "border-emerald-500/25 bg-emerald-500/6 hover:bg-emerald-500/12 text-emerald-600 dark:text-emerald-400 dark:border-emerald-500/20 dark:bg-emerald-500/8 dark:hover:bg-emerald-500/15"
                  : "border-amber-500/25 bg-amber-500/6 hover:bg-amber-500/12 text-amber-600 dark:text-amber-400 dark:border-amber-500/20 dark:bg-amber-500/8 dark:hover:bg-amber-500/15",
              )}
              onClick={onShowRecipients}
              aria-label={t("showObligationStatus")}
            >
              <span className="text-xs font-semibold tracking-tight">
                {t("reply")} {obligationSummary.done}/{obligationSummary.total}
              </span>
            </button>
          )
        ) : visibleReadStatusEntries.length > 0 ? (
          readOnly ? (
            <div
              className={classNames(
                "flex min-w-0 items-center gap-2 rounded-full border px-2.5 py-1",
                allRead
                  ? "border-emerald-500/15 bg-emerald-500/5 text-emerald-600 dark:text-emerald-400 dark:border-emerald-500/15 dark:bg-emerald-500/5"
                  : "border-black/5 bg-black/[0.025] dark:border-white/8 dark:bg-white/[0.035]",
              )}
            >
              {renderRecipientStatus()}
            </div>
          ) : (
            <button
              type="button"
              className={classNames(
                "touch-target-sm flex min-w-0 items-center gap-2 rounded-full border px-2.5 py-1 transition-all duration-150",
                allRead
                  ? "border-emerald-500/15 bg-emerald-500/5 hover:bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 dark:border-emerald-500/15 dark:bg-emerald-500/5 dark:hover:bg-emerald-500/10"
                  : "border-black/5 bg-black/[0.025] hover:bg-black/[0.045] dark:border-white/8 dark:bg-white/[0.035] dark:hover:bg-white/[0.055]",
              )}
              onClick={onShowRecipients}
              aria-label={t("showRecipientStatus")}
            >
              {renderRecipientStatus()}
            </button>
          )
        ) : null}

        {!obligationSummary && replyRequested ? (
          <span
            className={classNames(
              "rounded-full border border-violet-500/20 bg-violet-500/8 px-2.5 py-1 text-xs font-semibold tracking-tight",
              "text-violet-700 dark:text-violet-300",
            )}
          >
            {t("needReply")}
          </span>
        ) : null}
      </div>
    </div>
  );
}
