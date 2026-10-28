import { useContext } from "react";
import { useTranslation } from "react-i18next";
import { useStore } from "zustand";
import { PendingSummaryContext } from "../../features/pendingReplies/context";
import {
  matchesSubmittedAnswer,
  pendingReplyView,
  recordedOption,
} from "../../features/pendingReplies/model";
import {
  EMPTY_REPLY_ENTRY,
  PendingReplyStoreContext,
  replyKey,
  type PendingReplyStoreApi,
} from "../../features/pendingReplies/store";
import type { ReplyAnswer } from "../../features/pendingReplies/types";
import { answerPendingReply, type PendingReplyAnswer } from "../../services/api/pendingReplies";
import { useGroupStore } from "../../stores";
import type { ChatMessageData, LedgerEvent } from "../../types";
import { PendingReplyCard } from "./PendingReplyCard";

function newClientId(): string {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function ReplyEntry({
  store,
  groupId,
  event,
  onUseComposer,
  checking,
}: {
  store: PendingReplyStoreApi;
  groupId: string;
  event: LedgerEvent;
  onUseComposer: () => void;
  checking?: boolean;
}) {
  const { t } = useTranslation("chat");
  const summary = useContext(PendingSummaryContext);
  const eventId = event.id!;
  const key = replyKey(groupId, eventId);
  const entry = useStore(store, (state) => state.entries[key] ?? EMPTY_REPLY_ENTRY);
  const view = pendingReplyView(event)!;
  const state = entry.closed ?? (entry.attempt ? "pending" : view.state);
  const replyId = event._user_request?.reply_event_id;
  const loadedReply = useGroupStore((groupState) =>
    replyId
      ? groupState.chatByGroup[groupId]?.events.find((item) => item.id === replyId)
      : undefined,
  );
  const localOptionId =
    entry.confirmed && entry.draft.selection?.kind === "option" ? entry.draft.selection.id : null;
  const localOption = view.question?.options.find((option) => option.id === localOptionId);
  const answeredOption =
    state === "answered"
      ? localOption
        ? `${localOption.id}. ${localOption.label}`
        : recordedOption(view.question, eventId, event.by ?? "", loadedReply)
      : null;

  const send = async (answer: PendingReplyAnswer) => {
    const previous = store.getState().entries[key]?.attempt;
    let attempt: ReturnType<ReturnType<PendingReplyStoreApi["getState"]>["begin"]>;
    try {
      attempt = store.getState().begin(key, answer, newClientId, view.question);
    } catch {
      store.getState().setError(key, "idUnavailable");
      return;
    }
    if (!attempt) return;
    const response = await answerPendingReply(
      groupId,
      eventId,
      attempt.clientId,
      attempt.answer,
    ).catch(() => null);
    // The authenticated scope can disappear while the request is in flight.
    if (store.getState().entries[key]?.attempt?.clientId !== attempt.clientId) return;
    const result = response?.ok ? response.result?.event : undefined;
    const data = result?.data as ChatMessageData | undefined;
    if (
      result?.id &&
      result.kind === "chat.message" &&
      result.by === "user" &&
      result.group_id === groupId &&
      data?.reply_to === eventId &&
      Array.isArray(data.to) &&
      data.to.length === 1 &&
      data.to[0] === event.by &&
      data.client_id === attempt.clientId &&
      matchesSubmittedAnswer(attempt.question, attempt.answer, result)
    ) {
      const groups = useGroupStore.getState();
      groups.appendEvent(result, groupId);
      groups.updateObligationStatus(eventId, { reply: result }, groupId);
      store.getState().complete(key, attempt.clientId);
      summary?.refresh();
      return;
    }
    const code = response && !response.ok ? String(response.error?.code || "").toLowerCase() : "";
    const details =
      response && !response.ok
        ? (response.error?.details as { state?: unknown } | undefined)
        : undefined;
    if (
      code === "reply_request_closed" &&
      (details?.state === "answered" || details?.state === "cancelled")
    ) {
      store
        .getState()
        .close(key, attempt.clientId, details.state === "answered" ? "answered" : "withdrawn");
      summary?.refresh();
      return;
    }
    // Only errors after the daemon's strict idempotency lookup prove that a
    // previous uncertain attempt did not write an answer.
    const afterLookup = [
      "invalid_source_event",
      "requester_unavailable",
      "requester_unknown",
      "unsupported_request_source",
      "question_unavailable",
    ].includes(code);
    const definite = [
      "invalid_answer",
      "invalid_args",
      "invalid_source_event",
      "requester_unavailable",
      "requester_unknown",
      "unsupported_request_source",
      "question_unavailable",
      "permission_denied",
      "idempotency_conflict",
    ].includes(code);
    const uncertain = previous?.phase === "uncertain" ? !afterLookup : !definite;
    const error = uncertain
      ? "uncertain"
      : code === "requester_unknown"
        ? "requesterUnknown"
        : code === "requester_unavailable"
          ? "unavailable"
          : code === "unsupported_request_source"
            ? "unsupported"
            : "rejected";
    store.getState().fail(key, attempt.clientId, error, uncertain);
    if (definite) summary?.refresh();
  };
  const submit = (answer: ReplyAnswer) =>
    void send(
      answer.kind === "option" ? { v: 1, option_id: answer.id } : { v: 1, text: answer.text },
    );
  return (
    <PendingReplyCard
      question={entry.attempt ? entry.attempt.question : view.question}
      state={state}
      draft={entry.draft}
      sending={entry.attempt?.phase === "sending"}
      uncertain={entry.attempt?.phase === "uncertain"}
      checking={checking}
      error={entry.error ? t(`pendingReply.${entry.error}`) : undefined}
      showRetainedDraft={!entry.confirmed}
      answeredOption={answeredOption}
      onDraftChange={(draft) => store.getState().setDraft(key, draft)}
      onSubmit={submit}
      onRetry={() => {
        if (entry.attempt) void send(entry.attempt.answer);
      }}
      onUseComposer={onUseComposer}
    />
  );
}

export function PendingReplySlot({
  event,
  groupId,
  readOnly,
  onUseComposer,
  checking,
}: {
  event: LedgerEvent;
  groupId: string;
  readOnly?: boolean;
  onUseComposer: () => void;
  checking?: boolean;
}) {
  const store = useContext(PendingReplyStoreContext);
  if (readOnly || !store || !groupId || !event.id || !pendingReplyView(event)) return null;
  return (
    <ReplyEntry
      store={store}
      groupId={groupId}
      event={event}
      onUseComposer={onUseComposer}
      checking={checking}
    />
  );
}
