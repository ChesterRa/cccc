import { expect, it } from "vite-plus/test";
import { deliveryProgress } from "./nativeActorStatus";

it("places each delivery phase on its step and never counts callback acceptance as done", () => {
  expect(deliveryProgress("pending")).toEqual({ finished: 0, next: "current" });
  expect(deliveryProgress("delivering")).toEqual({ finished: 0, next: "current" });
  // HTTP acceptance only starts the wait for the Dot to take the turn.
  expect(deliveryProgress("delivered")).toEqual({ finished: 1, next: "current" });
  expect(deliveryProgress("working")).toEqual({ finished: 2, next: "current" });
  expect(deliveryProgress("failed")).toEqual({ finished: 0, next: "error" });
  expect(deliveryProgress("interrupted")).toEqual({ finished: 2, next: "error" });
});

it("keeps historical receipts out of the current task progress", () => {
  for (const phase of [
    "ready",
    "stopped",
    "unconfigured",
    "waiting_pairing",
    "waiting_subscription",
  ])
    expect(deliveryProgress(phase)).toBeNull();
});
it("marks an unclaimed wake at the waiting step without implying task execution", () => {
  expect(deliveryProgress("unclaimed")).toEqual({ finished: 1, next: "error" });
});
