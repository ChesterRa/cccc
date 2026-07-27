import { describe, expect, it } from "vite-plus/test";
import { resolveNewActorId } from "./actorCreateModel";

describe("resolveNewActorId", () => {
  it("uses the suggested id when the field is empty", () => {
    expect(resolveNewActorId("   ", "chatgpt-web-1")).toBe("chatgpt-web-1");
  });

  it("prefers and trims an explicitly entered id", () => {
    expect(resolveNewActorId("  reviewer  ", "chatgpt-web-1")).toBe("reviewer");
  });
});
