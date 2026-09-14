// @vitest-environment happy-dom
import { useUIStore } from "../../stores/useUIStore";
import { useGroupStore } from "../../stores/useGroupStore";
import { describe, expect, it, vi } from "vite-plus/test";

import type { ChatMessageData, LedgerEvent } from "../../types";
import { processLedgerEvent, type LedgerEventProcessorDeps } from "./ledgerEventProcessor";

function processorDeps() {
  const appendEvent = vi.fn();
  const updateReadStatus = vi.fn();
  const updateObligationStatus = vi.fn();
  const updateActorActivity = vi.fn();
  const refreshActors = vi.fn();
  const noop = vi.fn();
  const deps = {
    actors: [],
    activeTab: "chat",
    chatAtBottom: true,
    onContextSync: noop,
    onGroupScopeChanged: noop,
    appendEvent,
    updateReadStatus,
    updateObligationStatus,
    incrementActorUnread: noop,
    incrementWebModelQueued: noop,
    updateActorActivity,
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
  return {
    deps,
    appendEvent,
    updateReadStatus,
    updateObligationStatus,
    updateActorActivity,
    refreshActors,
  };
}

describe("processLedgerEvent actor activity", () => {
  it("keeps the source group id on actor activity updates", () => {
    const { deps, updateActorActivity } = processorDeps();
    const actors = [{ id: "peer1", running: false }];

    processLedgerEvent(
      "g_previous",
      { kind: "actor.activity", data: { actors } } as LedgerEvent,
      deps,
    );

    expect(updateActorActivity).toHaveBeenCalledWith(actors, "g_previous");
  });
});

describe("processLedgerEvent obligation facts", () => {
  function connectReplyFixture() {
    useGroupStore.setState({ chatByGroup: {} });
    const source = { instance_id: "A", device_id: "device-a", group_id: "g1" };
    const target = { instance_id: "B", device_id: "device-b", group_id: "g2" };
    const sender = { id: "worker", generation: "worker-1" };
    const request = {
      id: "request-1",
      kind: "chat.message",
      group_id: "g1",
      by: "user",
      data: {
        text: "Please reply",
        message_mode: "request_reply",
        dst_instance_id: "B",
        dst_group_id: "g2",
        connect_message: { source, target, recipients: [sender], delivery_id: "delivery-1" },
      },
      _obligation_status: {
        worker: { replied: false, cancelled: false, reply_requested: true, delivery_state: "" },
      },
    } as LedgerEvent;
    const reply = {
      id: "reply-1",
      kind: "chat.message",
      group_id: "g1",
      by: "connect:B",
      data: {
        text: "Done",
        message_mode: "send",
        reply_to: request.id,
        src_instance_id: "B",
        src_group_id: "g2",
        connect_message: {
          source: target,
          target: source,
          sender,
          reply_to: { event_id: request.id, delivery_id: "delivery-1" },
        },
      },
    } as LedgerEvent;
    const { deps } = processorDeps();
    deps.appendEvent = useGroupStore.getState().appendEvent;
    deps.updateObligationStatus = useGroupStore.getState().updateObligationStatus;
    useGroupStore.getState().setEvents([request], "g1");
    return { request, reply, deps };
  }

  it("projects a qualified remote reply immediately without reloading message statuses", () => {
    const { reply, deps } = connectReplyFixture();
    processLedgerEvent("g1", reply, deps);
    const request = useGroupStore
      .getState()
      .chatByGroup.g1.events.find((e) => e.id === "request-1");
    expect(request?._obligation_status?.worker.replied).toBe(true);
    processLedgerEvent("g1", reply, deps);
    expect(
      useGroupStore.getState().chatByGroup.g1.events.filter((e) => e.id === "reply-1"),
    ).toHaveLength(1);
  });

  it("updates a remote request retained only in the open history window", () => {
    const { request, reply, deps } = connectReplyFixture();
    const state = useGroupStore.getState();
    useGroupStore.setState({
      chatByGroup: {
        ...state.chatByGroup,
        g1: {
          ...state.chatByGroup.g1,
          events: [],
          chatWindow: {
            groupId: "g1",
            centerEventId: request.id!,
            centerIndex: 0,
            events: [request],
            hasMoreBefore: false,
            hasMoreAfter: false,
          },
        },
      },
    });
    processLedgerEvent("g1", reply, deps);
    expect(
      useGroupStore.getState().chatByGroup.g1.chatWindow?.events[0]._obligation_status?.worker
        .replied,
    ).toBe(true);
  });

  it("preserves a cancellation that preceded the remote reply", () => {
    const { reply, deps } = connectReplyFixture();
    deps.updateObligationStatus("request-1", { cancelled: true }, "g1");
    processLedgerEvent("g1", reply, deps);
    expect(
      useGroupStore.getState().chatByGroup.g1.events.find((e) => e.id === "request-1")
        ?._obligation_status?.worker,
    ).toMatchObject({ replied: false, cancelled: true });
  });

  it("still fulfills a local recipient's obligation for an inbound Connect request", () => {
    const { request, reply, deps } = connectReplyFixture();
    delete (request.data as ChatMessageData).dst_instance_id;
    delete (request.data as ChatMessageData).dst_group_id;
    request.by = "connect:B";
    reply.by = "worker";
    delete (reply.data as ChatMessageData).connect_message;
    useGroupStore.getState().setEvents([request], "g1");
    processLedgerEvent("g1", reply, deps);
    expect(
      useGroupStore.getState().chatByGroup.g1.events.find((e) => e.id === "request-1")
        ?._obligation_status?.worker.replied,
    ).toBe(true);
  });

  it.each(["local", "instance", "group", "device", "actor", "generation", "reference", "missing"])(
    "does not fulfill a remote obligation with a mismatched %s identity",
    (mismatch) => {
      const { reply, deps } = connectReplyFixture();
      const data = reply.data as ChatMessageData;
      const remote = structuredClone(data.connect_message!);
      data.connect_message = remote;
      if (mismatch === "local") {
        reply.by = "worker";
        delete data.connect_message;
      }
      if (mismatch === "instance") {
        remote.source.instance_id = "C";
        reply.by = "connect:C";
      }
      if (mismatch === "group") remote.source.group_id = "other";
      if (mismatch === "device") remote.source.device_id = "replacement";
      if (mismatch === "actor") remote.sender.id = "other";
      if (mismatch === "generation") remote.sender.generation = "worker-2";
      if (mismatch === "reference") remote.reply_to!.delivery_id = "other";
      if (mismatch === "missing") delete data.connect_message;
      processLedgerEvent("g1", reply, deps);
      const request = useGroupStore
        .getState()
        .chatByGroup.g1.events.find((e) => e.id === "request-1");
      expect(request?._obligation_status?.worker.replied).toBe(false);
    },
  );
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

describe("Group work view unread state", () => {
  it("counts replies while the message viewport is hidden even if it was at the bottom", () => {
    const { deps } = processorDeps();
    deps.incrementChatUnread = vi.fn();
    useUIStore.setState({ activeTab: "chat", chatSessions: {}, isSmallScreen: false });
    const event = {
      id: "reply-1",
      kind: "chat.message",
      by: "actor-1",
      data: { text: "Work finished", to: ["user"], message_mode: "send" },
    } as LedgerEvent;
    useUIStore.getState().setGroupWorkView("g1", "terminals");
    processLedgerEvent("g1", event, deps);
    expect(deps.incrementChatUnread).toHaveBeenCalledWith("g1");
    vi.mocked(deps.incrementChatUnread).mockClear();
    useUIStore.getState().setGroupWorkView("g1", "messages");
    processLedgerEvent("g1", { ...event, id: "reply-2" }, deps);
    expect(deps.incrementChatUnread).not.toHaveBeenCalled();
  });
});

it("keeps Connect cancellation evidence for propagation while updating obligations", () => {
  const { deps, appendEvent, updateObligationStatus } = processorDeps();
  const event: LedgerEvent = {
    kind: "chat.reply_request.cancelled",
    data: { source_event_id: "source", connect_cancel: { delivery_id: "cancel" } },
  };
  processLedgerEvent("group", event, deps);
  expect(appendEvent).toHaveBeenCalledWith(event, "group");
  expect(updateObligationStatus).toHaveBeenCalledWith("source", { cancelled: true }, "group");
});

it("refreshes scope authority for scope mutations instead of applying historical scope fields", () => {
  const { deps } = processorDeps();
  for (const kind of ["group.set_active_scope", "group.attach", "group.detach_scope"]) {
    processLedgerEvent(
      "g",
      { id: kind, group_id: "g", kind, scope_key: "historical", data: {} } as LedgerEvent,
      deps,
    );
  }
  expect(deps.onGroupScopeChanged).toHaveBeenCalledTimes(3);
});
