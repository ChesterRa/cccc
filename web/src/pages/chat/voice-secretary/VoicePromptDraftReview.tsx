import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../../components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../../../components/ui/popover";
import { copyTextToClipboard } from "../../../utils/copy";

/** A changed composer keeps its text; adopting the retained draft is an explicit action. */
export function VoicePromptDraftReview({
  text,
  replace,
  onApply,
  onDismiss,
  onReturnToComposer,
}: {
  text: string;
  replace: boolean;
  onApply: () => Promise<void>;
  onDismiss: () => Promise<void>;
  onReturnToComposer?: () => void;
}) {
  const { t } = useTranslation("chat");
  const [busy, setBusy] = useState(false);
  const [copyStatus, setCopyStatus] = useState("");
  const acting = useRef(false);
  const act = async (action: () => Promise<void>) => {
    if (busy) return;
    acting.current = true;
    setBusy(true);
    try {
      await action();
    } finally {
      setBusy(false);
    }
  };
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          data-voice-prompt-review
          className="shrink-0 rounded px-1 font-semibold underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
        >
          {t("voiceSecretaryPromptDraftReview")}
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align="start"
        collisionPadding={12}
        aria-label={t("voiceSecretaryPromptDraftReview")}
        onCloseAutoFocus={(event) => {
          // Applying/dismissing removes the trigger; return to the retained composer instead.
          if (acting.current && onReturnToComposer) {
            event.preventDefault();
            onReturnToComposer();
          }
        }}
        className="flex w-[min(32rem,calc(100vw-24px))] max-h-[min(32rem,var(--radix-popover-content-available-height))] flex-col gap-3 overflow-y-auto p-4"
      >
        <p className="text-sm text-[var(--color-text-secondary)]">
          {t("voiceSecretaryPromptDraftChanged")}
        </p>
        <textarea
          readOnly
          aria-label={t("voiceSecretaryPromptDraftReadyShort")}
          value={text}
          className="min-h-28 w-full shrink-0 resize-y rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] p-3 text-sm leading-6"
          rows={6}
        />
        <div className="flex flex-wrap items-center justify-end gap-2">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={busy}
            onClick={() => void act(onDismiss)}
          >
            {t("voiceSecretaryPromptDraftDismiss")}
          </Button>
          <Button
            type="button"
            variant="secondary"
            size="sm"
            onClick={async () => {
              setCopyStatus(
                (await copyTextToClipboard(text))
                  ? t("common:copied")
                  : t("voiceSecretaryReplyCopyFailed"),
              );
            }}
          >
            {t("common:copy")}
          </Button>
          <Button type="button" size="sm" disabled={busy} onClick={() => void act(onApply)}>
            {t(replace ? "voiceSecretaryPromptDraftReplace" : "voiceSecretaryPromptDraftAppend")}
          </Button>
        </div>
        {copyStatus ? (
          <p role="status" className="text-xs text-[var(--color-text-secondary)]">
            {copyStatus}
          </p>
        ) : null}
      </PopoverContent>
    </Popover>
  );
}
