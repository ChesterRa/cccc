import { useLayoutEffect, useRef, type ComponentProps, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { LazyMarkdownRenderer } from "../../components/LazyMarkdownRenderer";
import { Button } from "../../components/ui/button";
import type { ChetTextRequest } from "../../services/api/chet";
import type { ProcessRecoveryOwner } from "../../services/api/processRecovery";
import { classNames } from "../../utils/classNames";
import { formatFullTime, formatMessageTimestamp } from "../../utils/time";
import type { VoiceConversationTurn } from "../codexVoice/codexVoiceProtocol";
import type { ChetRequestItem, ChetRequestState, ChetTimelineItem } from "./chetTimeline";
import { chetRefusalText } from "./chetText";
import type { ChetSendProblem } from "./useChetConversation";

/** What a request entry can do; each acts on that request alone. */
export type ChetRequestActions = {
  problems: ReadonlyMap<string, ChetSendProblem>;
  storageAvailable: boolean;
  /** Another unknown send exists, so a new one must wait. */
  sendAgainDisabled: boolean;
  onSendAgain(request: ChetTextRequest): void;
  onRetry(id: string): void;
  onPutBack(id: string): void;
  onDismiss(id: string): void;
  onReviewRecovery(owners: ProcessRecoveryOwner[]): void;
};

type DotState = "active" | "neutral" | "attention" | "failed" | "inactive";

const STATE_DOT: Record<ChetRequestState, DotState> = {
  sending: "active",
  send_unknown: "attention",
  send_conflict: "failed",
  preparing: "active",
  queued: "active",
  working: "active",
  checking: "inactive",
  completed: "neutral",
  failed: "failed",
  cancelled: "inactive",
  too_large: "attention",
  ended: "inactive",
  not_started: "attention",
  received_unconfirmed: "attention",
  unconfirmed: "attention",
};

const DOT_CLASS: Record<DotState, string> = {
  active: "bg-emerald-400",
  neutral: "bg-[var(--color-text-secondary)]",
  attention: "bg-amber-500",
  failed: "bg-rose-500",
  inactive: "bg-[var(--color-text-tertiary)]",
};

function Meta({ children }: { children: ReactNode }) {
  return (
    <div className="flex min-w-0 flex-wrap items-baseline gap-x-1.5 text-[11px] leading-5 text-[var(--color-text-muted)]">
      {children}
    </div>
  );
}

function Time({ value }: { value: string }) {
  if (!value) return null;
  return (
    <time dateTime={value} title={formatFullTime(value)} className="tabular-nums">
      {formatMessageTimestamp(value)}
    </time>
  );
}

/** What you said or typed, set off from the assistant's prose. */
function UserBlock({ meta, text, note }: { meta: ReactNode; text: string; note?: string }) {
  return (
    <div className="ml-auto w-fit min-w-0 max-w-[88%] rounded-md border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-2 sm:max-w-[80%]">
      <Meta>{meta}</Meta>
      <div className="mt-0.5 whitespace-pre-wrap break-words text-sm leading-6 text-[var(--color-text-primary)]">
        {text}
      </div>
      {note ? <p className="mt-1 text-[11px] text-amber-700 dark:text-amber-300">{note}</p> : null}
    </div>
  );
}

function ChetBlock({ meta, children }: { meta: ReactNode; children: ReactNode }) {
  return (
    <div className="min-w-0 border-l-2 border-[var(--color-text-secondary)] pl-3">
      <Meta>{meta}</Meta>
      <div className="mt-0.5 min-w-0 break-words text-sm leading-6 text-[var(--color-text-primary)]">
        {children}
      </div>
    </div>
  );
}

function ActionButton({ children, ...props }: ComponentProps<typeof Button>) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="sm"
      data-lcd-key
      className="!min-h-7 !px-2"
      {...props}
    >
      {children}
    </Button>
  );
}

