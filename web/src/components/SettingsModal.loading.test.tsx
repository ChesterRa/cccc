// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { SettingsModal } from "./SettingsModal";
import type { DeveloperTab } from "./modals/settings/DeveloperTab";
import type { IMBridgeTab } from "./modals/settings/IMBridgeTab";
import { writeSettingsLastLocation } from "./modals/settings/settingsLastLocation";
import { useModalStore } from "../stores";
import * as api from "../services/api";
import type { WeixinLoginStatus } from "../types";

const developer = vi.hoisted(() => ({ props: null as ComponentProps<typeof DeveloperTab> | null }));
vi.mock("./modals/settings/DeveloperTab", () => ({
  DeveloperTab: (props: ComponentProps<typeof DeveloperTab>) => {
    developer.props = props;
    return <div>Developer fixture</div>;
  },
}));
const bridge = vi.hoisted(() => ({ props: null as ComponentProps<typeof IMBridgeTab> | null }));
vi.mock("./modals/settings/IMBridgeTab", () => ({
  IMBridgeTab: (props: ComponentProps<typeof IMBridgeTab>) => {
    bridge.props = props;
    return <div data-testid="bridge">{props.groupId}</div>;
  },
}));
vi.mock("./modals/settings/GuidanceTab", () => ({
  GuidanceTab: () => <div>Guidance fixture</div>,
}));
vi.mock("./modals/settings/AutomationTab", () => ({
  AutomationTab: () => <div>Automation fixture</div>,
}));
vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});
vi.mock("../services/api", async (original) => ({
  ...(await original<typeof import("../services/api")>()),
  fetchWebAccessSession: vi.fn(async () => ({
    ok: true,
    result: { web_access_session: { can_access_global_settings: true } },
  })),
  fetchIMStatus: vi.fn(),
  fetchIMConfig: vi.fn(),
  fetchObservability: vi.fn(async () => ({
    ok: true,
    result: { observability: { developer_mode: false, log_level: "INFO" } },
  })),
  fetchPing: vi.fn(async () => ({ ok: true, result: { version: "fixture" } })),
  previewRegistryReconcile: vi.fn(async () => ({
    ok: false,
    error: { code: "fixture", message: "Unavailable" },
  })),
  fetchActors: vi.fn(),
  setIMConfig: vi.fn(),
  startIMBridge: vi.fn(),
  stopIMBridge: vi.fn(),
  unsetIMConfig: vi.fn(),
  fetchWeixinLoginStatus: vi.fn(),
  startWeixinLogin: vi.fn(),
  verifyWeixinLogin: vi.fn(),
  logoutWeixin: vi.fn(),
}));
const loggedIn: WeixinLoginStatus = {
  status: "logged_in",
  logged_in: true,
  account_id: "fixture",
  qrcode_url: "",
  qr_ascii: "",
  error: "",
  running: false,
  pid: null,
  updated_at: "fixture",
};
const waiting = { ...loggedIn, status: "waiting_scan", logged_in: false, running: true };
const loggedOut = { ...loggedIn, status: "idle", logged_in: false };
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
let loginStatus = loggedIn;
let enabled = true;
let running = false;
const props = () => bridge.props!;
async function render(groupId = "g1", isOpen = true) {
  await act(async () =>
    root.render(
      <SettingsModal
        isOpen={isOpen}
        onClose={() => {}}
        settings={null}
        onUpdateSettings={async () => {}}
        busy={false}
        isDark={false}
        groupId={groupId}
      />,
    ),
  );
}
async function tab(tab: "im" | "guidance" | "automation") {
  await act(async () => useModalStore.getState().openSettingsTarget({ scope: "group", tab }));
  await act(async () => {});
}
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  vi.clearAllMocks();
  useModalStore.setState({ settingsTarget: null });
  writeSettingsLastLocation({ scope: "group", groupTab: "im", globalTab: "account" });
  loginStatus = loggedIn;
  enabled = true;
  running = false;
  vi.mocked(api.fetchIMStatus).mockImplementation(async (group_id) => ({
    ok: true,
    result: { group_id, configured: true, platform: "weixin", enabled, running, subscribers: 1 },
  }));
  vi.mocked(api.fetchIMConfig).mockResolvedValue({
    ok: true,
    result: { im: { platform: "weixin", weixin_account_id: "fixture" } },
  });
  vi.mocked(api.fetchActors).mockResolvedValue({ ok: true, result: { actors: [] } });
  vi.mocked(api.fetchWeixinLoginStatus).mockImplementation(async () => ({
    ok: true,
    result: loginStatus,
  }));
  vi.mocked(api.startWeixinLogin).mockImplementation(async () => {
    loginStatus = waiting;
    return { ok: true, result: waiting };
  });
  vi.mocked(api.verifyWeixinLogin).mockImplementation(async () => {
    loginStatus = loggedIn;
    return { ok: true, result: loggedIn };
  });
  vi.mocked(api.setIMConfig).mockImplementation(async () => {
    enabled = false;
    running = false;
    return { ok: true, result: {} };
  });
  vi.mocked(api.startIMBridge).mockImplementation(async () => {
    running = true;
    return { ok: true, result: {} };
  });
  vi.mocked(api.stopIMBridge).mockImplementation(async () => {
    enabled = false;
    running = false;
    return { ok: true, result: {} };
  });
  vi.mocked(api.unsetIMConfig).mockImplementation(async () => {
    enabled = false;
    return { ok: true, result: {} };
  });
  vi.mocked(api.logoutWeixin).mockImplementation(async () => {
    loginStatus = loggedOut;
    return { ok: true, result: loggedOut };
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  window.localStorage.clear();
  useModalStore.setState({ settingsTarget: null });
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("settings read ownership", () => {
  it("does not load unrelated settings or start an IM bridge on a Guidance deep link", async () => {
    useModalStore.getState().openSettingsTarget({ scope: "group", tab: "guidance" });
    await render();
    expect(api.fetchIMStatus).not.toHaveBeenCalled();
    expect(api.fetchIMConfig).not.toHaveBeenCalled();
    expect(api.fetchWeixinLoginStatus).not.toHaveBeenCalled();
    expect(api.fetchObservability).not.toHaveBeenCalled();
    expect(api.fetchActors).not.toHaveBeenCalled();
    expect(api.startIMBridge).not.toHaveBeenCalled();
    await tab("automation");
    expect(api.fetchActors).toHaveBeenCalledTimes(1);
    await tab("im");
    expect(api.fetchIMConfig).toHaveBeenCalledTimes(1);
    expect(api.fetchWeixinLoginStatus).toHaveBeenCalled();
    expect(api.startIMBridge).not.toHaveBeenCalled();
  });
  it("loads Developer settings on demand and preserves its draft on return", async () => {
    await render();
    expect(api.fetchObservability).not.toHaveBeenCalled();
    await act(async () =>
      useModalStore.getState().openSettingsTarget({ scope: "global", tab: "developer" }),
    );
    expect(api.fetchObservability).toHaveBeenCalledTimes(1);
    expect(developer.props?.logLevel).toBe("INFO");
    await act(async () => developer.props!.setLogLevel("DEBUG"));
    await tab("guidance");
    await act(async () =>
      useModalStore.getState().openSettingsTarget({ scope: "global", tab: "developer" }),
    );
    expect(api.fetchObservability).toHaveBeenCalledTimes(1);
    expect(developer.props?.logLevel).toBe("DEBUG");
  });
  it("keeps an IM draft across tab visits without rehydrating it", async () => {
    await render();
    await act(async () => props().setImWeixinAccountId("edited-account"));
    await tab("guidance");
    await tab("im");
    expect(props().imWeixinAccountId).toBe("edited-account");
    expect(api.fetchIMConfig).toHaveBeenCalledTimes(1);
    expect(api.startIMBridge).not.toHaveBeenCalled();
  });
  it("starts once after an explicitly initiated scan completes, including away from its tab", async () => {
    await render();
    expect(api.startIMBridge).not.toHaveBeenCalled();
    await act(async () => props().onStartWeixinLogin());
    await tab("guidance");
    loginStatus = loggedIn;
    await act(async () => vi.advanceTimersByTimeAsync(3000));
    expect(api.startIMBridge).toHaveBeenCalledExactlyOnceWith("g1");
    await act(async () => vi.advanceTimersByTimeAsync(9000));
    await tab("im");
    expect(api.startIMBridge).toHaveBeenCalledTimes(1);
    expect(props().imStatus?.running).toBe(true);
  });
  it("continues an explicitly initiated login after submitting its verification code", async () => {
    await render();
    await act(async () => props().onStartWeixinLogin());
    await act(async () => props().onVerifyWeixin("1234"));
    expect(api.startIMBridge).toHaveBeenCalledExactlyOnceWith("g1");
  });
  it("reports a failed scan-completion start without forgetting login or automatically retrying", async () => {
    await render();
    vi.mocked(api.startIMBridge).mockResolvedValueOnce({
      ok: false,
      error: { code: "fixture", message: "Connection unavailable" },
    });
    await act(async () => props().onStartWeixinLogin());
    loginStatus = loggedIn;
    await act(async () => vi.advanceTimersByTimeAsync(3000));
    expect(props().imConfigError).toBe("Connection unavailable");
    expect(props().weixinLoginStatus?.logged_in).toBe(true);
    await act(async () => vi.advanceTimersByTimeAsync(9000));
    expect(api.startIMBridge).toHaveBeenCalledTimes(1);
    vi.mocked(api.startIMBridge).mockResolvedValueOnce({
      ok: false,
      error: { code: "fixture", message: "Still unavailable" },
    });
    await act(async () => props().onStartBridge());
    expect(props().imConfigError).toBe("Still unavailable");
    expect(props().weixinLoginStatus?.logged_in).toBe(true);
    await act(async () => props().onStartBridge());
    expect(api.startIMBridge).toHaveBeenCalledTimes(3);
    expect(props().imConfigError).toBeUndefined();
    expect(props().imStatus?.running).toBe(true);
  });
  it.each(["stop", "logout", "remove", "group", "close", "platform"] as const)(
    "does not start from a delayed scan response after %s",
    async (action) => {
      await render();
      await act(async () => props().onStartWeixinLogin());
      let finish!: (value: { ok: true; result: WeixinLoginStatus }) => void;
      vi.mocked(api.fetchWeixinLoginStatus).mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      );
      await act(async () => vi.advanceTimersByTimeAsync(3000));
      expect(finish).toBeTypeOf("function");
      await act(async () => {
        if (action === "stop") await props().onStopBridge();
        else if (action === "logout") await props().onLogoutWeixin();
        else if (action === "remove") await props().onRemoveConfig();
        else if (action === "platform") props().onPlatformChange("telegram");
      });
      if (action === "group") await render("g2");
      if (action === "close") await render("g1", false);
      await act(async () => finish({ ok: true, result: loggedIn }));
      await act(async () => vi.advanceTimersByTimeAsync(6000));
      expect(api.startIMBridge).not.toHaveBeenCalled();
      if (action === "group") expect(props().imBusy).toBe(false);
    },
  );
  it("cancels a scan-completion start still waiting behind another management operation", async () => {
    await render();
    await act(async () => props().onStartWeixinLogin());
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    vi.spyOn(api, "runIMManagement").mockImplementationOnce(
      async (_group, _serialized, operation) => {
        await gate;
        return operation();
      },
    );
    loginStatus = loggedIn;
    await act(async () => vi.advanceTimersByTimeAsync(3000));
    expect(props().imBusy).toBe(true);
    expect(api.startIMBridge).not.toHaveBeenCalled();
    await act(async () => props().onStopBridge());
    await act(async () => release());
    expect(api.startIMBridge).not.toHaveBeenCalled();
    expect(props().imStatus?.enabled).toBe(false);
  });
  it("does not revive an expired login intent after a later saved-login read", async () => {
    await render();
    await act(async () => props().onStartWeixinLogin());
    loginStatus = { ...loggedOut, status: "expired" };
    await act(async () => vi.advanceTimersByTimeAsync(3000));
    loginStatus = loggedIn;
    await tab("guidance");
    await tab("im");
    expect(api.startIMBridge).not.toHaveBeenCalled();
  });
});
