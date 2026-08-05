import { describe, expect, it } from "vite-plus/test";
import {
  matchesWebModelActorSelection,
  resolveWebModelActorSelection,
} from "../../src/utils/webModelSelection";

describe("web model actor selection", () => {
  it("rejects empty and partial selections", () => {
    expect(resolveWebModelActorSelection("", "")).toBeNull();
    expect(resolveWebModelActorSelection("g_one", "")).toBeNull();
    expect(resolveWebModelActorSelection("", "chatgpt-web-1")).toBeNull();
  });

  it("normalizes a complete selection", () => {
    expect(resolveWebModelActorSelection(" g_one ", " chatgpt-web-1 ")).toEqual({
      groupId: "g_one",
      actorId: "chatgpt-web-1",
    });
  });

  it("matches only the current complete selection", () => {
    const current = { groupId: "g_one", actorId: "chatgpt-web-1" };
    expect(matchesWebModelActorSelection(current, " g_one ", " chatgpt-web-1 ")).toBe(true);
    expect(matchesWebModelActorSelection(current, "g_two", "chatgpt-web-1")).toBe(false);
    expect(matchesWebModelActorSelection(current, "g_one", "chatgpt-web-2")).toBe(false);
    expect(matchesWebModelActorSelection(current, "", "")).toBe(false);
  });
});
