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
    const button = document.createElement("button");
    document.body.append(button);
    button.focus();
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
      configurable: true,
    });
    Object.defineProperty(document, "execCommand", { value: execCommand(), configurable: true });
    expect(await copyTextToClipboard(secret)).toBe(outcome === "succeeds");
    expect(leftInPage(secret)).toBe(false);
    expect(document.activeElement).toBe(button);
  },
);

it("does not steal focus if the original control disappears during fallback", async () => {
  const button = document.createElement("button");
  const other = document.createElement("button");
  document.body.append(button, other);
  button.focus();
  Object.defineProperty(document, "execCommand", {
    value: vi.fn(() => {
      button.remove();
      other.focus();
      return true;
    }),
    configurable: true,
  });
  expect(await copyTextToClipboard("hello")).toBe(true);
  expect(document.activeElement).toBe(other);
});

it("preserves focus the user moved while the clipboard API was pending", async () => {
  const first = document.createElement("button");
  const next = document.createElement("button");
  document.body.append(first, next);
  first.focus();
  let reject!: (error: Error) => void;
  Object.defineProperty(navigator, "clipboard", {
    value: {
      writeText: () =>
        new Promise<void>((_resolve, fail) => {
          reject = fail;
        }),
    },
    configurable: true,
  });
  Object.defineProperty(document, "execCommand", { value: vi.fn(() => true), configurable: true });
  const copying = copyTextToClipboard("hello");
  next.focus();
  reject(new Error("denied"));
  expect(await copying).toBe(true);
  expect(document.activeElement).toBe(next);
});
