import { describe, expect, it, vi } from "vite-plus/test";

import type { LedgerEvent } from "../../types";
import { processLedgerEvent, type LedgerEventProcessorDeps } from "./ledgerEventProcessor";

function processorDeps() {
  const appendEvent = vi.fn();
  const updateReadStatus = vi.fn();
  const updateObligationStatus = vi.fn();
  const refreshActors = vi.fn();
  const noop = vi.fn();
  const deps = {
    actors: [],
    activeTab: "chat",
    chatAtBottom: true,
    onContextSync: noop,
    appendEvent,
    updateReadStatus,
    updateObligationStatus,
    incrementActorUnread: noop,
    incrementWebModelQueued: noop,
    updateActorActivity: noop,
    updateGroupRuntimeState: noop,
    promoteStreamingEventsByPrefix: noop,
    removeStreamingEvent: noop,
    clearEmptyStreamingEventsForActor: noop,
    refreshActors,
    refreshPresentation: noop,
    incrementChatUnread: noop,
    markPresentationSlotAttention: noop,
    clearPresentationSlotAttention: noop,
  } as unknown as LedgerEventProcessorDeps;
  return { deps, appendEvent, updateReadStatus, updateObligationStatus, refreshActors };
}

describe("processLedgerEvent obligation facts", () => {
  it("refreshes authoritative unread counts after a Mail read fact", () => {
    const { deps, appendEvent, updateReadStatus, refreshActors } = processorDeps();
    processLedgerEvent(
      "g1",
      { kind: "mail.read", data: { actor_id: "peer1", event_id: "message-1" } } as LedgerEvent,
      deps,
    );

    expect(updateReadStatus).toHaveBeenCalledWith("message-1", "peer1", "g1");
    expect(refreshActors).toHaveBeenCalledWith("g1", { includeUnread: true });
    expect(appendEvent).not.toHaveBeenCalled();
  });

  it("projects runtime delivery onto the source message without appending a UI event", () => {
    const { deps, appendEvent, updateObligationStatus } = processorDeps();
    processLedgerEvent(
      "g1",
      {
        kind: "runtime.delivery",
        data: { source_event_id: "message-1", actor_id: "peer1", state: "accepted" },
      } as LedgerEvent,
      deps,
    );

    expect(updateObligationStatus).toHaveBeenCalledWith(
      "message-1",
      { actorId: "peer1", deliveryState: "accepted" },
      "g1",
    );
    expect(appendEvent).not.toHaveBeenCalled();
  });

  it("projects reply cancellation onto all source recipients", () => {
    const { deps, appendEvent, updateObligationStatus } = processorDeps();
    processLedgerEvent(
      "g1",
      {
        kind: "chat.reply_request.cancelled",
        data: { source_event_id: "message-1" },
      } as LedgerEvent,
      deps,
    );

    expect(updateObligationStatus).toHaveBeenCalledWith("message-1", { cancelled: true }, "g1");
    expect(appendEvent).not.toHaveBeenCalled();
  });
});
