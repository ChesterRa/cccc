import { describe, expect, it } from "vite-plus/test";
import type { RuntimeInfo } from "../types";
import { installedRuntimeFor } from "./installedRuntime";

function detected(...installed: string[]): RuntimeInfo[] {
  // As the daemon reports them where Node and npm exist: web models and custom commands are
  // always available, and DeepSeek is too, since CCCC can install its harness.
  const always = ["deepseek", "web_model", "chatgpt_dots", "grok_web_model", "custom"];
  return ["claude", "codex", "opencode", ...always].map((name) => ({
    name,
    display_name: name,
    available: installed.includes(name) || always.includes(name),
  }));
}

describe("installedRuntimeFor", () => {
  it("keeps the current runtime when it is installed", () => {
    expect(installedRuntimeFor("codex", detected("codex", "claude"))).toBe("codex");
  });

  it("moves to the first installed command-line runtime when the current one is missing", () => {
    expect(installedRuntimeFor("codex", detected("claude"))).toBe("claude");
    expect(installedRuntimeFor("codex", detected("opencode"))).toBe("opencode");
  });

  it("never picks a web model, a custom command or DeepSeek, and leaves the choice alone when nothing is installed", () => {
    expect(installedRuntimeFor("codex", detected())).toBeNull();
    expect(installedRuntimeFor("codex", [])).toBeNull();
  });
});
