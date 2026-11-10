import { describe, expect, it } from "vite-plus/test";

import {
  actorSessionReplacement,
  actorSupportsNewSession,
  shouldReconcileStoppedActorStatus,
} from "./AgentTab.model";

describe("actorSessionReplacement", () => {
  const at = "2026-11-10T09:02:11.000000Z";

  it("is absent until the daemon reports when a saved conversation was not resumed", () => {
    expect(actorSessionReplacement({})).toBeNull();
    expect(
      actorSessionReplacement({
        runtime_session_replaced_at: null,
        runtime_session_replaced_reasons: ["model"],
      }),
    ).toBeNull();
  });

  it("keeps the reasons it has words for, in a fixed order", () => {
    expect(
      actorSessionReplacement({
        runtime_session_replaced_at: at,
        runtime_session_replaced_reasons: [
          "launch_identity",
          "cccc_cli",
          "added_later",
          "launch_path",
          "command",
        ],
      }),
    ).toEqual({ at, reasons: ["command", "launch_path", "cccc_cli", "launch_identity"] });
  });

  it("reports a replacement that names no changed setting", () => {
    expect(
      actorSessionReplacement({
        runtime_session_replaced_at: at,
        runtime_session_replaced_reasons: null,
      }),
    ).toEqual({ at, reasons: [] });
  });
});

describe("shouldReconcileStoppedActorStatus", () => {
  it("does not offer a new native or browser session for the resident Dots runtime", () => {
    expect(actorSupportsNewSession("chatgpt_dots")).toBe(false);
  });
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
