import { describe, expect, it } from "vitest";

import { supportsRuntimeNewSession } from "../../src/utils/runtimeNewSession";

describe("supportsRuntimeNewSession", () => {
  it("allows only runtimes with native new-session support", () => {
    expect(supportsRuntimeNewSession("claude")).toBe(true);
    expect(supportsRuntimeNewSession("codex")).toBe(true);
    expect(supportsRuntimeNewSession("gemini")).toBe(true);
    expect(supportsRuntimeNewSession("cc")).toBe(false);
    expect(supportsRuntimeNewSession("web_model")).toBe(false);
    expect(supportsRuntimeNewSession("custom")).toBe(false);
    expect(supportsRuntimeNewSession("")).toBe(false);
  });
});
