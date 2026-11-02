import * as api from "../../services/api";
import { newClientId } from "../../utils/clientId";

/** The replies a person sends an agent most: go ahead, explain, rethink, or turn it into a choice. */
export const QUICK_REPLY_KINDS = ["proceed", "why", "rethink", "choices"] as const;
export type QuickReplyKind = (typeof QUICK_REPLY_KINDS)[number];

export type QuickReplyTarget = {
  groupId: string;
  replyEventId: string;
  to: string[];
  quoteText: string;
};

export type QuickReplyResult =
  | { status: "sent" }
  | { status: "busy" }
  | { status: "failed"; message: string; unknown: boolean };

// A quick reply keeps its client ID, in memory, until it is known to be sent, so that a retry
// repeats the request under the same ID.
const pendingClientIds = new Map<string, string>();
const inFlight = new Set<string>();

// The page cannot tell whether the daemon recorded these: the request or its answer was lost.
// The Web reports a daemon that dropped the connection after the request as unavailable too.
const UNKNOWN_RESULT_CODES = new Set([
  "NETWORK_ERROR",
  "EMPTY_RESPONSE",
  "HTTP_ERROR",
  "PARSE_ERROR",
  "daemon_unavailable",
]);

export async function sendQuickReply(
  target: QuickReplyTarget,
  kind: QuickReplyKind,
  text: string,
): Promise<QuickReplyResult> {
  const key = [target.groupId, target.replyEventId, kind, text].join("\u0000");
  if (inFlight.has(key)) return { status: "busy" };
  inFlight.add(key);
  const clientId = pendingClientIds.get(key) || newClientId();
  pendingClientIds.set(key, clientId);
  try {
    const response = await api.replyMessage(
      target.groupId,
      text,
      target.to,
      target.replyEventId,
      undefined,
      clientId,
      [],
      target.quoteText,
      "send",
    );
    // Only the recorded event, which a duplicate also returns, proves the reply is sent; an
    // empty success is as unknown as a lost one.
    const result = response.ok
      ? (response.result as { event?: { id?: unknown } } | undefined)
      : null;
    if (response.ok && String(result?.event?.id || "").trim()) {
      pendingClientIds.delete(key);
      return { status: "sent" };
    }
    if (response.ok) return { status: "failed", message: "", unknown: true };
    const code = String(response.error?.code || "");
    return {
      status: "failed",
      message: String(response.error?.message || code),
      unknown: UNKNOWN_RESULT_CODES.has(code),
    };
  } finally {
    inFlight.delete(key);
  }
}