function StateLine({ item, actions }: { item: ChetRequestItem; actions: ChetRequestActions }) {
  const { t } = useTranslation("modals");
  const state = item.state;
  const dot = STATE_DOT[state];
  const problem = actions.problems.get(item.id);
  const label =
    state === "not_started"
      ? t(
          item.notStartedReason === "cancelled"
            ? "chet.state.notStartedCancelled"
            : item.notStartedReason === "assistant_unavailable"
              ? "chet.state.notStartedUnavailable"
              : "chet.state.notStarted",
        )
      : state === "unconfirmed" && item.source === "voice"
        ? t("chet.state.voicePending")
        : state === "completed" && item.outcomeShownLater
          ? t("chet.state.completedLater")
          : t(`chet.state.${state}`);
  return (
    <div className="space-y-1">
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs text-[var(--color-text-muted)]">
        <span
          aria-hidden="true"
          data-status-dot={dot}
          data-status-motion={state === "preparing" || state === "working" ? "working" : undefined}
          className={classNames("h-2 w-2 flex-none rounded-full", DOT_CLASS[dot])}
        />
        <span
          className={classNames(
            "min-w-0 break-words",
            dot === "attention" && "text-amber-700 dark:text-amber-300",
            dot === "failed" && "text-rose-600 dark:text-rose-300",
          )}
          data-status-tone={
            dot === "attention" ? "warning" : dot === "failed" ? "error" : undefined
          }
        >
          {label}
        </span>
        {state === "not_started" && item.source === "text" ? (
          <ActionButton
            disabled={actions.sendAgainDisabled}
            title={t("chet.sendAgainHint")}
            onClick={() => actions.onSendAgain({ text: item.text, context: item.context })}
          >
            {t("chet.sendAgain")}
          </ActionButton>
        ) : null}
        {state === "send_unknown" ? (
          <ActionButton title={t("chet.retryHint")} onClick={() => actions.onRetry(item.id)}>
            {t("chet.retry")}
          </ActionButton>
        ) : null}
        {state === "send_unknown" || state === "send_conflict" ? (
          <>
            <ActionButton onClick={() => actions.onPutBack(item.id)}>
              {t("chet.putBack")}
            </ActionButton>
            <ActionButton title={t("chet.dismissHint")} onClick={() => actions.onDismiss(item.id)}>
              {t("chet.dismiss")}
            </ActionButton>
          </>
        ) : null}
      </div>
      {problem?.kind === "retry_refused" ? (
        <p
          className="flex flex-wrap items-center gap-2 pl-4 text-xs text-rose-600 dark:text-rose-300"
          data-status-tone="error"
        >
          <span>
            {t("chet.retryRefused", { error: chetRefusalText(t, problem.code, problem.message) })}
          </span>
          {problem.recoveryOwners.length ? (
            <ActionButton onClick={() => actions.onReviewRecovery(problem.recoveryOwners)}>
              {t("codexVoiceReviewRecoveryRecords")}
            </ActionButton>
          ) : null}
        </p>
      ) : null}
      {state === "send_unknown" && !actions.storageAvailable ? (
        <p className="pl-4 text-xs text-[var(--color-text-muted)]">{t("chet.notKeptHere")}</p>
      ) : null}
    </div>
  );
}

function RequestEntry({
  item,
  isDark,
  actions,
}: {
  item: ChetRequestItem;
  isDark: boolean;
  actions: ChetRequestActions;
}) {
  const { t } = useTranslation("modals");
  const outcome = item.outcome && !item.outcomeShownLater ? item.outcome : null;
  const answered = Boolean(outcome && (outcome.text.trim() || outcome.error.trim()));
  // A shown answer already says the request finished.
  const showState = item.state !== "completed" || !answered;
  return (
    <article className="space-y-2" data-chet-request={item.source} data-chet-state={item.state}>
      {item.sourceMissing ? (
        <p className="text-xs text-[var(--color-text-muted)]">{t("chet.sourceMissing")}</p>
      ) : item.source === "text" ? (
        <UserBlock
          text={item.text}
          meta={
            <>
              <span className="font-semibold text-[var(--color-text-primary)]">
                {t("chet.you")}
              </span>
              <Time value={item.createdAt} />
              <span>· {t("chet.sourceText")}</span>
              {item.context ? (
                <span className="max-w-full truncate" title={item.context.group_title}>
                  · {item.context.group_title}
                </span>
              ) : null}
            </>
          }
        />
      ) : (
        <div
          className="min-w-0 border-l border-dashed border-[var(--color-text-tertiary)] pl-3"
          title={t("chet.voiceDelegationHint")}
        >
          <Meta>
            <span className="font-semibold">{t("chet.voiceDelegation")}</span>
            <Time value={item.createdAt} />
          </Meta>
          <div className="mt-0.5 whitespace-pre-wrap break-words text-sm leading-6 text-[var(--color-text-secondary)]">
            {item.text}
          </div>
        </div>
      )}
      {outcome && answered ? (
        <ChetBlock
          meta={
            <>
              <span className="font-semibold text-[var(--color-text-primary)]">Chet</span>
              <Time value={item.resultAt} />
            </>
          }
        >
          {outcome.text.trim() ? (
            <LazyMarkdownRenderer
              content={outcome.text}
              isDark={isDark}
              className="text-sm leading-6"
              fallback={<div className="whitespace-pre-wrap">{outcome.text}</div>}
            />
          ) : null}
          {item.state === "failed" && outcome.error.trim() ? (
            <p
              className="mt-1 whitespace-pre-wrap text-xs text-rose-600 dark:text-rose-300"
              data-status-tone="error"
            >
              {outcome.error}
            </p>
          ) : null}
        </ChetBlock>
      ) : null}
      {showState ? <StateLine item={item} actions={actions} /> : null}
    </article>
  );
}

function TranscriptEntry({
  role,
  text,
  meta,
  note,
}: {
  role: "user" | "assistant";
  text: string;
  meta: ReactNode;
  note?: string;
}) {
  return role === "user" ? (
    <UserBlock meta={meta} text={text} note={note} />
  ) : (
    <ChetBlock meta={meta}>
      <div className="whitespace-pre-wrap">{text}</div>
      {note ? <p className="mt-1 text-[11px] text-amber-700 dark:text-amber-300">{note}</p> : null}
    </ChetBlock>
  );
}

