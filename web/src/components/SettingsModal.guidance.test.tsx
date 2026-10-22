// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { SettingsModal } from "./SettingsModal";
import { useModalStore } from "../stores";
import { writeSettingsLastLocation } from "./modals/settings/settingsLastLocation";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
  Trans: ({ i18nKey }: { i18nKey: string }) => i18nKey,
}));
vi.mock("../services/api", async (original) => ({
  ...(await original<typeof import("../services/api")>()),
  fetchWebAccessSession: async () => ({
    ok: true,
    result: { web_access_session: { can_access_global_settings: true } },
  }),
  fetchActors: async () => ({ ok: true, result: { actors: [] } }),
  fetchGroupPrompts: async () => ({
    ok: true,
    result: {
      preamble: {
        kind: "preamble",
        content: "Preamble",
        filename: "preamble.md",
        source: "home",
        path: "/fixture/preamble.md",
      },
      help: {
        kind: "help",
        content: "Help",
        filename: "help.md",
        source: "home",
        path: "/fixture/help.md",
      },
    },
  }),
  fetchObservability: async () => ({
    ok: true,
    result: { observability: { developer_mode: false, log_level: "INFO" } },
  }),
}));

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  vi.stubGlobal("cancelAnimationFrame", vi.fn());
  HTMLElement.prototype.scrollIntoView = vi.fn();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  writeSettingsLastLocation({ scope: "group", groupTab: "guidance", globalTab: "account" });
  useModalStore.setState({ settingsTarget: null });
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  window.localStorage.clear();
  useModalStore.setState({ settingsTarget: null });
  vi.unstubAllGlobals();
});

it.each([0, 1])(
  "keeps expanded Guidance %i keyboard focus and Escape in the child dialog",
  async (index) => {
    const closeSettings = vi.fn();
    await act(async () =>
      root.render(
        <SettingsModal
          codexVoice={{} as never}
          isOpen
          onClose={closeSettings}
          settings={null}
          onUpdateSettings={async () => {}}
          busy={false}
          isDark={false}
          groupId="g"
        />,
      ),
    );
    // GuidanceTab is a lazy chunk. Wait for it inside act() so its retry render, its
    // prompt load and their effects are flushed by act(), not by the Scheduler after the
    // test has moved on. act() flushes only when its outermost scope exits, so poll
    // between back-to-back scopes rather than with vi.waitFor inside one.
    // Bounded under the 5 s test timeout so a lazy-chunk failure ends at this assertion.
    const readinessDeadline = Date.now() + 4000;
    while (host.querySelectorAll("textarea").length < 2 && Date.now() < readinessDeadline) {
      await act(async () => new Promise((resolve) => setTimeout(resolve, 10)));
    }
    expect(host.querySelectorAll("textarea")).toHaveLength(2);
    const trigger = [...host.querySelectorAll<HTMLButtonElement>("button")].filter(
      (button) => button.textContent === "guidance.expand",
    )[index];
    trigger.focus();
    await act(async () => trigger.click());
    // BodyPortal attaches its host in an effect, after the click's render.
    const child = await vi.waitFor(() => {
      const dialog = [...document.querySelectorAll<HTMLElement>('[role="dialog"]')].find(
        (candidate) => !host.contains(candidate),
      );
      expect(dialog).toBeDefined();
      return dialog!;
    });
    expect(child.contains(document.activeElement)).toBe(true);
    const first = child.querySelector<HTMLButtonElement>("button")!;
    const last = child.querySelector<HTMLTextAreaElement>("textarea")!;
    last.focus();
    await act(async () =>
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Tab", cancelable: true })),
    );
    expect(document.activeElement).toBe(first);
    await act(async () =>
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, cancelable: true }),
      ),
    );
    expect(document.activeElement).toBe(last);
    await act(async () =>
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true })),
    );
    expect(document.contains(child)).toBe(false);
    expect(closeSettings).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(trigger);
    await act(async () =>
      document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true })),
    );
    expect(closeSettings).toHaveBeenCalledOnce();
  },
);
