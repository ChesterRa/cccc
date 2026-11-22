import { describe, expect, it } from "vite-plus/test";

import type { LedgerEvent, ObligationStatus } from "../types";
import { extractRuntimeDeliveryData } from "../utils/ledgerEventHandlers";
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
        user_request: snapshot._user_request,
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

  it("keeps the live Chet answer and its author through a stale pending snapshot", () => {
    const request: LedgerEvent = {
      ...source,
      by: "worker",
      data: { message_mode: "request_reply", to: ["user"] },
      _obligation_status: { user: pending },
      _user_request: { question: "none", requester: "available" },
    };
    const reply: LedgerEvent = {
      id: "answer",
      kind: "chat.message",
      by: "svc:chet",
      data: { reply_to: request.id, _cccc_chet_group_generation: "generation" },
    };
    expect(updateObligationAtIndex([request], 0, { reply }).changed).toBe(false);
    const live = updateObligationAtIndex([request], 0, {
      reply: { ...reply, data: { ...reply.data, _cccc_chet_on_behalf_of: "user" } },
    });
    expect(live.changed).toBe(true);
    expect(merge(live.next, request)[0]).toMatchObject({
      _obligation_status: { user: { replied: true } },
      _user_request: { reply_event_id: "answer", reply_by: "svc:chet" },
    });
    const cancelled = { ...request, _obligation_status: { user: { ...pending, cancelled: true } } };
    expect(merge(live.next, cancelled)[0]).toMatchObject(cancelled);
    expect(merge(live.next, cancelled)[0]._user_request?.reply_by).toBeUndefined();
  });

  it("keeps a delivery's reason with its state", () => {
    const failed = updateObligationAtIndex([source], 0, {
      actorId: "worker",
      deliveryState: "failed",
      deliveryReason: "stopped",
    });
    expect(failed.next[0]._obligation_status?.worker.delivery_reason).toBe("stopped");

    const failedAgain = updateObligationAtIndex(failed.next, 0, {
      actorId: "worker",
      deliveryState: "failed",
      deliveryReason: "stopped again",
    });
    expect(failedAgain.changed).toBe(true);
    expect(failedAgain.next[0]._obligation_status?.worker.delivery_reason).toBe("stopped again");

    const repeated = updateObligationAtIndex(failedAgain.next, 0, {
      actorId: "worker",
      deliveryState: "failed",
      deliveryReason: "stopped again",
    });
    expect(repeated.changed).toBe(false);

    const retried = updateObligationAtIndex(failedAgain.next, 0, {
      actorId: "worker",
      deliveryState: "claimed",
      deliveryReason: "",
    });
    expect(retried.next[0]._obligation_status?.worker.delivery_reason).toBe("");
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

it("uses the authoritative requester state and does not retain it when that status is removed", () => {
  const userRequest: LedgerEvent = {
    id: "user-request",
    kind: "chat.message",
    by: "worker",
    data: { message_mode: "request_reply", to: ["user"] },
    _obligation_status: { user: pending },
    _user_request: { question: "none", requester: "available" },
  };
  const changed = mergeLedgerEventStatuses([userRequest], {
    "user-request": {
      obligation_status: { user: pending },
      user_request: { question: "none", requester: "unavailable" },
    },
  });
  expect(changed[0]._user_request?.requester).toBe("unavailable");
  const removed = mergeLedgerEventStatuses(changed, {
    "user-request": { obligation_status: { user: pending } },
  });
  expect(removed[0]._user_request).toBeUndefined();
  expect(
    mergeLedgerEvents(changed, [{ ...userRequest, _user_request: undefined }], 100)[0]
      ._user_request,
  ).toBeUndefined();
});

it("updates delivery evidence and waiting reasons from SSE receipts without stale carryover", () => {
  const waiting: LedgerEvent = {
    ...source,
    _obligation_status: {
      worker: { ...pending, delivery_state: "", waiting_reason: "group_paused" },
    },
  };

  // Explicit retryable=true failure keeps an existing higher-priority blocker (e.g. group_paused)
  // rather than downgrading it to not_submitted; when no blocker is known yet, it immediately
  // marks waiting not_submitted without flashing terminal failure, and a later snapshot wins.
  const retryableFailed = extractRuntimeDeliveryData({
    kind: "runtime.delivery",
    data: {
      actor_id: "worker",
      source_event_id: "request",
      state: "failed",
      retryable: true,
      reason: "queue full",
    },
  });
  expect(retryableFailed).toEqual({
    actorId: "worker",
    eventId: "request",
    state: "failed",
    reason: "queue full",
    evidence: undefined,
    waitingReason: "not_submitted",
  });
  const whilePaused = updateObligationAtIndex([waiting], 0, {
    actorId: retryableFailed!.actorId,
    deliveryState: retryableFailed!.state,
    deliveryReason: retryableFailed!.reason,
    deliveryEvidence: retryableFailed!.evidence,
    waitingReason: retryableFailed!.waitingReason,
  });
  expect(whilePaused.next[0]._obligation_status?.worker).toMatchObject({
    delivery_state: "failed",
    delivery_reason: "queue full",
    delivery_evidence: undefined,
    waiting_reason: "group_paused",
  });

  const unblocked: LedgerEvent = {
    ...source,
    _obligation_status: { worker: { ...pending, delivery_state: "" } },
  };
  const afterRetryable = updateObligationAtIndex([unblocked], 0, {
    actorId: retryableFailed!.actorId,
    deliveryState: retryableFailed!.state,
    deliveryReason: retryableFailed!.reason,
    deliveryEvidence: retryableFailed!.evidence,
    waitingReason: retryableFailed!.waitingReason,
  });
  expect(afterRetryable.next[0]._obligation_status?.worker).toMatchObject({
    delivery_state: "failed",
    delivery_reason: "queue full",
    delivery_evidence: undefined,
    waiting_reason: "not_submitted",
  });

  // Authoritative backend snapshot overrides the local SSE not_submitted fallback (e.g. group_paused).
  const pausedSnapshot = mergeLedgerEventStatuses(afterRetryable.next, {
    request: {
      obligation_status: {
        worker: {
          ...pending,
          delivery_state: "failed",
          delivery_reason: "queue full",
          waiting_reason: "group_paused",
        },
      },
    },
  });
  expect(pausedSnapshot[0]._obligation_status?.worker.waiting_reason).toBe("group_paused");

  // Accepted receipt with terminal_written replaces waiting_reason and records evidence.
  const acceptedTerminal = extractRuntimeDeliveryData({
    kind: "runtime.delivery",
    data: {
      actor_id: "worker",
      source_event_id: "request",
      state: "accepted",
      delivery_evidence: "terminal_written",
    },
  });
  const afterTerminal = updateObligationAtIndex(afterRetryable.next, 0, {
    actorId: acceptedTerminal!.actorId,
    deliveryState: acceptedTerminal!.state,
    deliveryReason: acceptedTerminal!.reason,
    deliveryEvidence: acceptedTerminal!.evidence,
    waitingReason: acceptedTerminal!.waitingReason,
  });
  expect(afterTerminal.next[0]._obligation_status?.worker).toMatchObject({
    delivery_state: "accepted",
    delivery_evidence: "terminal_written",
    waiting_reason: undefined,
  });

  // Legacy accepted receipt without delivery_evidence clears prior evidence instead of guessing.
  const legacyAccepted = extractRuntimeDeliveryData({
    kind: "runtime.delivery",
    data: { actor_id: "worker", source_event_id: "request", state: "accepted" },
  });
  const afterLegacy = updateObligationAtIndex(afterTerminal.next, 0, {
    actorId: legacyAccepted!.actorId,
    deliveryState: legacyAccepted!.state,
    deliveryReason: legacyAccepted!.reason,
    deliveryEvidence: legacyAccepted!.evidence,
    waitingReason: legacyAccepted!.waitingReason,
  });
  expect(afterLegacy.next[0]._obligation_status?.worker.delivery_evidence).toBeUndefined();

  // Terminal non-retryable failed receipt clears both evidence and waiting_reason.
  const terminalFailed = extractRuntimeDeliveryData({
    kind: "runtime.delivery",
    data: {
      actor_id: "worker",
      source_event_id: "request",
      state: "failed",
      retryable: false,
      reason: "scope missing",
    },
  });
  expect(terminalFailed?.waitingReason).toBeUndefined();
  const afterTerminalFail = updateObligationAtIndex(afterRetryable.next, 0, {
    actorId: terminalFailed!.actorId,
    deliveryState: terminalFailed!.state,
    deliveryReason: terminalFailed!.reason,
    deliveryEvidence: terminalFailed!.evidence,
    waitingReason: terminalFailed!.waitingReason,
  });
  expect(afterTerminalFail.next[0]._obligation_status?.worker.waiting_reason).toBeUndefined();

  // Settling a reply clears any prior waiting_reason immediately, and neither a late
  // receipt SSE nor a late unsettled snapshot can reintroduce waiting_reason once settled.
  const afterReply = updateObligationAtIndex([waiting], 0, { actorId: "worker", replied: true });
  expect(afterReply.next[0]._obligation_status?.worker.waiting_reason).toBeUndefined();

  const lateReceiptAfterReply = updateObligationAtIndex(afterReply.next, 0, {
    actorId: "worker",
    deliveryState: "failed",
    deliveryReason: "late retryable",
    waitingReason: "not_submitted",
  });
  expect(lateReceiptAfterReply.next[0]._obligation_status?.worker.waiting_reason).toBeUndefined();

  const lateSnapshotAfterReply = mergeLedgerEventStatuses(afterReply.next, {
    request: {
      obligation_status: {
        worker: {
          ...pending,
          replied: false,
          cancelled: false,
          delivery_state: "",
          waiting_reason: "group_paused",
        },
      },
    },
  });
  expect(lateSnapshotAfterReply[0]._obligation_status?.worker).toMatchObject({
    replied: true,
    waiting_reason: undefined,
  });

  // Reading a promoted Mail clears any pending waiting_reason immediately.
  const promotedMail: LedgerEvent = {
    ...source,
    data: { message_mode: "mail", to: ["worker"] },
    _read_status: { worker: false },
    _obligation_status: {
      worker: { ...pending, delivery_state: "failed", waiting_reason: "not_submitted" },
    },
  };
  const afterMailRead = updateReadThroughIndex([promotedMail], 0, "worker");
  expect(afterMailRead.changed).toBe(true);
  expect(afterMailRead.next[0]._read_status?.worker).toBe(true);
  expect(afterMailRead.next[0]._obligation_status?.worker.waiting_reason).toBeUndefined();
});
