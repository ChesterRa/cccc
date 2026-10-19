// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { SpeakingVoiceSelect } from "./SpeakingVoiceSelect";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { language: string }) =>
      ({
        codexVoiceVoiceLabel: "Speaking voice",
        "codexVoiceLanguages.ja": "Japanese",
        "codexVoiceLanguages.fr": "French",
        codexVoicePrimaryLanguage: `Primary language: ${values?.language}`,
      })[key] || key,
  }),
}));

let host: HTMLDivElement;
let root: Root;
let controller: CodexVoiceSessionController;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  controller = {
    call: null,
    preferences: { provider: "codex", voice: "cove" },
    supportedVoices: ["cove", "himari", "aube"],
    isEngaged: false,
    updatePreferences: vi.fn(),
  } as unknown as CodexVoiceSessionController;
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

async function render() {
  await act(async () => root.render(<SpeakingVoiceSelect controller={controller} />));
  return host.querySelector<HTMLButtonElement>('button[aria-label="Speaking voice"]')!;
}

it("searches the advertised voices by language and selects the raw voice ID", async () => {
  const trigger = await render();
  await act(async () => trigger.click());
  const search = document.querySelector<HTMLInputElement>("input[cmdk-input]")!;
  expect(search).not.toBeNull();
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
      search,
      "Japanese",
    );
    search.dispatchEvent(new Event("input", { bubbles: true }));
  });
  const options = [...document.querySelectorAll<HTMLElement>('[role="option"]')];
  expect(options).toHaveLength(1);
  expect(options[0].textContent).toContain("Himari");
  expect(options[0].textContent).toContain("Primary language: Japanese");
  await act(async () => options[0].click());
  expect(controller.updatePreferences).toHaveBeenCalledExactlyOnceWith({ voice: "himari" });
  expect(trigger.getAttribute("aria-expanded")).toBe("false");
});

it("uses the current service's catalog without adding Codex language hints to Grok", async () => {
  controller.preferences = { ...controller.preferences, provider: "grok", voice: "eve" };
  controller.supportedVoices = ["eve", "aurora"];
  const trigger = await render();
  expect(trigger.textContent).toContain("Eve");
  await act(async () => trigger.click());
  const options = [...document.querySelectorAll<HTMLElement>('[role="option"]')];
  expect(options.map((option) => option.textContent)).toEqual(["Eve", "Aurora"]);
  await act(async () => options[1].click());
  expect(controller.updatePreferences).toHaveBeenCalledExactlyOnceWith({ voice: "aurora" });
});

it("shows the actual call voice and prevents changing it", async () => {
  controller.isEngaged = true;
  controller.call = { provider: "grok", voice: "aurora" } as CodexVoiceSessionController["call"];
  const trigger = await render();
  expect(trigger.textContent).toContain("Aurora");
  expect(trigger.disabled).toBe(true);
  await act(async () => trigger.click());
  expect(document.querySelector('[role="option"]')).toBeNull();
  expect(controller.updatePreferences).not.toHaveBeenCalled();
});

it("keeps the chosen voice visible while the catalog is unavailable", async () => {
  controller.preferences.voice = "himari";
  controller.supportedVoices = [];
  const trigger = await render();
  expect(trigger.textContent).toContain("Himari");
  expect(trigger.disabled).toBe(true);
});
