import { describe, expect, it } from "vitest";

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
      data: {
        text: "relay ping",
        dst_group_id: "g_remote",
        dst_to: ["@foreman"],
      },
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
      data: {
        text: "relay ping",
        dst_group_id: "g_dst",
      },
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
