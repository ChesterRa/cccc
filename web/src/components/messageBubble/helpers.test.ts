import { describe, expect, it } from "vitest";
import { mayContainMarkdown } from "./helpers";

describe("mayContainMarkdown", () => {
  it("renders a message whose only Markdown is bold text", () => {
    expect(mayContainMarkdown("**重要**：先别推送。")).toBe(true);
    expect(mayContainMarkdown("Gates pass. **Next:** the stand-up.")).toBe(true);
    expect(mayContainMarkdown("**0.4.51** is ready")).toBe(true);
    expect(mayContainMarkdown("**✅ 已完成**")).toBe(true);
    expect(mayContainMarkdown("**（重要）** 先看这条")).toBe(true);
    expect(mayContainMarkdown("**“结果”**：通过")).toBe(true);
  });

  it("keeps globs and stray asterisks as plain text", () => {
    expect(mayContainMarkdown("Matches **/src/** only")).toBe(false);
    expect(mayContainMarkdown("Edit src/**/*.rs files")).toBe(false);
    expect(mayContainMarkdown("** spaced ** is not bold")).toBe(false);
    expect(mayContainMarkdown("a ** b and c ** d")).toBe(false);
    expect(mayContainMarkdown("Plain text with no markup.")).toBe(false);
  });

  it("still recognises the other Markdown it covered before", () => {
    expect(mayContainMarkdown("Use `cargo test` here")).toBe(true);
    expect(mayContainMarkdown("- one\n- two")).toBe(true);
    expect(mayContainMarkdown("[cccc] Attachments:\n**bold**")).toBe(false);
  });
});
