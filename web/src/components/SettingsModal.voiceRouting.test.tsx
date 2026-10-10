// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { SettingsModal } from "./SettingsModal";
import type { VoiceSettingsTab } from "./modals/settings/VoiceSettingsTab";
import { useModalStore } from "../stores";
import { writeSettingsLastLocation } from "./modals/settings/settingsLastLocation";
import { CodexVoiceSettingsPanel } from "../features/codexVoice/CodexVoiceSettingsPanel";

const voice = vi.hoisted(() => ({ props: null as ComponentProps<typeof VoiceSettingsTab> | null }));
vi.mock("./modals/settings/VoiceSettingsTab", () => ({
  VoiceSettingsTab: (props: ComponentProps<typeof VoiceSettingsTab>) => {
    voice.props = props;
    return (
      <CodexVoiceSettingsPanel
        active={props.isActive && props.section === "realtime"}
        analystRequest={props.analystRequest}
        controller={{} as never}
        analystSettings={{} as never}
        onAnalystSettingsActive={() => {}}
      />
    );
  },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../features/codexVoice/useVoicePreferences", () => ({ useVoicePreferences: () => ({}) }));
vi.mock("../features/codexVoice/CodexVoiceAudioSettings", () => ({
  CodexVoiceAudioSettings: () => null,
}));
vi.mock("../features/codexVoice/CodexVoicePreferenceFields", () => ({
  CodexVoicePreferenceFields: () => null,
}));
vi.mock("../features/codexVoice/CodexVoiceAnalystSettings", () => ({
  CodexVoiceAnalystSettingsFields: () => <div>Analyst fields</div>,
}));
vi.mock("../services/api", async (original) => ({
  ...(await original<typeof import("../services/api")>()),
  fetchWebAccessSession: async () => ({
    ok: true,
    result: { web_access_session: { can_access_global_settings: true } },
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
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  HTMLElement.prototype.scrollIntoView = vi.fn();
  voice.props = null;
  writeSettingsLastLocation({ scope: "global", globalTab: "voice", groupTab: "guidance" });
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  window.localStorage.clear();
  useModalStore.setState({ settingsTarget: null });
});

async function render(isOpen = true) {
  await act(async () =>
    root.render(
      <SettingsModal
        codexVoice={{} as never}
        isOpen={isOpen}
        onClose={() => {}}
        settings={null}
        onUpdateSettings={async () => {}}
        busy={false}
        isDark={false}
        groupId="g1"
      />,
    ),
  );
}

it("forwards the Analyst destination through settings and clears it for a normal gear visit", async () => {
  useModalStore
    .getState()
    .openSettingsTarget({
      scope: "global",
      tab: "voice",
      voiceSection: "realtime",
      voiceAnalyst: true,
    });
  await render();
  await vi.waitFor(() => expect(voice.props?.analystRequest).toBeGreaterThan(0));
  expect(voice.props?.section).toBe("realtime");
  await act(async () => useModalStore.getState().openCodexVoiceSettings());
  expect(voice.props?.analystRequest).toBe(0);
  expect(voice.props?.section).toBe("realtime");
});

it("does not reuse the previous Analyst destination when a normal gear visit reopens settings", async () => {
  useModalStore
    .getState()
    .openSettingsTarget({
      scope: "global",
      tab: "voice",
      voiceSection: "realtime",
      voiceAnalyst: true,
    });
  await render();
  const visiblePanel = () => host.querySelector<HTMLElement>('[role="tabpanel"]:not([hidden])')!;
  await vi.waitFor(() => expect(visiblePanel().id).toContain("analyst"));
  await render(false);
  await act(async () => useModalStore.getState().openCodexVoiceSettings());
  await render();
  expect(voice.props?.analystRequest).toBe(0);
  expect(visiblePanel().id).toContain("audio");
});
