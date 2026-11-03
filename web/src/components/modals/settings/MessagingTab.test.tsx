// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createInstance, type i18n } from "i18next";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import enSettings from "../../../i18n/locales/en/settings.json";
import { useUIStore } from "../../../stores/useUIStore";
import { MessagingTab } from "./MessagingTab";

describe("MessagingTab browser display preference", () => {
  let host: HTMLDivElement;
  let root: Root;
  let i18n: i18n;
  let originalState: ReturnType<typeof useUIStore.getState>;
  const onSave = vi.fn();
  const setDefaultSendTo = vi.fn();
  const onSubmit = vi.fn();
  const onReset = vi.fn();

  beforeEach(async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    originalState = useUIStore.getState();
    useUIStore.setState({ showPendingReplyReminders: true });
    vi.clearAllMocks();
    i18n = createInstance();
    await i18n.init({
      lng: "en",
      fallbackLng: "en",
      resources: { en: { settings: enSettings } },
      interpolation: { escapeValue: false },
    });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    useUIStore.setState(originalState, true);
    vi.unstubAllGlobals();
  });

  const render = (busy = false) =>
    act(async () =>
      root.render(
        <I18nextProvider i18n={i18n}>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              onSubmit();
            }}
            onReset={onReset}
          >
            <MessagingTab
              isDark={false}
              busy={busy}
              defaultSendTo="broadcast"
              setDefaultSendTo={setDefaultSendTo}
              onSave={onSave}
            />
          </form>
        </I18nextProvider>,
      ),
    );
  const toggle = () => host.querySelector<HTMLInputElement>('[role="switch"]')!;
  const routingDraft = () => host.querySelector('[role="radio"][aria-checked="true"]')?.textContent;

  it("toggles immediately in both directions without saving or changing the routing draft", async () => {
    await render();
    const input = toggle();
    expect(input.checked).toBe(true);
    expect(input.getAttribute("aria-checked")).toBe("true");
    expect(document.getElementById(input.getAttribute("aria-labelledby")!)?.textContent).toBe(
      "Show pending reply reminders",
    );
    expect(input.closest("section")?.textContent).toContain("all groups in this browser");
    expect(routingDraft()).toBe("Broadcast to all enabled agents");

    for (const checked of [false, true]) {
      await act(async () => toggle().click());
      expect(useUIStore.getState().showPendingReplyReminders).toBe(checked);
      expect(toggle().checked).toBe(checked);
      expect(toggle().getAttribute("aria-checked")).toBe(String(checked));
      expect(routingDraft()).toBe("Broadcast to all enabled agents");
    }
    expect(onSave).not.toHaveBeenCalled();
    expect(setDefaultSendTo).not.toHaveBeenCalled();
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onReset).not.toHaveBeenCalled();
  });

  it("stays available to restore reminders while routing settings are saving", async () => {
    useUIStore.setState({ showPendingReplyReminders: false });
    await render(true);
    expect(host.querySelector<HTMLButtonElement>("[data-settings-save-bar] button")?.disabled).toBe(
      true,
    );
    expect(toggle().checked).toBe(false);
    expect(toggle().disabled).toBe(false);
    await act(async () => toggle().click());
    expect(toggle().checked).toBe(true);
    expect(toggle().getAttribute("aria-checked")).toBe("true");
    expect(useUIStore.getState().showPendingReplyReminders).toBe(true);
    expect(routingDraft()).toBe("Broadcast to all enabled agents");
    expect(onSave).not.toHaveBeenCalled();
    expect(setDefaultSendTo).not.toHaveBeenCalled();
    expect(onSubmit).not.toHaveBeenCalled();
    expect(onReset).not.toHaveBeenCalled();
  });
});
