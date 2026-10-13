// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { SettingsModal } from "./SettingsModal";
import type WebModelConnectorsTab from "./modals/settings/WebModelConnectorsTab";
import { useModalStore } from "../stores";

// Boundary: only which Web Models view the modal opens is under test here.
const webModels = vi.hoisted(() => ({
  props: null as ComponentProps<typeof WebModelConnectorsTab> | null,
}));
vi.mock("./modals/settings/WebModelConnectorsTab", () => ({
  default: (props: ComponentProps<typeof WebModelConnectorsTab>) => {
    webModels.props = props;
    return null;
  },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
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
  webModels.props = null;
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  window.localStorage.clear();
  useModalStore.setState({ settingsTarget: null });
});

it("opens Web Models directly on the Dots view when a Dots Actor asks for its settings", async () => {
  useModalStore
    .getState()
    .openSettingsTarget({ scope: "global", tab: "webModels", webModelProvider: "dots" });
  await act(async () =>
    root.render(
      <SettingsModal
        codexVoice={{} as never}
        isOpen
        onClose={() => {}}
        settings={null}
        onUpdateSettings={async () => {}}
        busy={false}
        isDark={false}
        groupId="g1"
      />,
    ),
  );
  await vi.waitFor(() => expect(webModels.props?.provider).toBe("dots"));
  expect(webModels.props?.isActive).toBe(true);
});
