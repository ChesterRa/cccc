import { describe, expect, it } from "vite-plus/test";

import type { LedgerEvent, ObligationStatus } from "../types";
import { mergeLedgerEvents } from "../utils/mergeLedgerEvents";
import {
  mergeLedgerEventStatuses,
  updateObligationAtIndex,
  updateReadThroughIndex,
} from "./groupStoreCore";

const pending: ObligationStatus = {
  replied: false,
  cancelled: false,
  reply_requested: true,
  delivery_state: "accepted",
};

const source: LedgerEvent = {
  id: "request",
  kind: "chat.message",
  by: "user",
  data: { message_mode: "request_reply", to: ["worker"] },
  _obligation_status: { worker: pending },
};

const mergers = {
  statuses: (current: LedgerEvent[], snapshot: LedgerEvent) =>
    mergeLedgerEventStatuses(current, {
      [snapshot.id!]: {
        read_status: snapshot._read_status,
        obligation_status: snapshot._obligation_status,
      },
    }),
  history: (current: LedgerEvent[], snapshot: LedgerEvent) =>
    mergeLedgerEvents(current, [snapshot], 100),
};

describe.each(Object.entries(mergers))("late %s response", (_, merge) => {
  it.each(["replied", "cancelled"] as const)("does not reopen a %s obligation", (outcome) => {
    // The HTTP snapshot precedes a live ledger event but arrives after it.
    const live = updateObligationAtIndex([source], 0, { actorId: "worker", [outcome]: true });
    expect(live.changed).toBe(true);
    const recovered = merge(live.next, source);
    expect(recovered[0]._obligation_status?.worker[outcome]).toBe(true);
  });

  it("does not make consumed Mail unread again", () => {
    const mail: LedgerEvent = {
      ...source,
      data: { message_mode: "mail", to: ["worker"] },
      _read_status: { worker: false },
    };
    const live = updateReadThroughIndex([mail], 0, "worker");
    expect(live.changed).toBe(true);
    expect(merge(live.next, mail)[0]._read_status).toEqual({ worker: true });
  });

  it("accepts the daemon's terminal outcome and recipient removal", () => {
    const live = updateObligationAtIndex([source], 0, { actorId: "worker", replied: true });
    const authoritative = {
      ...source,
      _obligation_status: { worker: { ...pending, cancelled: true } },
    };
    expect(merge(live.next, authoritative)[0]._obligation_status).toEqual(
      authoritative._obligation_status,
    );
    expect(merge(live.next, { ...source, _obligation_status: {} })[0]._obligation_status).toEqual(
      {},
    );
  });

  it("does not freeze retryable runtime delivery state", () => {
    const failed = {
      ...source,
      _obligation_status: { worker: { ...pending, delivery_state: "failed" } },
    };
    const retrying = {
      ...source,
      _obligation_status: { worker: { ...pending, delivery_state: "claimed" } },
    };
    expect(merge([failed], retrying)[0]._obligation_status?.worker.delivery_state).toBe("claimed");
  });
});
