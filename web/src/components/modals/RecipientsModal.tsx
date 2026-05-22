import { useTranslation } from "react-i18next";
import { classNames } from "../../utils/classNames";
import { useModalA11y } from "../../hooks/useModalA11y";
import { ModalFrame } from "./ModalFrame";

export type RecipientEntry = readonly [string, boolean];

export interface RecipientsModalProps {
  isOpen: boolean;
  isDark: boolean;
  toLabel: string;
  statusKind: "read" | "ack" | "reply";
  entries: RecipientEntry[];
  onClose: () => void;
}

export function RecipientsModal({
  isOpen,
  isDark,
  toLabel,
  statusKind,
  entries,
  onClose,
}: RecipientsModalProps) {
  const { t } = useTranslation("modals");
  const { modalRef } = useModalA11y(isOpen, onClose);
  if (!isOpen) return null;

  const isAck = statusKind === "ack";
  const isReply = statusKind === "reply";
  const titleText = isReply ? t("recipients.replyStatus") : isAck ? t("recipients.acknowledgements") : t("recipients.recipients");

  const titleContent = (
    <div className="min-w-0 pr-2">
      <div id="recipients-title" className="text-sm font-semibold truncate text-[var(--color-text-primary)]">
        {titleText}
      </div>
      <div className="text-[11px] truncate text-[var(--color-text-muted)] mt-0.5" title={t("recipients.toLabel", { label: toLabel })}>
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
            {entries.map(([id, cleared]) => (
              <div key={id} className="flex items-center justify-between gap-3 px-4 py-3">
                <div className="text-sm font-medium truncate text-[var(--color-text-primary)]">{id}</div>
                <div
                  className={classNames(
                    "text-sm font-semibold tracking-tight",
                    cleared ? "text-emerald-600 dark:text-emerald-400" : "text-[var(--color-text-muted)]"
                  )}
                  aria-label={cleared ? (isReply ? "replied" : isAck ? "acknowledged" : "read") : "pending"}
                >
                  {isReply ? (cleared ? "↩" : "○") : isAck ? (cleared ? "✓" : "○") : cleared ? "✓✓" : "✓"}
                </div>
              </div>
            ))}
          </div>
        ) : (
          <div className="text-sm py-6 text-center text-[var(--color-text-muted)]">{t("recipients.noTracking")}</div>
        )}

        <div className="text-[11px] mt-3 text-[var(--color-text-muted)]">
          {isReply
            ? t("recipients.legendReply")
            : isAck
              ? t("recipients.legendAck")
              : t("recipients.legendRead")}
        </div>
      </div>
    </ModalFrame>
  );
}
