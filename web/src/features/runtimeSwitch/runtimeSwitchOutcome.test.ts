import { expect, it } from "vite-plus/test";
import { runtimeSwitchErrorMessage } from "./runtimeSwitchOutcome";

it("distinguishes a Web owner from a stopped process after an incomplete switch", () => {
  expect(
    runtimeSwitchErrorMessage(
      {
        code: "actor_switch_incomplete",
        message: "source offline",
        details: { old_owner: "web_model" },
      },
      (key) => key,
    ),
  ).toBe("runtimeSwitch.incompleteWeb source offline");
});

it("explains how to resolve a busy browser or an unfinished Web turn", () => {
  expect(
    runtimeSwitchErrorMessage({ code: "actor_browser_busy", message: "busy" }, (key) => key),
  ).toBe("runtimeSwitch.browserBusy");
  expect(
    runtimeSwitchErrorMessage({ code: "delivery_unresolved", message: "turn" }, (key) => key),
  ).toBe("runtimeSwitch.deliveryUnresolved");
});
