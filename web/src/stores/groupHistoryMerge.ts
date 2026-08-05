import type { LedgerEvent } from "../types";

export function mergeOlderLedgerEvents(
  current: LedgerEvent[],
  older: LedgerEvent[],
): { events: LedgerEvent[]; added: number } {
  const existingIds = new Set(
    current.map((event) => String(event.id || "").trim()).filter(Boolean),
  );
  const uniqueOlder = older.filter((event) => {
    const eventId = String(event.id || "").trim();
    if (!eventId || existingIds.has(eventId)) return false;
    existingIds.add(eventId);
    return true;
  });
  return { events: [...uniqueOlder, ...current], added: uniqueOlder.length };
}
