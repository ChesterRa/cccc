import { describe, expect, it } from "vite-plus/test";

import type { LedgerEvent } from "../types";
import { mergeLedgerEvents } from "./mergeLedgerEvents";

describe("mergeLedgerEvents", () => {
  it("projects cross-group receipt anchors onto source messages without showing receipt events", () => {
    const source: LedgerEvent = {
      id: "evt_src",
      ts: "2026-01-01T00:00:01.000Z",
      kind: "chat.message",
      group_id: "g_src",
      by: "user",
      data: { text: "relay ping", dst_group_id: "g_remote", dst_to: ["@foreman"] },
    };
    const receipt: LedgerEvent = {
      id: "evt_receipt",
      ts: "2026-01-01T00:00:02.000Z",
      kind: "chat.cross_group_receipt",
      group_id: "g_src",
      by: "system",
      data: {
        source_event_id: "evt_src",
        dst_group_id: "g_remote",
        remote_event_id: "evt_remote",
        status: "sent",
      },
    };

    const merged = mergeLedgerEvents([], [receipt, source], 100);

    expect(merged).toHaveLength(1);
    expect(merged[0].id).toBe("evt_src");
    expect(merged[0].data).toMatchObject({
      text: "relay ping",
      dst_group_id: "g_remote",
      remote_event_id: "evt_remote",
    });
  });

  it("hydrates existing source messages when a later receipt arrives", () => {
    const existing: LedgerEvent = {
      id: "evt_src",
      ts: "2026-01-01T00:00:01.000Z",
      kind: "chat.message",
      data: { text: "relay ping", dst_group_id: "g_dst" },
    };
    const receipt: LedgerEvent = {
      id: "evt_receipt",
      ts: "2026-01-01T00:00:02.000Z",
      kind: "chat.cross_group_receipt",
      data: {
        source_event_id: "evt_src",
        dst_group_id: "g_dst",
        dst_event_id: "evt_dst",
        status: "sent",
      },
    };

    const merged = mergeLedgerEvents([existing], [receipt], 100);

    expect(merged).toHaveLength(1);
    expect(merged[0].id).toBe("evt_src");
    expect(merged[0].data).toMatchObject({
      text: "relay ping",
      dst_group_id: "g_dst",
      dst_event_id: "evt_dst",
    });
  });
});

it.each(["failed", "unconfirmed", "sent"] as const)(
  "preserves %s receipts through raw replay and late queued status",
  (state) => {
    const source: LedgerEvent = {
      id: "source",
      kind: "chat.message",
      data: { text: "remote", dst_instance_id: "b", dst_group_id: "group" },
    };
    const receipt: LedgerEvent = {
      id: "receipt",
      kind: "chat.cross_group_receipt",
      data: {
        source_event_id: "source",
        transport: "connect",
        status: state,
        error: state === "sent" ? "" : "unavailable",
      },
    };
    const projected = mergeLedgerEvents([source], [receipt], 100);
    expect(projected).toHaveLength(1);
    expect(projected[0]._connect_delivery).toMatchObject({
      state,
      error: state === "sent" ? "" : "unavailable",
    });
    const replayed = mergeLedgerEvents(projected, [source], 100);
    expect(replayed[0]._connect_delivery).toEqual(projected[0]._connect_delivery);
    expect(
      mergeLedgerEvents(replayed, [{ ...source, _connect_delivery: { state: "queued" } }], 100)[0]
        ._connect_delivery?.state,
    ).toBe(state);
  },
);

it.each(["sent", "failed", "unconfirmed"] as const)(
  "keeps %s cancellation independent of message delivery across replay",
  (state) => {
    const source: LedgerEvent = {
      id: "source",
      kind: "chat.message",
      data: { text: "question", dst_instance_id: "remote" },
      _connect_delivery: { state: "sent", remote_event_id: "remote-message" },
    };
    const cancel: LedgerEvent = {
      id: "cancel",
      kind: "chat.reply_request.cancelled",
      data: { source_event_id: "source", connect_cancel: { delivery_id: "cancel-job" } },
    };
    const pending = mergeLedgerEvents([source], [cancel], 100);
    expect(pending.find((e) => e.id === "source")?._connect_cancellation?.state).toBe("queued");
    const receipt: LedgerEvent = {
      id: "receipt",
      kind: "chat.cross_group_receipt",
      data: {
        source_event_id: "cancel",
        original_event_id: "source",
        transport: "connect",
        action: "cancel",
        status: state,
        remote_event_id: "remote-control",
      },
    };
    const final = mergeLedgerEvents(pending, [receipt], 100);
    const replayed = mergeLedgerEvents(final, [source, cancel], 100).find((e) => e.id === "source");
    expect(replayed?._connect_cancellation?.state).toBe(state);
    expect(replayed?._connect_delivery).toEqual(source._connect_delivery);
    expect(replayed?.data?.remote_event_id).not.toBe("remote-control");
  },
);

it("keeps retired Bridge reply suppression across receipt projection and raw replay", async () => {
  const { getReplyEventId } = await import("./chatReply");
  const source: LedgerEvent = {
    id: "old-source",
    kind: "chat.message",
    by: "user",
    data: { text: "Old remote request", dst_group_id: "colliding-local-group" },
  };
  const receipt: LedgerEvent = {
    id: "retired-receipt",
    kind: "chat.cross_group_receipt",
    by: "system",
    data: { source_event_id: source.id, status: "unconfirmed", group_bridge_retired: true },
  };
  const projected = mergeLedgerEvents([source], [receipt], 100);
  expect(projected).toHaveLength(1);
  expect(projected[0]._retired_bridge).toBe(true);
  expect(projected[0].data).toEqual(source.data);
  expect(getReplyEventId(projected[0])).toBe("");
  const replayed = mergeLedgerEvents(projected, [source], 100);
  expect(getReplyEventId(replayed[0])).toBe("");
});
