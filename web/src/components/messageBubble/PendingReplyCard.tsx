import { useCallback, useContext, useEffect, useId, useRef, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui/button";
import { Textarea } from "../ui/textarea";
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
  answeredOption?: string | null;
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
  if (readOnly) return null;

  if (state !== "pending") {
    return (
      <div
        className="border-t border-[var(--glass-border-subtle)] pt-3 text-sm text-[var(--color-text-secondary)]"
        role="status"
      >
        <p>
          {state === "answered" && answeredOption
            ? t("pendingReply.answeredChoice", { choice: answeredOption })
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

  return (
    <form
      className="border-t border-[var(--glass-border-subtle)] pt-3"
      data-pending-reply
      onSubmit={submit}
    >
      {options.length > 0 && (
        <fieldset disabled={locked} className="min-w-0 space-y-2">
          <legend className="mb-2 text-sm font-semibold text-[var(--color-text-primary)]">
            {t("pendingReply.choose")}
          </legend>
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
                <span className="min-w-0 text-sm text-[var(--color-text-primary)]">
                  <span className="font-semibold">
                    {String.fromCharCode(65 + index)}. {option.label}
                  </span>
                  {recommended && (
                    <span className="ml-2 rounded bg-[var(--glass-tab-bg)] px-1.5 py-0.5 text-xs text-[var(--color-text-secondary)]">
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
        </fieldset>
      )}
      {other && (
        <div className={options.length ? "mt-3" : ""}>
          <label
            htmlFor={textId}
            className="mb-1 block text-sm font-medium text-[var(--color-text-primary)]"
          >
            {t("pendingReply.answer")}
          </label>
          <Textarea
            id={textId}
            rows={3}
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
      <div className="mt-3 flex justify-end">
        {uncertain ? (
          <Button type="button" disabled={sending} onClick={onRetry}>
            {t("pendingReply.retrySame")}
          </Button>
        ) : (
          <Button type="submit" disabled={!answer || sending || checking}>
            {t(sending ? "pendingReply.sending" : "pendingReply.send")}
          </Button>
        )}
      </div>
    </form>
  );
}
