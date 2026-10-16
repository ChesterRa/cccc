// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { VoiceProviderSelect } from "./VoiceProviderSelect";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) =>
      ({
        codexVoiceProviderLabel: "Voice source",
        "codexVoiceProviders.codex": "Codex Voice",
        "codexVoiceProviders.grok": "Grok Voice",
      })[key] || key,
  }),
}));

let host: HTMLDivElement;
let root: Root;
let controller: CodexVoiceSessionController;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  controller = {
    preferences: { provider: "codex" },
    call: null,
    isEngaged: false,
    updatePreferences: vi.fn(),
    clearError: vi.fn(),
  } as unknown as CodexVoiceSessionController;
});

afterEach(() => {
  act(() => root.unmount());
  act(() => vi.runOnlyPendingTimers());
  vi.useRealTimers();
  host.remove();
});

function render() {
  act(() => root.render(<VoiceProviderSelect controller={controller} />));
  return host.querySelector<HTMLButtonElement>('button[aria-label="Voice source"]')!;
}

it("opens both providers, focuses the selection and supports keyboard navigation", () => {
  const trigger = render();
  expect(trigger.getAttribute("aria-haspopup")).toBe("menu");
  act(() => trigger.click());
  const items = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]'));
  expect(items.map((item) => item.querySelector("span")?.textContent)).toEqual([
    "Codex Voice",
    "Grok Voice",
  ]);
  expect(document.activeElement).toBe(items[0]);
  act(() => items[0].dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true })));
  expect(document.activeElement).toBe(items[1]);
  act(() => items[1].click());
  expect(controller.updatePreferences).toHaveBeenCalledExactlyOnceWith({ provider: "grok" });
  expect(controller.clearError).toHaveBeenCalledOnce();
  expect(document.activeElement).toBe(trigger);
});

it("Escape closes the menu and returns focus without selecting another provider", () => {
  const trigger = render();
  act(() => trigger.click());
  const selected = document.querySelector<HTMLButtonElement>('[role="menuitemradio"]')!;
  act(() => selected.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
  expect(document.querySelector('[role="menu"]')).toBeNull();
  expect(document.activeElement).toBe(trigger);
  expect(controller.updatePreferences).not.toHaveBeenCalled();
});

it("shows the actual call provider and keeps switching disabled during a call", () => {
  controller.call = { provider: "grok" } as CodexVoiceSessionController["call"];
  controller.isEngaged = true;
  const trigger = render();
  expect(trigger.textContent).toContain("Grok Voice");
  expect(trigger.disabled).toBe(true);
  act(() => trigger.click());
  expect(document.querySelector('[role="menu"]')).toBeNull();
  expect(controller.updatePreferences).not.toHaveBeenCalled();
});
