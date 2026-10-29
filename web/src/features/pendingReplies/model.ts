import type { ChatMessageData, LedgerEvent, LedgerEventStatusPayload } from "../../types";
import type { PendingReplyAnswer } from "../../services/api/pendingReplies";
import { projectMessageMode } from "../../utils/crossGroupRecipients";
import type { ReplyState, StructuredQuestion } from "./types";

type ReplyView = { state: ReplyState; question: StructuredQuestion | null };

function record(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/** Do not interpret an unfamiliar or malformed descriptor as a choice. */
export function parseQuestion(value: unknown): StructuredQuestion | null {
  const input = record(value);
  if (
    !input ||
    input.v !== 1 ||
    typeof input.prompt !== "string" ||
    (input.context !== undefined && typeof input.context !== "string") ||
    (input.options !== undefined && !Array.isArray(input.options))
  )
    return null;
  // The canonical v1 serializer omits the empty options array for a text-only question.
  const values: unknown[] = input.options === undefined ? [] : (input.options as unknown[]);
  if (![0, 2, 3, 4].includes(values.length)) return null;
  const options = [];
  for (const [index, value] of values.entries()) {
    const option = record(value);
    if (
      !option ||
      option.id !== String.fromCharCode(65 + index) ||
      typeof option.label !== "string" ||
      (option.consequence !== undefined && typeof option.consequence !== "string")
    )
      return null;
    options.push({
      id: option.id,
      label: option.label,
      ...(typeof option.consequence === "string" ? { consequence: option.consequence } : {}),
    });
  }
  let recommended: StructuredQuestion["recommended"];
  if (input.recommended !== undefined && input.recommended !== null) {
    const value = record(input.recommended);
    if (
      !value ||
      typeof value.option_id !== "string" ||
      !options.some((option) => option.id === value.option_id) ||
      typeof value.reason !== "string"
    )
      return null;
    recommended = { option_id: value.option_id, reason: value.reason };
  }
  return {
    v: 1,
    prompt: input.prompt,
    ...(typeof input.context === "string" ? { context: input.context } : {}),
    options,
    ...(recommended ? { recommended } : {}),
  };
}

/** The question as a record, without the instruction to answer it. */
export function recordedQuestionText(
  question: StructuredQuestion,
  recommendedLabel: string,
): string {
  const lines = question.options.map(
    (option) =>
      `${option.id}. ${option.label}${option.consequence ? ` — ${option.consequence}` : ""}`,
  );
  if (question.recommended)
    lines.push(
      `${recommendedLabel}: ${question.recommended.option_id} — ${question.recommended.reason}`,
    );
  return [question.context, question.prompt, lines.join("\n")].filter(Boolean).join("\n\n");
}

/** Use only a loaded, matching structured reply, never a draft, as an answer. */
export function recordedOption(
  question: StructuredQuestion | null,
  requestId: string,
  requester: string,
  reply: LedgerEvent | undefined,
): string | null {
  if (!question || reply?.kind !== "chat.message" || reply.by !== "user") return null;
  const data = record(reply.data);
  const answer = record(data?.answer);
  if (
    data?.reply_to !== requestId ||
    data.message_mode !== "send" ||
    !Array.isArray(data.to) ||
    data.to.length !== 1 ||
    data.to[0] !== requester ||
    answer?.v !== 1 ||
    typeof answer.option_id !== "string" ||
    answer.text != null ||
    Object.keys(answer).some((key) => !["v", "option_id", "text"].includes(key))
  )
    return null;
  const option = question.options.find((item) => item.id === answer.option_id);
  if (!option || data.text !== `Selected ${option.id}: ${option.label}`) return null;
  return `${option.id}. ${option.label}`;
}

/** A successful response must contain the answer that was actually sent. */
export function matchesSubmittedAnswer(
  question: StructuredQuestion | null,
  answer: PendingReplyAnswer,
  reply: LedgerEvent,
): boolean {
  const data = record(reply.data);
  if (!data || data.message_mode !== "send") return false;
  if (!question) return !("answer" in data) && "text" in answer && data.text === answer.text;
  const stored = record(data.answer);
  if (
    !stored ||
    stored.v !== 1 ||
    Object.keys(stored).some((key) => !["v", "option_id", "text"].includes(key))
  )
    return false;
  if ("option_id" in answer) {
    const option = question.options.find((item) => item.id === answer.option_id);
    return (
      !!option &&
      stored.option_id === answer.option_id &&
      stored.text == null &&
      data.text === `Selected ${option.id}: ${option.label}`
    );
  }
  return (
    stored.text === answer.text &&
    stored.option_id == null &&
    data.text === (question.options.length ? `Other: ${answer.text}` : answer.text)
  );
}

/** Only the daemon's obligation and requester status can authorize a card. */
export function pendingReplyView(
  event: LedgerEvent,
  status?: LedgerEventStatusPayload,
): ReplyView | null {
  const data = (event.data ?? {}) as ChatMessageData;
  if (
    event.kind !== "chat.message" ||
    event.by === "user" ||
    projectMessageMode(data) !== "request_reply"
  )
    return null;
  const metadata = status?.user_request ?? event._user_request;
  const recipients = Array.isArray(data.to) ? data.to : [];
  if (!metadata && !recipients.some((to) => to === "user" || to === "@user")) return null;
  const obligation = status?.obligation_status?.user ?? event._obligation_status?.user;
  if (
    !metadata ||
    !obligation ||
    obligation.reply_requested !== true ||
    typeof obligation.replied !== "boolean" ||
    typeof obligation.cancelled !== "boolean" ||
    (obligation.replied && obligation.cancelled)
  )
    return { state: "unknown", question: null };

  const question = metadata.question === "valid" ? parseQuestion(data.question) : null;
  if (
    (metadata.question === "valid" && !question) ||
    !["valid", "none", "unavailable"].includes(metadata.question) ||
    (metadata.question === "none" && data.question != null)
  )
    return { state: "unknown", question: null };
  if (obligation.replied) return { state: "answered", question };
  if (obligation.cancelled) return { state: "withdrawn", question };
  switch (metadata.requester) {
    case "available":
      return { state: "pending", question };
    case "unavailable":
    case "unsupported":
      return { state: metadata.requester, question };
    case "unknown":
      return { state: "requesterUnknown", question };
    default:
      return { state: "unknown", question: null };
  }
}
