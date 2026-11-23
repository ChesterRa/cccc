import { describe, expect, it } from "vite-plus/test";
import type { Actor } from "../../types";
import { chatSetupRequirements } from "./chatSetupRequirements";

describe("chatSetupRequirements", () => {
  it("treats only device-kind native Actors as remote-only without checking source ID strings", () => {
    const deviceActor: Actor = {
      id: "remote-1",
      external_session_id: "opaque-id",
      external_session_kind: "device",
    };
    const localActor: Actor = {
      id: "local-1",
      external_session_id: "remote:spoofed-prefix",
      external_session_kind: "local",
    };
    const legacyShapeActor: Actor = {
      id: "legacy-1",
      external_session_id: "remote:without-projection",
    };

    expect(chatSetupRequirements("g-device", "", [deviceActor], true).needsScope).toBe(false);
    expect(chatSetupRequirements("g-mixed", "", [deviceActor, localActor], true).needsScope).toBe(
      true,
    );
    expect(chatSetupRequirements("g-local", "", [legacyShapeActor], true).needsScope).toBe(true);
  });
});
