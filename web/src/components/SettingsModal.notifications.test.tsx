// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { SettingsModal } from "./SettingsModal";
import { writeSettingsLastLocation } from "./modals/settings/settingsLastLocation";

const access = vi.hoisted(() => ({ admin: true, signedIn: false }));
vi.mock("./modals/settings/NotificationsTab", () => ({
  NotificationsTab: () => <div>notification fixture</div>,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../services/api", async (original) => ({
  ...(await original<typeof import("../services/api")>()),
  fetchWebAccessSession: vi.fn(async () => ({
    ok: true,
    result: {
      web_access_session: {
        login_active: true,
        can_access_global_settings: access.admin,
        current_browser_signed_in: access.signedIn,
      },
    },
  })),
  fetchObservability: vi.fn(async () => ({
    ok: false,
    error: { code: "fixture", message: "Unavailable" },
  })),
}));

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  access.admin = true;
  access.signedIn = false;
  writeSettingsLastLocation({ scope: "global", groupTab: "guidance", globalTab: "notifications" });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  localStorage.clear();
});
const render = async (allowed: boolean) =>
  act(async () =>
    root.render(
      <SettingsModal
        codexVoice={{} as import("../features/codexVoice/useCodexVoiceShell").CodexVoiceShellState}
        isOpen
        onClose={() => {}}
        settings={null}
        onUpdateSettings={async () => {}}
        busy={false}
        isDark={false}
        notificationsAllowed={allowed}
      />,
    ),
  );

it("does not expose notifications until writable web mode is confirmed", async () => {
  await render(false);
  expect(host.textContent).not.toContain("tabs.notifications");
  expect(host.textContent).not.toContain("notification fixture");
  await render(true);
  expect(host.textContent).toContain("tabs.notifications");
});

it("makes notification settings available to signed-in users without admin rights", async () => {
  access.admin = false;
  access.signedIn = true;
  await render(true);
  expect(host.textContent).toContain("tabs.notifications");
  expect(host.textContent).not.toContain("tabs.webAccess");
});
