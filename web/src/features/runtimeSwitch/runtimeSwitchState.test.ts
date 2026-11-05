import { expect, it } from "vite-plus/test";
import { hasPendingActorSwitch } from "./runtimeSwitchState";
import { runtimeSwitchErrorMessage, runtimeSwitchStartError } from "./runtimeSwitchOutcome";

it("only fences the matching member registration and treats incomplete identity conservatively", () => {
  const group = {
    group_id: "g",
    runtime_switches: [{ actor_id: "a", actor_generation: "old", pending: true }],
  };
  expect(hasPendingActorSwitch(group, { id: "a", generation: "old" }, "g")).toBe(true);
  expect(hasPendingActorSwitch(group, { id: "a", generation: "new" }, "g")).toBe(false);
  expect(hasPendingActorSwitch(group, { id: "a" }, "g")).toBe(true);
  expect(hasPendingActorSwitch(group, { id: "a", generation: "old" }, "other")).toBe(false);
  expect(hasPendingActorSwitch(group, { id: "b", generation: "old" }, "g")).toBe(false);
});

it("distinguishes an unknown result from a failed preparation and its old owner state", () => {
  const t = (key: string) => key;
  expect(runtimeSwitchErrorMessage({ code: "actor_switch_unknown", message: "network" }, t)).toBe(
    "runtimeSwitch.unknown",
  );
  expect(runtimeSwitchErrorMessage({ code: "actor_switch_pending", message: "pending" }, t)).toBe(
    "runtimeSwitch.previousPending",
  );
  expect(
    runtimeSwitchErrorMessage(
      {
        code: "actor_switch_incomplete",
        message: "stop failed",
        details: { old_owner: "restart_failed" },
      },
      t,
    ),
  ).toContain("runtimeSwitch.incompleteRestartFailed");
});

it("includes the startup failure that tells the operator what to fix", () => {
  expect(runtimeSwitchStartError({ message: "Executable was not found" }, (key) => key)).toBe(
    "runtimeSwitch.startError Executable was not found",
  );
  expect(runtimeSwitchStartError(undefined, (key) => key)).toBe("runtimeSwitch.startError");
});

it("joins translated switch notices with the punctuation conventions of each language", async () => {
  const { joinRuntimeSentences } = await import("./runtimeSwitchOutcome");
  const en = (await import("../../i18n/locales/en/actors.json")).default.runtimeSwitch;
  const zh = (await import("../../i18n/locales/zh/actors.json")).default.runtimeSwitch;
  const ja = (await import("../../i18n/locales/ja/actors.json")).default.runtimeSwitch;
  expect(joinRuntimeSentences([en.oldManaged, en.clearCustomManaged, en.profileHint])).toBe(
    `${en.oldManaged} ${en.clearCustomManaged} ${en.profileHint}`,
  );
  for (const locale of [zh, ja]) {
    expect(
      joinRuntimeSentences([locale.oldManaged, locale.clearCustomManaged, locale.profileHint]),
    ).toBe(`${locale.oldManaged}${locale.clearCustomManaged}${locale.profileHint}`);
    expect(joinRuntimeSentences([locale.completed, locale.accessPaused])).toBe(
      `${locale.completed}${locale.accessPaused}`,
    );
  }
});
