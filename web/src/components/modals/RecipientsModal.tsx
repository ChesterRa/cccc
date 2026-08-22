import { useTranslation } from "react-i18next";
import { classNames } from "../../utils/classNames";
import { useModalA11y } from "../../hooks/useModalA11y";
import { ModalFrame } from "./ModalFrame";

export interface RecipientEntry {
  id: string;
  cleared: boolean;
  deliveryState: string;
  read: boolean;
  replied: boolean;
  replyRequested: boolean;
  cancelled: boolean;
}

export interface RecipientsModalProps {
  isOpen: boolean;
  isDark: boolean;
  toLabel: string;
  statusKind: "delivery" | "read" | "reply";
  entries: RecipientEntry[];
  messageMode: "send" | "request_reply" | "mail";
  busyAction: string;
  canCancelReply: boolean;
  onDeliver: (actorId: string, forceAmbiguous: boolean) => void;
  onCancelReply: () => void;
  onClose: () => void;
}

export function RecipientsModal({
  isOpen,
  isDark,
  toLabel,
  statusKind,
  entries,
  messageMode,
  busyAction,
  canCancelReply,
  onDeliver,
  onCancelReply,
  onClose,
}: RecipientsModalProps) {
  const { t } = useTranslation("modals");
  const { modalRef } = useModalA11y(isOpen, onClose);
  if (!isOpen) return null;

  const isReply = statusKind === "reply";
  const isRead = statusKind === "read";
  const titleText = isReply
    ? t("recipients.replyStatus")
    : isRead
      ? t("recipients.recipients")
      : t("recipients.deliveryStatus");

  const deliveryLabel = (entry: RecipientEntry): string => {
    if (entry.deliveryState === "accepted") return t("recipients.deliveryAccepted");
    if (entry.deliveryState === "claimed") return t("recipients.deliveryClaimed");
    if (entry.deliveryState === "failed") return t("recipients.deliveryFailed");
    if (entry.deliveryState === "ambiguous") return t("recipients.deliveryAmbiguous");
    if (messageMode === "mail") return t("recipients.inInbox");
    return t("recipients.notDelivered");
  };

  const deliverLabel = (entry: RecipientEntry): string => {
    if (entry.deliveryState === "failed") return t("recipients.retry");
    if (entry.deliveryState === "ambiguous") return t("recipients.retryAnyway");
    return t("recipients.sendNow");
  };

  const titleContent = (
    <div className="min-w-0 pr-2">
      <div className="text-sm font-semibold truncate text-[var(--color-text-primary)]">
        {titleText}
      </div>
      <div
        className="text-[11px] truncate text-[var(--color-text-muted)] mt-0.5"
        title={t("recipients.toLabel", { label: toLabel })}
      >
        {t("recipients.toLabel", { label: toLabel })}
      </div>
    </div>
  );

  return (
    <ModalFrame
      isOpen={isOpen}
      isDark={isDark}
      onClose={onClose}
      titleId="recipients-title"
      title={titleContent}
      closeAriaLabel={t("common:close")}
      panelClassName="w-full max-w-md max-h-[80vh] sm:max-h-[calc(100dvh-8rem)]"
      modalRef={modalRef}
    >
      <div className="p-4 sm:p-5 overflow-auto flex-1 min-h-0">
        {entries.length > 0 ? (
          <div className="rounded-xl border divide-y border-[var(--glass-border-subtle)] divide-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)]">
            {entries.map((entry) => {
              const canDeliver =
                entry.id !== "user" &&
                (messageMode !== "mail" || !entry.read) &&
                !entry.replied &&
                !entry.cancelled &&
                !["accepted", "claimed"].includes(entry.deliveryState);
              const actionKey = `deliver:${entry.id}`;
              return (
                <div key={entry.id} className="flex items-center justify-between gap-3 px-4 py-3">
                  <div className="min-w-0">
                    <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">
                      {entry.id}
                    </div>
                    <div className="mt-0.5 text-[11px] text-[var(--color-text-muted)]">
                      {deliveryLabel(entry)}
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    {canDeliver ? (
                      <button
                        type="button"
                        className="rounded-lg border border-[var(--glass-border-subtle)] px-2.5 py-1.5 text-xs font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--glass-tab-bg-hover)] disabled:opacity-50"
                        disabled={Boolean(busyAction)}
                        onClick={() => {
                          const force = entry.deliveryState === "ambiguous";
                          if (force && !window.confirm(t("recipients.retryAmbiguousConfirm"))) {
                            return;
                          }
                          onDeliver(entry.id, force);
                        }}
                      >
                        {busyAction === actionKey ? t("recipients.sending") : deliverLabel(entry)}
                      </button>
                    ) : null}
                    <div
                      className={classNames(
                        "text-sm font-semibold tracking-tight",
                        entry.cleared
                          ? "text-emerald-600 dark:text-emerald-400"
                          : "text-[var(--color-text-muted)]",
                      )}
                      aria-label={
                        entry.cleared
                          ? isReply
                            ? "replied"
                            : isRead
                              ? "read"
                              : "delivered"
                          : "pending"
                      }
                    >
                      {isReply
                        ? entry.cleared
                          ? "↩"
                          : "○"
                        : isRead
                          ? entry.cleared
                            ? "✓✓"
                            : "✓"
                          : entry.cleared
                            ? "✓"
                            : "○"}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        ) : (
          <div className="text-sm py-6 text-center text-[var(--color-text-muted)]">
            {t("recipients.noTracking")}
          </div>
        )}

        <div className="text-[11px] mt-3 text-[var(--color-text-muted)]">
          {isReply
            ? t("recipients.legendReply")
            : isRead
              ? t("recipients.legendRead")
              : t("recipients.legendDelivery")}
        </div>
        {canCancelReply ? (
          <div className="mt-4 border-t border-[var(--glass-border-subtle)] pt-4">
            <button
              type="button"
              className="rounded-lg px-3 py-2 text-sm font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--glass-tab-bg-hover)] disabled:opacity-50"
              disabled={Boolean(busyAction)}
              onClick={() => {
                if (window.confirm(t("recipients.cancelReplyConfirm"))) onCancelReply();
              }}
            >
              {busyAction === "cancel-reply"
                ? t("recipients.cancelling")
                : t("recipients.cancelReplyRequest")}
            </button>
          </div>
        ) : null}
      </div>
    </ModalFrame>
  );
}
