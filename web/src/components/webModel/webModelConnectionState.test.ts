import { describe, expect, it } from "vite-plus/test";
import { webModelConnectionState } from "./webModelConnectionState";

describe("ChatGPT connection status authority", () => {
  it("does not confuse a ready browser with a verified binding", () => {
    expect(webModelConnectionState({ state: "unpaired" }, { ready: true }).displayState).toBe(
      "unpaired",
    );
  });
  it.each(["waiting", "awaiting_confirmation", "failed", "interrupted", "cancelled"])(
    "keeps %s visible even with an old binding",
    (state) => {
      expect(
        webModelConnectionState({ state, url: "https://chatgpt.com/c/old" }, { ready: true }),
      ).toMatchObject({
        displayState: ["failed", "interrupted", "cancelled"].includes(state)
          ? `replacement_${state}`
          : state,
        bound: true,
      });
    },
  );
  it("distinguishes a stopped bound Actor from browser authentication issues", () => {
    const pairing = { state: "bound", url: "https://chatgpt.com/c/old", actor_enabled: false };
    expect(webModelConnectionState(pairing, { active: false }).displayState).toBe("bound_stopped");
    expect(
      webModelConnectionState(pairing, { active: true, login_required: true }).displayState,
    ).toBe("login_required");
    expect(
      webModelConnectionState(pairing, { active: true, verification_required: true }).displayState,
    ).toBe("verification_required");
  });
});
