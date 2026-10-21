// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vite-plus/test";
import { copyTextToClipboard } from "./copy";

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = "";
  Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
});

const leftInPage = (secret: string) =>
  [...document.querySelectorAll("textarea")].some((area) => area.value === secret);

it.each([
  [
    "throws",
    () =>
      vi.fn(() => {
        throw new Error("blocked");
      }),
  ],
  ["fails", () => vi.fn(() => false)],
  ["succeeds", () => vi.fn(() => true)],
])(
  "removes the fallback copy of the text when the legacy copy %s",
  async (outcome, execCommand) => {
    const secret = "https://cccc.example.test/mcp/web-model/c/token/private-only";
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
      configurable: true,
    });
    Object.defineProperty(document, "execCommand", { value: execCommand(), configurable: true });
    expect(await copyTextToClipboard(secret)).toBe(outcome === "succeeds");
    expect(leftInPage(secret)).toBe(false);
  },
);
