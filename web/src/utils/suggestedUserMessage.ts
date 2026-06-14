import type { LedgerEvent } from "../types";

export type SuggestedUserMessage = {
  eventId: string;
  text: string;
  by: string;
  ts: string;
};

export const SUGGESTED_USER_MESSAGE_CONSUMED_KEY = "cccc.suggestedUserMessage.consumed.v1";
export const SUGGESTED_USER_MESSAGE_MAX_CHARS = 4000;

export function normalizeSuggestedUserMessage(value: unknown): string {
  const text = String(value || "").trim();
  if (!text) return "";
  return text.slice(0, SUGGESTED_USER_MESSAGE_MAX_CHARS);
}

export function composerTargetAllowsSuggestedUserMessage(input: {
  selectedGroupId: string;
  destGroupId: string;
  composerGroupSettled: boolean;
}): boolean {
  const selected = String(input.selectedGroupId || "").trim();
  const dest = String(input.destGroupId || "").trim();
  return Boolean(input.composerGroupSettled && selected && dest === selected);
}

function messageTargetsUser(data: Record<string, unknown>): boolean {
  const rawTo = data.to;
  if (!Array.isArray(rawTo)) return false;
  return rawTo.some((item) => {
    const token = String(item || "").trim();
    return token === "user" || token === "@user";
  });
}

export function readConsumedSuggestedUserMessageIds(): Set<string> {
  if (typeof window === "undefined") return new Set();
  try {
    const raw = window.localStorage.getItem(SUGGESTED_USER_MESSAGE_CONSUMED_KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    if (!Array.isArray(parsed)) return new Set();
    return new Set(parsed.map((item) => String(item || "").trim()).filter(Boolean));
  } catch {
    return new Set();
  }
}

export function consumeSuggestedUserMessage(eventId: string): void {
  const id = String(eventId || "").trim();
  if (!id || typeof window === "undefined") return;
  try {
    const consumed = readConsumedSuggestedUserMessageIds();
    consumed.add(id);
    const ids = Array.from(consumed).slice(-200);
    window.localStorage.setItem(SUGGESTED_USER_MESSAGE_CONSUMED_KEY, JSON.stringify(ids));
  } catch {
    // A failed localStorage write should not block accepting the suggestion.
  }
}

export function latestSuggestedUserMessage(
  events: LedgerEvent[] | undefined,
  consumedIds: Set<string> = new Set(),
): SuggestedUserMessage | null {
  const rows = Array.isArray(events) ? events : [];
  for (let index = rows.length - 1; index >= 0; index -= 1) {
    const event = rows[index];
    if (String(event?.kind || "") !== "chat.message") continue;
    const by = String(event.by || "").trim();
    if (by === "user") return null;
    const eventId = String(event.id || "").trim();
    const data = event.data && typeof event.data === "object" ? event.data as Record<string, unknown> : {};
    if (!messageTargetsUser(data)) continue;
    const text = normalizeSuggestedUserMessage(data.suggested_user_message);
    if (!eventId || !text || consumedIds.has(eventId)) return null;
    return {
      eventId,
      text,
      by,
      ts: String(event.ts || "").trim(),
    };
  }
  return null;
}
