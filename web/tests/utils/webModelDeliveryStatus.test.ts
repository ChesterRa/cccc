import { describe, expect, it } from "vitest";

import type { LedgerEvent } from "../../src/types";
import {
  buildWebModelDeliveryStatusByEventId,
  latestWebModelDeliveryStatusNeedingAppPermissionHint,
  webModelDeliveryStatusIsFreshForAppPermissionHint,
  webModelDeliveryNeedsAppPermissionHint,
} from "../../src/utils/webModelDeliveryStatus";

function deliveryEvent(kind: string, data: Record<string, unknown>, ts: string): LedgerEvent {
  return {
    kind,
    ts,
    data,
  };
}

describe("buildWebModelDeliveryStatusByEventId", () => {
  it("keeps the latest delivery state for the same source event", () => {
    const status = buildWebModelDeliveryStatusByEventId([
      deliveryEvent(
        "web_model.browser_delivery.submitting",
        { trigger_event_id: "evt-1", actor_id: "web-1", delivery_id: "del-1" },
        "2026-05-08T00:00:00Z",
      ),
      deliveryEvent(
        "web_model.browser_delivery.submitted",
        {
          trigger_event_id: "evt-1",
          actor_id: "web-1",
          delivery_id: "del-1",
          browser: { submission_evidence: "message_echo" },
        },
        "2026-05-08T00:00:02Z",
      ),
      deliveryEvent(
        "web_model.browser_delivery.failed",
        { trigger_event_id: "evt-1", actor_id: "web-1", delivery_id: "del-1", error: "submit failed" },
        "2026-05-08T00:00:03Z",
      ),
    ]);

    expect(status["evt-1"]).toMatchObject({
      state: "failed",
      actorId: "web-1",
      deliveryId: "del-1",
      updatedAt: "2026-05-08T00:00:03Z",
      detail: "submit failed",
    });
  });

  it("maps a batch delivery status to every event id", () => {
    const status = buildWebModelDeliveryStatusByEventId([
      deliveryEvent(
        "web_model.browser_delivery.pending",
        { event_ids: ["evt-1", "evt-2"], actor_id: "web-1", delivery_id: "del-2" },
        "2026-05-08T00:01:00Z",
      ),
    ]);

    expect(status["evt-1"]?.state).toBe("pending");
    expect(status["evt-2"]?.state).toBe("pending");
    expect(status["evt-1"]).toBe(status["evt-2"]);
  });

  it("maps delayed new-chat binding completion as a terminal bound state", () => {
    const status = buildWebModelDeliveryStatusByEventId([
      deliveryEvent(
        "web_model.browser_delivery.pending",
        { event_ids: ["evt-1"], actor_id: "web-1", delivery_id: "del-2" },
        "2026-05-08T00:01:00Z",
      ),
      deliveryEvent(
        "web_model.browser_delivery.bound",
        {
          event_ids: ["evt-1"],
          actor_id: "web-1",
          delivery_id: "del-2",
          bound_conversation_url: "https://chatgpt.com/c/new-chat",
        },
        "2026-05-08T00:01:03Z",
      ),
    ]);

    expect(status["evt-1"]).toMatchObject({
      state: "bound",
      actorId: "web-1",
      deliveryId: "del-2",
      updatedAt: "2026-05-08T00:01:03Z",
    });
  });

  it("maps ambiguous delivery as a non-failed state", () => {
    const status = buildWebModelDeliveryStatusByEventId([
      deliveryEvent(
        "web_model.browser_delivery.ambiguous",
        {
          trigger_event_id: "evt-ambiguous",
          actor_id: "web-1",
          delivery_id: "del-ambiguous",
          error: "submit verification timed out",
        },
        "2026-05-08T00:01:30Z",
      ),
    ]);

    expect(status["evt-ambiguous"]).toMatchObject({
      state: "ambiguous",
      actorId: "web-1",
      deliveryId: "del-ambiguous",
      detail: "submit verification timed out",
    });
  });

  it("prefers event_ids over trigger_event_id", () => {
    const status = buildWebModelDeliveryStatusByEventId([
      deliveryEvent(
        "web_model.browser_delivery.submitted",
        {
          event_ids: ["batch-1"],
          trigger_event_id: "legacy-trigger",
          actor_id: "web-1",
          delivery_id: "del-3",
        },
        "2026-05-08T00:02:00Z",
      ),
    ]);

    expect(status["batch-1"]?.state).toBe("submitted");
    expect(status["legacy-trigger"]).toBeUndefined();
  });

  it("uses persisted delivery status attached to chat messages", () => {
    const status = buildWebModelDeliveryStatusByEventId([
      {
        id: "evt-persisted",
        kind: "chat.message",
        ts: "2026-05-08T00:03:00Z",
        data: { text: "hello" },
        _web_model_delivery_status: {
          state: "failed",
          actor_id: "web-1",
          delivery_id: "del-4",
          updated_at: "2026-05-08T00:03:04Z",
          detail: "prompt inserted but not submitted",
        },
      },
    ]);

    expect(status["evt-persisted"]).toMatchObject({
      state: "failed",
      actorId: "web-1",
      deliveryId: "del-4",
      updatedAt: "2026-05-08T00:03:04Z",
      detail: "prompt inserted but not submitted",
    });
  });

  it("shows the ChatGPT app permission hint while ChatGPT may still be waiting for app permission", () => {
    const base = {
      actorId: "web-1",
      deliveryId: "del-1",
      updatedAt: "2026-05-08T00:00:00Z",
      detail: "",
    };

    expect(webModelDeliveryNeedsAppPermissionHint({ ...base, state: "submitting" })).toBe(true);
    expect(webModelDeliveryNeedsAppPermissionHint({ ...base, state: "submitted" })).toBe(true);
    expect(webModelDeliveryNeedsAppPermissionHint({ ...base, state: "pending" })).toBe(true);
    expect(webModelDeliveryNeedsAppPermissionHint({ ...base, state: "bound" })).toBe(true);
    expect(webModelDeliveryNeedsAppPermissionHint({ ...base, state: "ambiguous" })).toBe(true);
    expect(webModelDeliveryNeedsAppPermissionHint({ ...base, state: "failed" })).toBe(false);
    expect(webModelDeliveryNeedsAppPermissionHint({ ...base, state: "pending" }, true)).toBe(false);
  });

  it("selects the latest delivery status that needs the ChatGPT app permission hint", () => {
    const statuses = {
      "evt-submitted": {
        state: "submitted" as const,
        actorId: "web-1",
        deliveryId: "del-1",
        updatedAt: "2026-05-08T00:00:02Z",
        detail: "",
      },
      "evt-failed": {
        state: "failed" as const,
        actorId: "web-1",
        deliveryId: "del-2",
        updatedAt: "2026-05-08T00:00:04Z",
        detail: "submit failed",
      },
      "evt-bound": {
        state: "bound" as const,
        actorId: "web-1",
        deliveryId: "del-3",
        updatedAt: "2026-05-08T00:00:05Z",
        detail: "",
      },
    };
    const latest = latestWebModelDeliveryStatusNeedingAppPermissionHint(statuses, false, {
      nowMs: Date.parse("2026-05-08T00:00:20Z"),
    });

    expect(latest).toMatchObject({
      state: "bound",
      deliveryId: "del-3",
    });
    expect(latestWebModelDeliveryStatusNeedingAppPermissionHint({ "evt-bound": latest! }, true)).toBeNull();
  });

  it("does not select stale delivery statuses for the ChatGPT app permission hint", () => {
    const status = {
      state: "submitted" as const,
      actorId: "web-1",
      deliveryId: "del-old",
      updatedAt: "2026-05-08T00:00:00Z",
      detail: "",
    };

    expect(webModelDeliveryStatusIsFreshForAppPermissionHint(
      status,
      Date.parse("2026-05-08T00:10:00Z"),
    )).toBe(true);
    expect(webModelDeliveryStatusIsFreshForAppPermissionHint(
      status,
      Date.parse("2026-05-08T01:00:00Z"),
    )).toBe(false);
    expect(latestWebModelDeliveryStatusNeedingAppPermissionHint({ "evt-old": status }, false, {
      nowMs: Date.parse("2026-05-08T01:00:00Z"),
    })).toBeNull();
  });
});
