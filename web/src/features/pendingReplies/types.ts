/** Matches the daemon's Unicode scalar limit; never truncate a user's answer. */
export const MAX_REPLY_SCALARS = 4000;

export type ReplyOption = { id: string; label: string; consequence?: string };

export type ReplyQuestion = {
  options: readonly ReplyOption[];
  recommended?: { option_id: string; reason: string } | null;
};

export type StructuredQuestion = ReplyQuestion & { v: 1; context?: string; prompt: string };

export type ReplyDraft = {
  selection: { kind: "option"; id: string } | { kind: "other" } | null;
  text: string;
};

export type ReplyAnswer = { kind: "option"; id: string } | { kind: "other"; text: string };
export type ReplyState =
  | "pending"
  | "answered"
  | "withdrawn"
  | "unavailable"
  | "requesterUnknown"
  | "unknown"
  | "unsupported";