/**
 * One timeline for typed requests, voice requests and transcripts, read like a document.
 * Follows new entries while the reader is at the end, and keeps the reading place when
 * earlier history loads above.
 */
export function ChetTimeline({
  items,
  liveTurns,
  loaded,
  hasOlder,
  loadingOlder,
  onLoadOlder,
  isDark,
  actions,
  notices,
}: {
  items: readonly ChetTimelineItem[];
  /** The current call's captions, until history holds them. */
  liveTurns: readonly VoiceConversationTurn[];
  loaded: boolean;
  hasOlder: boolean;
  loadingOlder: boolean;
  onLoadOlder(): void;
  isDark: boolean;
  actions: ChetRequestActions;
  notices?: ReactNode;
}) {
  const { t } = useTranslation("modals");
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const followRef = useRef(true);
  const anchorRef = useRef<{ height: number; top: number } | null>(null);
  const firstId = items[0]?.id;
  const contentKey = `${items.length}:${items[items.length - 1]?.id || ""}:${
    items[items.length - 1]?.kind === "request"
      ? (items[items.length - 1] as ChetRequestItem).state
      : ""
  }:${liveTurns.map((turn) => turn.text.length).join(",")}`;

  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    const anchor = anchorRef.current;
    if (anchor) {
      // Earlier history went in above: keep the entry the reader was looking at in place.
      element.scrollTop = anchor.top + (element.scrollHeight - anchor.height);
      anchorRef.current = null;
      return;
    }
    if (followRef.current) element.scrollTop = element.scrollHeight;
  }, [contentKey, firstId]);

  const loadOlder = () => {
    const element = scrollRef.current;
    if (element) anchorRef.current = { height: element.scrollHeight, top: element.scrollTop };
    onLoadOlder();
  };

  const empty = loaded && !items.length && !liveTurns.length;
  return (
    <div
      ref={scrollRef}
      className="min-h-0 flex-1 overflow-y-auto px-4 py-4 sm:px-6"
      data-chet-timeline
      onScroll={(event) => {
        const element = event.currentTarget;
        followRef.current = element.scrollHeight - element.clientHeight - element.scrollTop < 80;
      }}
    >
      <div className="mx-auto flex max-w-3xl flex-col gap-5">
        {notices}
        {hasOlder ? (
          <div className="flex justify-center">
            <Button
              type="button"
              variant="ghost"
              size="sm"
              data-lcd-key
              disabled={loadingOlder}
              onClick={loadOlder}
            >
              {t("chet.loadOlder")}
            </Button>
          </div>
        ) : null}
        {!loaded ? (
          <p className="py-10 text-center text-sm text-[var(--color-text-muted)]" role="status">
            {t("chet.loading")}
          </p>
        ) : null}
        {empty ? (
          <div className="py-12 text-center">
            <p className="text-base font-semibold text-[var(--color-text-primary)]">
              {t("chet.emptyTitle")}
            </p>
            <p className="mx-auto mt-2 max-w-md text-sm leading-6 text-balance text-[var(--color-text-muted)]">
              {t("chet.emptyHint")}
            </p>
          </div>
        ) : null}
        {items.map((item) =>
          item.kind === "request" ? (
            <RequestEntry key={item.id} item={item} isDark={isDark} actions={actions} />
          ) : (
            <TranscriptEntry
              key={item.id}
              role={item.role}
              text={item.text}
              note={
                !item.complete
                  ? t(item.final ? "chet.transcriptIncomplete" : "chet.transcriptUnfinished")
                  : undefined
              }
              meta={
                <>
                  <span className="font-semibold text-[var(--color-text-primary)]">
                    {item.role === "user" ? t("chet.you") : "Chet"}
                  </span>
                  <Time value={item.createdAt} />
                  <span>
                    · {t(item.role === "user" ? "chet.sourceTranscript" : "chet.sourceVoiceReply")}
                  </span>
                </>
              }
            />
          ),
        )}
        {liveTurns.map((turn) => (
          <TranscriptEntry
            key={`live:${turn.id}`}
            role={turn.role}
            text={turn.text}
            note={
              turn.save === "not_saved"
                ? t("chet.transcriptNotSaved")
                : turn.save === "unconfirmed"
                  ? t("chet.transcriptSaveUnconfirmed")
                  : undefined
            }
            meta={
              <>
                <span className="font-semibold text-[var(--color-text-primary)]">
                  {turn.role === "user" ? t("chet.you") : "Chet"}
                </span>
                <span>
                  · {t(turn.role === "user" ? "chet.sourceTranscript" : "chet.sourceVoiceReply")} ·{" "}
                  {t("chet.liveCall")}
                </span>
              </>
            }
          />
        ))}
      </div>
    </div>
  );
}
