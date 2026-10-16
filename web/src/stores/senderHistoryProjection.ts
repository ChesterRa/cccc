import type { LedgerEvent } from "../types";
import {
  mergeEventWithExistingStatus,
  projectCrossGroupReceipts,
} from "../utils/mergeLedgerEvents";

export type SenderHistory = {
  key: string;
  senders: readonly string[];
  events: LedgerEvent[];
  messageRevision: number;
  receiptRevision: number;
};

/** Reads order new rows; transport updates only hydrate known rows and invalidate the forward read. */
export function projectSenderHistory(
  history: SenderHistory,
  incoming: readonly LedgerEvent[],
  previousRaw: readonly LedgerEvent[],
): SenderHistory {
  const known = new Set(history.events.map((event) => event.id));
  const alreadyReceived = new Set(previousRaw.map((event) => event.id));
  const incomingById = new Map(incoming.map((event) => [event.id, event]));
  const events = projectCrossGroupReceipts([
    ...history.events.map((event) => {
      const update = incomingById.get(event.id);
      return update ? mergeEventWithExistingStatus(update, event) : event;
    }),
    ...incoming.filter((event) => event.kind !== "chat.message"),
  ]).filter((event) => event.kind === "chat.message");
  const hasUnseen = incoming.some(
    (event) =>
      event.kind === "chat.message" &&
      history.senders.includes(event.by ?? "") &&
      !known.has(event.id) &&
      !alreadyReceived.has(event.id),
  );
  const hasReceipt = incoming.some(
    (event) =>
      event.kind === "chat.cross_group_receipt" || event.kind === "chat.reply_request.cancelled",
  );
  return {
    ...history,
    events,
    messageRevision: history.messageRevision + Number(hasUnseen),
    receiptRevision: history.receiptRevision + Number(hasReceipt),
  };
}
