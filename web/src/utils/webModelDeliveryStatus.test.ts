import { expect, it } from "vite-plus/test";
import type { LedgerEvent } from "../types";
import { isBrowserWebModelRuntime } from "../types";
import {
  buildWebModelDeliveryStatusByEventId,
  latestWebModelDeliveryStatusNeedingAppPermissionHint,
} from "./webModelDeliveryStatus";

it("keeps browser recovery and permission notices on current browser Actors", () => {
  const events = ["chatgpt", "grok", "removed"].map((actor) => ({
    id: actor + "-receipt",
    ts: new Date().toISOString(),
    kind: "web_model.browser_delivery.ambiguous",
    data: { actor_id: actor, delivery_id: actor + "-batch", event_ids: [actor + "-message"] },
  })) as LedgerEvent[];
  const actors = [
    { id: "chatgpt", runtime: "web_model" },
    { id: "grok", runtime: "grok_web_model" },
  ];
  const ids = new Set(
    actors.filter((actor) => isBrowserWebModelRuntime(actor.runtime)).map((actor) => actor.id),
  );
  const statuses = buildWebModelDeliveryStatusByEventId(events, ids);
  expect(Object.keys(statuses)).toEqual(["chatgpt-message"]);
  expect(latestWebModelDeliveryStatusNeedingAppPermissionHint(statuses)?.actorId).toBe("chatgpt");
  expect(buildWebModelDeliveryStatusByEventId(events, new Set())).toEqual({});
  // Projection never deletes or rewrites the historical receipts.
  expect(events).toHaveLength(3);
});

it("applies the same Actor boundary to hydrated message receipts", () => {
  const event = {
    id: "grok-message",
    kind: "chat.message",
    _web_model_delivery_status: {
      state: "ambiguous",
      actor_id: "grok",
      delivery_id: "old-browser-batch",
      updated_at: new Date().toISOString(),
    },
  } as LedgerEvent;
  const statuses = buildWebModelDeliveryStatusByEventId([event], new Set(["chatgpt"]));
  expect(statuses).toEqual({});
  expect(latestWebModelDeliveryStatusNeedingAppPermissionHint(statuses)).toBeNull();
});
