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
vi.mock("./CodexVoiceAnalystSettings", async () => {
  const { CodexVoiceSettingsHeading } = await import("./CodexVoiceSettingsSection");
  return {
    CodexVoiceAnalystSettings: ({ heading }: { heading: string }) => (
      <CodexVoiceSettingsHeading>{heading}</CodexVoiceSettingsHeading>
    ),
  };
});

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const visiblePanel = () => host.querySelector<HTMLElement>('[role="tabpanel"]:not([hidden])')!;

it("titles each visible section with its own tab label and switches on tab click", () => {
  const onClose = vi.fn();
  act(() =>
    root.render(
      <CodexVoiceSettingsPanel active={false} controller={{} as never} onClose={onClose} />,
    ),
  );

  const tabs = [...host.querySelectorAll<HTMLButtonElement>('[role="tab"]')];
  expect(tabs.map((tab) => tab.textContent)).toEqual([
    "codexVoiceSettingsVoiceAudio",
    "voicePreferences.notifications",
    "codexVoiceAnalystTitle",
  ]);
  for (const [index, tab] of tabs.entries()) {
    act(() => tab.click());
    expect(tab.getAttribute("aria-selected")).toBe("true");
    expect(visiblePanel().id).toBe(tab.getAttribute("aria-controls"));
    expect(visiblePanel().querySelector("h4")?.textContent).toBe(tabs[index].textContent);
  }

  act(() =>
    host.querySelector<HTMLButtonElement>('[aria-label="codexVoiceBackToConversation"]')!.click(),
  );
  expect(onClose).toHaveBeenCalledOnce();
});
