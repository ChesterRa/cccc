import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useUiStylePrefs } from "../../utils/uiStyle";

/** A message this short reads at a glance and is never folded. */
const FOLD_OVER_CHARS = 140;

/**
 * The words of a Group message quoted in Chet's conversation. In the preview style a long
 * one folds to its first line, so Chet's own conclusion under it leads and several rounds
 * fit on a screen; the full text is one click away. Other styles show it whole.
 */
export function ChetGroupMessageText({ text }: { text: string }) {
  const { t } = useTranslation("modals");
  const { style } = useUiStylePrefs();
  const [open, setOpen] = useState(false);
  const body = "whitespace-pre-wrap break-words text-sm leading-6 text-[var(--color-text-primary)]";
  const trimmed = text.trim();
  const long = trimmed.includes("\n") || trimmed.length > FOLD_OVER_CHARS;
  if (style !== "preview" || !long) return <div className={`mt-0.5 ${body}`}>{text}</div>;
  const firstLine = trimmed.split("\n").find((line) => line.trim()) ?? "";
  return (
    <div className="mt-0.5 min-w-0" data-chet-group-text={open ? "open" : "folded"}>
      {open ? (
        <div className={body}>{text}</div>
      ) : (
        <p className="truncate text-sm leading-6 text-[var(--color-text-secondary)]">{firstLine}</p>
      )}
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="mt-0.5 rounded-sm text-xs text-[var(--color-text-tertiary)] underline-offset-2 hover:text-[var(--color-text-secondary)] hover:underline focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
      >
        {t(open ? "chet.groupMessageFold" : "chet.groupMessageUnfold")}
      </button>
    </div>
  );
}
