import { expect, it } from "vite-plus/test";
import en from "../i18n/locales/en/actors.json";
import { runtimeConfirmationKeys } from "./runtimeConfirmationKeys";

it("names Hermes's first setup apart from Claude's confirmation and the startup wait", () => {
  const hermes = runtimeConfirmationKeys("hermes");
  const claude = runtimeConfirmationKeys("claude");
  const other = runtimeConfirmationKeys("kiro");
  // These keys are built at run time, so the literal key scan cannot see them.
  for (const keys of [hermes, claude, other])
    for (const key of Object.values(keys)) expect(en).toHaveProperty([key]);
  expect(hermes).toMatchObject({ pending: "runtimeSetupPending", paused: "runtimeSetupPaused" });
  expect(claude).toMatchObject({
    pending: "runtimeConfirmationPending",
    paused: "runtimeConfirmationPaused",
  });
  // Only Claude and Hermes wait for a confirmation terminal; others pause at startup input.
  expect(other.paused).toBe("runtimeStartupInputPaused");
  expect(other.pausedHint).toBe("runtimeStartupInputPausedHint");
});
