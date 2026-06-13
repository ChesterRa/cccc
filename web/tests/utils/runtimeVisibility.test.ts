import { describe, expect, it } from "vitest";
import {
  filterVisibleRuntimeActors,
  isAssistantRuntimeActor,
  isUnsupportedInternalRuntimeActor,
  isRuntimeSurfaceActorVisible,
} from "../../src/utils/runtimeVisibility";

describe("runtimeVisibility", () => {
  const foreman = { id: "foreman", role: "foreman" };
  const peer = { id: "peer-1", role: "peer" };
  const voice = { id: "voice-secretary", internal_kind: "voice_secretary" };
  const legacy = { id: "legacy-internal", internal_kind: "legacy" };

  it("detects unsupported internal runtime actors", () => {
    expect(isUnsupportedInternalRuntimeActor(legacy)).toBe(true);
    expect(isUnsupportedInternalRuntimeActor(voice)).toBe(false);
    expect(isUnsupportedInternalRuntimeActor(peer)).toBe(false);
    expect(isAssistantRuntimeActor(voice)).toBe(true);
    expect(isAssistantRuntimeActor(peer)).toBe(false);
  });

  it("applies peer runtime visibility only to standard runtime actors", () => {
    expect(
      isRuntimeSurfaceActorVisible(peer, {
        peerRuntimeVisibility: "visible",
      })
    ).toBe(true);
    expect(
      isRuntimeSurfaceActorVisible(peer, {
        peerRuntimeVisibility: "hidden",
      })
    ).toBe(false);
    expect(
      isRuntimeSurfaceActorVisible(voice, {
        peerRuntimeVisibility: "hidden",
        assistantRuntimeVisibility: "visible",
      })
    ).toBe(true);
  });

  it("applies assistant runtime visibility to Voice Secretary", () => {
    expect(
      isRuntimeSurfaceActorVisible(voice, {
        peerRuntimeVisibility: "visible",
        assistantRuntimeVisibility: "hidden",
      })
    ).toBe(false);
    expect(
      isRuntimeSurfaceActorVisible(legacy, {
        peerRuntimeVisibility: "visible",
        assistantRuntimeVisibility: "visible",
      })
    ).toBe(false);
  });

  it("filters runtime actors without hiding actor identity elsewhere", () => {
    expect(
      filterVisibleRuntimeActors([foreman, peer, voice, legacy], {
        peerRuntimeVisibility: "visible",
        assistantRuntimeVisibility: "visible",
      }).map((actor) => actor.id)
    ).toEqual(["foreman", "peer-1", "voice-secretary"]);
    expect(
      filterVisibleRuntimeActors([foreman, peer, voice, legacy], {
        peerRuntimeVisibility: "hidden",
        assistantRuntimeVisibility: "visible",
      }).map((actor) => actor.id)
    ).toEqual(["voice-secretary"]);
  });
});
