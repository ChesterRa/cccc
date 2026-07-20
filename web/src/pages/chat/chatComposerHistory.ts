import type { LedgerEvent } from "../../types";

export interface ComposerHistorySession {
  groupId: string;
  entries: string[];
  index: number;
  draft: string;
}

export interface ComposerHistoryMove {
  session: ComposerHistorySession | null;
  text: string;
}

export function buildComposerHistoryEntries(messages: LedgerEvent[]): string[] {
  const entries: string[] = [];
  for (const event of messages) {
    if (String(event?.kind || "").trim() !== "chat.message") continue;
    if (event._streaming || String(event?.by || "").trim() !== "user") continue;
    const data =
      event?.data && typeof event.data === "object"
        ? (event.data as Record<string, unknown>)
        : null;
    if (!data || data._optimistic === true) continue;
    if (String(data.src_group_id || "").trim()) continue;
    if (typeof data.text !== "string" || !data.text.trim()) continue;
    entries.push(data.text);
  }
  return entries;
}

export function canStartComposerHistory(input: {
  composerText: string;
  composerGroupSettled: boolean;
  selectedGroupId: string;
  busy: string;
  menuOpen: boolean;
  isComposing: boolean;
  hasModifier: boolean;
}): boolean {
  return Boolean(
    input.composerText === "" &&
    input.composerGroupSettled &&
    String(input.selectedGroupId || "").trim() &&
    input.busy !== "send" &&
    !input.menuOpen &&
    !input.isComposing &&
    !input.hasModifier,
  );
}

export function startComposerHistory(
  entries: string[],
  groupId: string,
  draft: string,
): ComposerHistorySession | null {
  if (entries.length === 0) return null;
  return {
    groupId: String(groupId || "").trim(),
    entries: entries.slice(),
    index: entries.length - 1,
    draft,
  };
}

export function getComposerHistoryText(session: ComposerHistorySession): string {
  return session.entries[session.index] || "";
}

export function moveComposerHistory(
  session: ComposerHistorySession,
  direction: "older" | "newer",
): ComposerHistoryMove {
  if (direction === "older") {
    const nextSession = { ...session, index: Math.max(0, session.index - 1) };
    return { session: nextSession, text: getComposerHistoryText(nextSession) };
  }

  if (session.index >= session.entries.length - 1) {
    return { session: null, text: session.draft };
  }
  const nextSession = { ...session, index: session.index + 1 };
  return { session: nextSession, text: getComposerHistoryText(nextSession) };
}
