// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { CodexVoiceSettingsPanel } from "./CodexVoiceSettingsPanel";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
// Section bodies have their own tests; this file covers the panel's navigation contract.
vi.mock("./useVoicePreferences", () => ({ useVoicePreferences: () => ({}) }));
vi.mock("./CodexVoiceAudioSettings", () => ({ CodexVoiceAudioSettings: () => null }));
vi.mock("./CodexVoicePreferenceFields", () => ({ CodexVoicePreferenceFields: () => null }));
vi.mock("./CodexVoiceAnalystSettings", () => ({
  CodexVoiceAnalystSettingsFields: () => <div>Analyst fields</div>,
}));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  HTMLElement.prototype.scrollIntoView = vi.fn();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const visiblePanel = () => host.querySelector<HTMLElement>('[role="tabpanel"]:not([hidden])')!;

it("switches shared settings tabs without recreating the Analyst form", () => {
  const onAnalystSettingsActive = vi.fn();
  act(() =>
    root.render(
      <CodexVoiceSettingsPanel
        active
        controller={{} as never}
        analystSettings={{} as never}
        onAnalystSettingsActive={onAnalystSettingsActive}
      />,
    ),
  );

  const tabs = [...host.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
  expect(tabs.map((tab) => tab.textContent)).toEqual([
    "codexVoiceSettingsVoiceAudio",
    "voicePreferences.notifications",
    "codexVoiceAnalystTitle",
  ]);
  for (const tab of tabs) {
    act(() => tab.click());
    expect(tab.getAttribute("aria-selected")).toBe("true");
    expect(visiblePanel().id).toBe(tab.getAttribute("aria-controls"));
    expect(visiblePanel().getAttribute("aria-labelledby")).toBe(tab.id);
  }

  expect(onAnalystSettingsActive).toHaveBeenLastCalledWith(true);
  act(() => tabs[0].click());
  expect(onAnalystSettingsActive).toHaveBeenLastCalledWith(false);
});

it("consumes an Analyst settings destination once and focuses its tab", async () => {
  const onAnalystSettingsActive = vi.fn();
  const render = async (request: number, active = true) =>
    act(async () =>
      root.render(
        <CodexVoiceSettingsPanel
          active={active}
          analystRequest={request}
          controller={{} as never}
          analystSettings={{} as never}
          onAnalystSettingsActive={onAnalystSettingsActive}
        />,
      ),
    );
  await render(125, false);
  expect(visiblePanel().id).toContain("audio");
  await render(125);
  await vi.waitFor(() => {
    expect(visiblePanel().id).toContain("analyst");
    expect(document.activeElement?.id).toBe("codex-voice-settings-analyst-tab");
  });
  const notifications = host.querySelector<HTMLButtonElement>(
    "#codex-voice-settings-notifications-tab",
  )!;
  act(() => notifications.click());
  await render(125, false);
  await render(125);
  expect(visiblePanel().id).toContain("notifications");
  await render(126);
  await vi.waitFor(() => expect(visiblePanel().id).toContain("analyst"));
});
