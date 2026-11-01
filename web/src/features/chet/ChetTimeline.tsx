import { useLayoutEffect, useRef, useState, type ComponentProps, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDownIcon } from "../../components/Icons";
import { LazyMarkdownRenderer } from "../../components/LazyMarkdownRenderer";
import { Button } from "../../components/ui/button";
import type { ChetTextRequest } from "../../services/api/chet";
import type { ProcessRecoveryOwner } from "../../services/api/processRecovery";
import { classNames } from "../../utils/classNames";
import { formatFullTime, formatMessageTimestamp } from "../../utils/time";
import type { VoiceConversationTurn } from "../codexVoice/codexVoiceProtocol";
import type {
  ChetRequestItem,
  ChetRequestState,
  ChetTimelineItem,
  ChetTranscriptItem,
} from "./chetTimeline";
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

function Chevron({ expanded }: { expanded: boolean }) {
  return (
    <ChevronDownIcon
      size={12}
      aria-hidden="true"
      className={classNames("flex-none self-center", !expanded && "-rotate-90")}
    />
  );
}

/** What Chet noted to hand off during a call: its wording, not the user's. */
function VoiceDelegationNote({ item }: { item: ChetRequestItem }) {
  const { t } = useTranslation("modals");
  return (
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
  const [delegationShown, setDelegationShown] = useState(false);
  const outcome = item.outcome && !item.outcomeShownLater ? item.outcome : null;
  const answered = Boolean(outcome && (outcome.text.trim() || outcome.error.trim()));
  // The answer stands for an answered voice request; Chet's wording of it stays one click away.
  const folded = item.source === "voice" && answered;
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
      ) : folded ? null : (
        <VoiceDelegationNote item={item} />
      )}
      {outcome && answered ? (
        <ChetBlock
          meta={
            <>
              <span className="font-semibold text-[var(--color-text-primary)]">Chet</span>
              <Time value={item.resultAt} />
              {folded ? (
                <button
                  type="button"
                  aria-expanded={delegationShown}
                  title={t("chet.voiceDelegationHint")}
                  onClick={() => setDelegationShown((shown) => !shown)}
                  className="inline-flex items-baseline gap-0.5 rounded-sm hover:text-[var(--color-text-secondary)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
                >
                  <Chevron expanded={delegationShown} />
                  {t("chet.voiceDelegation")}
                </button>
              ) : null}
            </>
          }
        >
          {folded && delegationShown ? (
            <div className="mb-2 whitespace-pre-wrap break-words border-l border-dashed border-[var(--color-text-tertiary)] pl-3 text-sm leading-6 text-[var(--color-text-secondary)]">
              {item.text}
            </div>
          ) : null}
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
 * Chet's saved voice turns between other entries. What it said is secondary to its answers,
 * so the run is folded to its last turn's opening until the reader opens it.
 */
function VoiceRun({
  turns,
  expanded,
  onToggle,
}: {
  turns: readonly ChetTranscriptItem[];
  expanded: boolean;
  onToggle(): void;
}) {
  const { t } = useTranslation("modals");
  const last = turns[turns.length - 1];
  // Folded, the run still says when part of its transcript is missing.
  const gap = turns.some((turn) => !turn.final)
    ? "chet.transcriptUnfinished"
    : turns.some((turn) => !turn.complete)
      ? "chet.transcriptIncomplete"
      : null;
  return (
    <div
      className="min-w-0 border-l-2 border-[var(--color-text-tertiary)] pl-3"
      data-chet-voice-run
    >
      <button
        type="button"
        aria-expanded={expanded}
        title={t("chet.voiceRunHint")}
        onClick={onToggle}
        className="flex w-full min-w-0 flex-wrap items-center gap-x-1.5 rounded-sm text-left text-xs leading-5 text-[var(--color-text-muted)] hover:text-[var(--color-text-secondary)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
      >
        <Chevron expanded={expanded} />
        <span className="font-semibold">Chet</span>
        <span>
          · {t("chet.sourceTranscript")}
          {turns.length > 1 ? ` · ${t("chet.voiceSegments", { count: turns.length })}` : ""}
        </span>
        <Time value={last.createdAt} />
        {/* Shares the line while there is room; on a narrow screen it takes its own line. */}
        {expanded ? null : <span className="min-w-24 flex-1 basis-0 truncate">{last.text}</span>}
      </button>
      {expanded || !gap ? null : (
        <p className="text-[11px] leading-5 text-amber-700 dark:text-amber-300">{t(gap)}</p>
      )}
      {expanded ? (
        <div className="mt-1 space-y-2">
          {turns.map((turn) => (
            <div key={turn.id} className="min-w-0">
              <div className="whitespace-pre-wrap break-words text-sm leading-6 text-[var(--color-text-secondary)]">
                {turn.text}
              </div>
              {turn.complete ? null : (
                <p className="mt-1 text-[11px] text-amber-700 dark:text-amber-300">
                  {t(turn.final ? "chet.transcriptIncomplete" : "chet.transcriptUnfinished")}
                </p>
              )}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

type TimelineRow =
  | { kind: "entry"; item: ChetRequestItem | ChetTranscriptItem }
  | { kind: "voice"; turns: ChetTranscriptItem[] };

/** Groups each run of Chet's adjacent voice turns from one call into a single row. */
function timelineRows(items: readonly ChetTimelineItem[]): TimelineRow[] {
  const rows: TimelineRow[] = [];
  for (const item of items) {
    const previous = rows[rows.length - 1];
    if (item.kind !== "transcript" || item.role !== "assistant") {
      rows.push({ kind: "entry", item });
    } else if (previous?.kind === "voice" && previous.turns[0].callId === item.callId) {
      previous.turns.push(item);
    } else {
      rows.push({ kind: "voice", turns: [item] });
    }
  }
  return rows;
}

/**
 * One timeline for typed requests, voice requests and transcripts, read like a document.
 * Follows new entries while the reader is at the end, and keeps the reading place when
 * earlier history loads above.
 */
export function ChetTimeline({
  items,
  liveTurns,
  liveCallId = null,
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
  /** The call in progress, whose latest run of Chet's turns stays open: its audio may still play. */
  liveCallId?: string | null;
  loaded: boolean;
  hasOlder: boolean;
  loadingOlder: boolean;
  onLoadOlder(): void;
  isDark: boolean;
  actions: ChetRequestActions;
  notices?: ReactNode;
}) {
  const { t } = useTranslation("modals");
  // The reader's choice, kept by turn so it holds as a run grows or earlier history joins it.
  const [turnChoices, setTurnChoices] = useState<ReadonlyMap<string, boolean>>(() => new Map());
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
  const rows = timelineRows(items);
  // A turn's transcript can be saved after a later one, so the latest run is the one holding
  // the call's highest turn, not the last row.
  let speakingRow = -1;
  let latestSeq = -Infinity;
  rows.forEach((row, index) => {
    if (!liveCallId || row.kind !== "voice" || row.turns[0].callId !== liveCallId) return;
    for (const turn of row.turns) {
      if (turn.seq >= latestSeq) {
        latestSeq = turn.seq;
        speakingRow = index;
      }
    }
  });
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
        {rows.map((row, index) => {
          if (row.kind === "voice") {
            let chosen: boolean | undefined;
            for (const turn of row.turns) chosen = turnChoices.get(turn.id) ?? chosen;
            const expanded = chosen ?? index === speakingRow;
            const toggle = () =>
              setTurnChoices((previous) => {
                const next = new Map(previous);
                for (const turn of row.turns) next.set(turn.id, !expanded);
                return next;
              });
            return (
              <VoiceRun
                key={row.turns[0].id}
                turns={row.turns}
                expanded={expanded}
                onToggle={toggle}
              />
            );
          }
          const item = row.item;
          return item.kind === "request" ? (
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
          );
        })}
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
