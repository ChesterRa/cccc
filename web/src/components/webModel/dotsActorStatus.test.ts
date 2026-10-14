import { expect, it } from "vite-plus/test";
import type { DotsActorState } from "../../services/api/webModel";
import { deliveryProgress } from "./dotsActorStatus";

const state = (patch: Partial<DotsActorState>): DotsActorState => ({
  group_id: "g",
  actor_id: "dot",
  enabled: true,
  connected: true,
  subscribed: true,
  paired: true,
  phase: "ready",
  pending_count: 0,
  ...patch,
});
const execution = (status: "done" | "failed") => ({
  turn_id: "t",
  event_ids: ["e"],
  status,
  completed_at: "2026-10-12T00:00:00Z",
});

it("places each delivery phase on its step and never counts callback acceptance as done", () => {
  expect(deliveryProgress(state({ phase: "pending" }))).toEqual({ finished: 0, next: "current" });
  expect(deliveryProgress(state({ phase: "delivering" }))).toEqual({
    finished: 0,
    next: "current",
  });
  // HTTP acceptance only starts the wait for the Dot to take the turn.
  expect(deliveryProgress(state({ phase: "delivered" }))).toEqual({ finished: 1, next: "current" });
  expect(deliveryProgress(state({ phase: "working" }))).toEqual({ finished: 2, next: "current" });
  expect(deliveryProgress(state({ phase: "failed" }))).toEqual({ finished: 0, next: "error" });
  expect(deliveryProgress(state({ phase: "interrupted" }))).toEqual({ finished: 2, next: "error" });
});

it("summarizes an idle Actor from its last execution only", () => {
  expect(deliveryProgress(state({ phase: "ready" }))).toBeNull();
  expect(deliveryProgress(state({ phase: "ready", last_execution: execution("done") }))).toEqual({
    finished: 4,
    next: "current",
  });
  expect(
    deliveryProgress(state({ phase: "stopped", last_execution: execution("failed") })),
  ).toBeNull();
});
