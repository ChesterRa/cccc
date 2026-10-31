import { describe, expect, it } from "vite-plus/test";
import { sameVoiceDocumentGeneration, sameVoiceDocumentSession } from "./documentDraftSession";

describe("Voice Secretary draft ownership", () => {
  it("does not confuse a later load of the same path with the original draft", () => {
    const first = { path: "a.md", session: 1 };
    expect(sameVoiceDocumentSession(first, { path: "a.md", session: 3 })).toBe(false);
    expect(sameVoiceDocumentSession(first, { path: "b.md", session: 1 })).toBe(false);
    expect(sameVoiceDocumentSession(first, { path: "a.md", session: 1 })).toBe(true);
  });

  it("permits a refresh to resolve its own initial selection without changing generation", () => {
    expect(
      sameVoiceDocumentGeneration({ path: "", session: 1 }, { path: "a.md", session: 1 }),
    ).toBe(true);
    expect(
      sameVoiceDocumentGeneration({ path: "", session: 1 }, { path: "a.md", session: 2 }),
    ).toBe(false);
  });
});
