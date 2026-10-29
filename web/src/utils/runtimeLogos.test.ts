import { describe, expect, it } from "vite-plus/test";
import { getRuntimeLogoSrc } from "./runtimeLogos";

describe("runtime logos", () => {
  it("shows Grok Bot actors with their own icon, not the Grok Build CLI mark", () => {
    expect(getRuntimeLogoSrc("grok_web_model")).toMatch(/logos\/grok-bot\.svg$/);
    expect(getRuntimeLogoSrc("grok")).toMatch(/logos\/grok\.svg$/);
  });
});
