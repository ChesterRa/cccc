import { describe, expect, it } from "vite-plus/test";

import { shouldReconcileStoppedActorStatus } from "./AgentTab.model";

describe("shouldReconcileStoppedActorStatus", () => {
  const recoverableStoppedActor = {
    activated: true,
    isVisible: true,
    isRunning: false,
    isActorEnabled: true,
    isActorBusy: false,
  };

  it("reconciles an enabled visible actor whose cached state says stopped", () => {
    expect(shouldReconcileStoppedActorStatus(recoverableStoppedActor)).toBe(true);
  });

  it.each([
    ["not activated", { activated: false }],
    ["not visible", { isVisible: false }],
    ["already running", { isRunning: true }],
    ["disabled", { isActorEnabled: false }],
    ["being started or stopped", { isActorBusy: true }],
  ])("does not reconcile when the actor is %s", (_label, override) => {
    expect(shouldReconcileStoppedActorStatus({ ...recoverableStoppedActor, ...override })).toBe(
      false,
    );
  });
});
