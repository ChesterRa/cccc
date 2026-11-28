import { useTranslation } from "react-i18next";

/** Withdraws a question as no longer needing an answer, in one click. */
export function DismissReplyButton({
  disabled,
  busy,
  onDismiss,
}: {
  disabled?: boolean;
  busy?: boolean;
  onDismiss: () => void;
}) {
  const { t } = useTranslation("chat");
  return (
    <button
      type="button"
      disabled={disabled || busy}
      onClick={onDismiss}
      className="shrink-0 rounded-md px-2 py-1 text-xs text-[var(--color-text-tertiary)] underline-offset-2 hover:text-[var(--color-text-secondary)] hover:underline disabled:opacity-50"
      title={t("pendingReply.dismissHint")}
      data-ask-dismiss
    >
      {t(busy ? "pendingReply.dismissing" : "pendingReply.dismiss")}
    </button>
  );
}
