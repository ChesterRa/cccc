import {
  useCallback,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  type FormEvent,
} from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui/button";
import { Textarea } from "../ui/textarea";
import { SendIcon } from "../Icons";
import { LcdSpinner } from "../LcdSpinner";
import { classNames } from "../../utils/classNames";
import { PendingReplyEditingContext } from "./PendingReplyEditingContext";
import type {
  ReplyAnswer,
  ReplyDraft,
  ReplyQuestion,
  ReplyState,
} from "../../features/pendingReplies/types";
import { MAX_REPLY_SCALARS } from "../../features/pendingReplies/types";

/** A controlled draft survives a virtualized message row being unmounted. */
export function PendingReplyCard({
  question,
  state,
  readOnly,
  sending,
  uncertain,
  checking,
  error,
  showRetainedDraft,
  answeredOption,
  quiet,
  draft,
  onDraftChange,
  onSubmit,
  onRetry,
  onUseComposer,
}: {
  question?: ReplyQuestion | null;
  state: ReplyState;
  readOnly?: boolean;
  sending?: boolean;
  uncertain?: boolean;
  checking?: boolean;
  error?: string;
  showRetainedDraft?: boolean;
  answeredOption?: { id: string; label: string } | null;
  /** 'Waiting for you' indicators are off: the question without their emphasis. */
  quiet?: boolean;
  draft: ReplyDraft;
  onDraftChange: (draft: ReplyDraft) => void;
  onSubmit: (answer: ReplyAnswer) => void;
  onRetry?: () => void;
  onUseComposer?: () => void;
}) {
  const { t } = useTranslation("chat");
  const groupName = useId();
  const textId = useId();
  const onEditingChange = useContext(PendingReplyEditingContext);
  const editing = useRef(false);
  const options = question?.options ?? [];
  const other = options.length === 0 || draft.selection?.kind === "other";
  const locked = !!sending || !!uncertain;
  const selectedId = draft.selection?.kind === "option" ? draft.selection.id : null;
  const editorVisible = !readOnly && state === "pending" && other && !locked;
  const releaseEditor = useCallback(() => {
    if (!editing.current) return;
    editing.current = false;
    onEditingChange?.(textId, false);
  }, [onEditingChange, textId]);
  useEffect(() => () => releaseEditor(), [releaseEditor]);
  useEffect(() => {
    if (!editorVisible) releaseEditor();
  }, [editorVisible, releaseEditor]);
  const editorRef = useRef<HTMLTextAreaElement | null>(null);
  // The answer box grows with its text from its starting rows up to its max-height,
  // then scrolls inside, so a long answer stays readable while it is written.
  const fitEditor = useCallback(() => {
    const node = editorRef.current;
    if (!node) return;
    node.style.height = "auto";
    const style = window.getComputedStyle(node);
    const borders =
      (Number.parseFloat(style.borderTopWidth) || 0) +
      (Number.parseFloat(style.borderBottomWidth) || 0);
    const natural = node.scrollHeight + borders;
    const max = Number.parseFloat(style.maxHeight);
    node.style.height = `${Number.isFinite(max) ? Math.min(natural, max) : natural}px`;
  }, []);
  useLayoutEffect(fitEditor, [fitEditor, draft.text, other, state]);
  // A narrower box wraps the same text onto more lines (a turned phone, a side panel),
  // so it fits again when its width changes, and once the layout and fonts settle after
  // it appears; its own height changes are ignored.
  useEffect(() => {
    const node = editorRef.current;
    if (!node) return;
    let cancelled = false;
    void document.fonts?.ready.then(() => {
      if (!cancelled) fitEditor();
    });
    if (typeof ResizeObserver === "undefined") {
      return () => {
        cancelled = true;
      };
    }
    let width = -1;
    let frame = 0;
    // Fit on the next frame: resizing the observed box inside its own callback leaves
    // WebKit with an undelivered notification (a "loop" error).
    const observer = new ResizeObserver(() => {
      if (node.clientWidth === width) return;
      width = node.clientWidth;
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(fitEditor);
    });
    observer.observe(node);
    return () => {
      cancelled = true;
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [fitEditor, other, state]);
  if (readOnly) return null;

  if (state !== "pending") {
    return (
      <div
        className="border-t border-[var(--glass-border-subtle)] pt-3 text-sm text-[var(--color-text-secondary)]"
        role="status"
        data-pending-reply-closed
      >
        <p>
          {state === "answered" && answeredOption
            ? t("pendingReply.answeredChoice", {
                // The options stay listed above, so the letter is enough.
                choice: answeredOption.id,
              })
            : t(`pendingReply.${state}`)}
        </p>
        {showRetainedDraft && (draft.text || draft.selection) && (
          <div className="mt-2 whitespace-pre-wrap break-words text-[var(--color-text-primary)]">
            <span className="font-medium">{t("pendingReply.retainedDraft")}</span>
            {selectedId && (
              <p>
                {question?.options.find((option) => option.id === selectedId)?.label ?? selectedId}
              </p>
            )}
            {draft.text && <p>{draft.text}</p>}
          </div>
        )}
        {state === "unsupported" && onUseComposer && (
          <Button type="button" variant="secondary" className="mt-2" onClick={onUseComposer}>
            {t("pendingReply.openComposer")}
          </Button>
        )}
      </div>
    );
  }

  const textLength = Array.from(draft.text).length;
  const tooLong = other && textLength > MAX_REPLY_SCALARS;
  // Rust's trim uses Unicode White_Space, which differs from JavaScript's
  // trim for a few characters such as NEL and the byte-order mark.
  const hasText = /\P{White_Space}/u.test(draft.text);
  const answer: ReplyAnswer | null = other
    ? hasText && !tooLong
      ? { kind: "other", text: draft.text }
      : null
    : selectedId !== null && options.some((option) => option.id === selectedId)
      ? { kind: "option", id: selectedId }
      : null;
  const submit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (answer && !locked && !checking) onSubmit(answer);
  };

  const sendLabel = t(sending ? "pendingReply.sending" : "pendingReply.send");
  const action = uncertain ? (
    <Button type="button" disabled={sending} onClick={onRetry}>
      {t("pendingReply.retrySame")}
    </Button>
  ) : (
    // Beside a narrow answer box the key keeps only its send mark (lcd.css); the words
    // stay its name and tooltip.
    <Button
      type="submit"
      disabled={!answer || sending || checking}
      data-ask-send
      aria-label={sendLabel}
      title={sendLabel}
    >
      <span data-ask-send-mark aria-hidden="true">
        {sending ? (
          <LcdSpinner className="h-4 w-4">
            <SendIcon size={16} strokeWidth={1.25} />
          </LcdSpinner>
        ) : (
          <SendIcon size={16} strokeWidth={1.25} />
        )}
      </span>
      <span data-ask-send-label>{sendLabel}</span>
    </Button>
  );

  const otherChoice = (
    <label
      className={classNames(
        "flex min-h-[44px] cursor-pointer items-center gap-3 rounded-lg border px-3 py-2.5 text-sm",
        draft.selection?.kind === "other"
          ? "border-[var(--color-border-focus)] bg-[var(--glass-tab-bg)]"
          : "border-[var(--glass-border-subtle)]",
      )}
    >
      <input
        type="radio"
        name={groupName}
        disabled={locked}
        checked={draft.selection?.kind === "other"}
        onChange={() => onDraftChange({ ...draft, selection: { kind: "other" } })}
        className="h-4 w-4 shrink-0 accent-[var(--primary)]"
      />
      {t("pendingReply.other")}
    </label>
  );

  // Each radio is disabled on its own, so the fieldset never disables the action
  // placed in its last row.
  const choices = options.length > 0 && (
    <fieldset className="min-w-0 space-y-2">
      <legend className="sr-only">{t("pendingReply.choose")}</legend>
      {options.map((option, index) => {
        const selected = draft.selection?.kind === "option" && draft.selection.id === option.id;
        const recommended = question?.recommended?.option_id === option.id;
        return (
          <label
            key={option.id}
            className={classNames(
              "flex min-h-[44px] cursor-pointer items-start gap-3 rounded-lg border px-3 py-2.5",
              selected
                ? "border-[var(--color-border-focus)] bg-[var(--glass-tab-bg)]"
                : "border-[var(--glass-border-subtle)]",
            )}
          >
            <input
              type="radio"
              name={groupName}
              disabled={locked}
              checked={selected}
              onChange={() =>
                onDraftChange({ ...draft, selection: { kind: "option", id: option.id } })
              }
              className="mt-1 h-4 w-4 shrink-0 accent-[var(--primary)]"
            />
            <span className="min-w-0 break-words text-sm text-[var(--color-text-primary)]">
              <span className="font-semibold">
                {String.fromCharCode(65 + index)}. {option.label}
              </span>
              {recommended && (
                <span
                  className="ml-2 rounded bg-[var(--glass-tab-bg)] px-1.5 py-0.5 text-xs text-[var(--color-text-secondary)]"
                  data-recommended-tag
                >
                  {t("pendingReply.recommended")}
                </span>
              )}
              {option.consequence && (
                <span className="mt-1 block whitespace-pre-wrap text-[var(--color-text-secondary)]">
                  {option.consequence}
                </span>
              )}
              {recommended && question?.recommended?.reason && (
                <span className="mt-1 block whitespace-pre-wrap text-xs text-[var(--color-text-tertiary)]">
                  {t("pendingReply.reason", { reason: question.recommended.reason })}
                </span>
              )}
            </span>
          </label>
        );
      })}
      {!other ? (
        // The action shares the last row and wraps below it when the row is short.
        <div className="lcd-ask-other">
          {otherChoice}
          <div className="lcd-ask-action">{action}</div>
        </div>
      ) : (
        otherChoice
      )}
    </fieldset>
  );

  const editor = (
    <Textarea
      ref={editorRef}
      id={textId}
      rows={2}
      className="max-h-[min(12rem,40dvh)] resize-none"
      value={draft.text}
      disabled={locked}
      onFocus={() => {
        editing.current = true;
        onEditingChange?.(textId, true);
      }}
      onBlur={releaseEditor}
      onChange={(event) => onDraftChange({ ...draft, text: event.target.value })}
      placeholder={t("pendingReply.placeholder")}
    />
  );

  return (
    <form
      className="border-t border-[var(--glass-border-subtle)] pt-3"
      data-pending-reply
      data-pending-reply-quiet={quiet || undefined}
      onSubmit={submit}
    >
      {!quiet ? (
        // A small title set into the frame, in the product's existing words.
        // The button says when the answer is being sent; the title does not repeat it.
        <p className="lcd-ask-title">{t("filterWaitingForYou")}</p>
      ) : null}
      {/* The action sits beside "Other" (above) or beside the answer box. */}
      {choices}
      {other && (
        <div className="lcd-ask-answer">
          <label htmlFor={textId} className="sr-only">
            {t("pendingReply.answer")}
          </label>
          {editor}
          {action}
        </div>
      )}
      {error && (
        <p className="mt-2 whitespace-pre-wrap text-sm text-[var(--destructive)]" role="alert">
          {error}
        </p>
      )}
      {checking && !uncertain && (
        <p className="mt-2 text-xs text-[var(--color-text-secondary)]" role="status">
          {t("pendingReply.checking")}
        </p>
      )}
      {tooLong && (
        <p className="mt-2 text-sm text-[var(--destructive)]" role="alert">
          {t("pendingReply.tooLong", { count: textLength, limit: MAX_REPLY_SCALARS })}
        </p>
      )}
    </form>
  );
}
