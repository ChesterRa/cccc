// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { IMBridgeTab } from "./IMBridgeTab";
import * as api from "../../../services/api";

vi.mock("react-i18next", () => {
  const t = (key: string, fallback?: string | { defaultValue?: string }) =>
    typeof fallback === "string" ? fallback : fallback?.defaultValue || key;
  return {
    Trans: ({ children }: { children?: unknown }) => children,
    useTranslation: () => ({ t }),
  };
});

vi.mock("../../../services/api", async (importOriginal) => {
  const original = await importOriginal<typeof import("../../../services/api")>();
  return {
    ...original,
    fetchIMAuthorized: vi.fn(),
    fetchIMPending: vi.fn(),
    revokeIMChat: vi.fn(),
  };
});

const chats: api.IMAuthorizedChat[] = [
  { platform: "dingtalk", chat_id: "same-chat", thread_id: 0, verbose: false, authorized_at: 1 },
  { platform: "weixin", chat_id: "same-chat", thread_id: 1, verbose: false, authorized_at: 1 },
];

describe("IMBridgeTab revoke loading identity", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.mocked(api.fetchIMAuthorized).mockResolvedValue({ ok: true, result: { authorized: chats } });
    vi.mocked(api.fetchIMPending).mockResolvedValue({ ok: true, result: { pending: [] } });
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    await act(async () => {
      root.render(<IMBridgeTab {...props()} />);
    });
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.clearAllMocks();
  });

  it("disables only the matching chat thread and clears after success", async () => {
    let resolve!: (value: Awaited<ReturnType<typeof api.revokeIMChat>>) => void;
    vi.mocked(api.revokeIMChat).mockReturnValue(new Promise((done) => (resolve = done)));
    const buttons = revokeButtons();

    await act(async () => buttons[0].click());
    expect(api.revokeIMChat).toHaveBeenCalledWith("group-a", "same-chat", 0);
    expect(buttons[0].disabled).toBe(true);
    expect(buttons[1].disabled).toBe(false);

    await act(async () => resolve({ ok: true, result: { revoked: true } }));
    expect(revokeButtons()[0].disabled).toBe(false);
  });

  it("clears the matching loading state after failure", async () => {
    let reject!: (error: Error) => void;
    vi.mocked(api.revokeIMChat).mockReturnValue(new Promise((_, fail) => (reject = fail)));
    const buttons = revokeButtons();

    await act(async () => buttons[1].click());
    expect(buttons[0].disabled).toBe(false);
    expect(buttons[1].disabled).toBe(true);

    await act(async () => reject(new Error("network")));
    expect(revokeButtons()[1].disabled).toBe(false);
  });

  it("keeps the Weixin QR-login flow free of pairing controls", async () => {
    vi.clearAllMocks();
    const weixinProps = props();
    weixinProps.imPlatform = "weixin";
    weixinProps.imStatus = { ...weixinProps.imStatus!, platform: "weixin", subscribers: 1 };
    weixinProps.weixinLoginStatus = {
      status: "logged_in",
      logged_in: true,
      auto_subscribed: true,
      running: false,
    };
    vi.mocked(api.fetchIMAuthorized).mockResolvedValue({
      ok: true,
      result: {
        authorized: [
          {
            platform: "weixin",
            chat_id: "wx-user",
            thread_id: 0,
            verbose: false,
            authorized_at: 1,
            authorization_source: "weixin_login",
          },
        ],
      },
    });

    await act(async () => {
      root.render(<IMBridgeTab {...weixinProps} />);
    });

    expect(api.fetchIMPending).not.toHaveBeenCalled();
    expect(container.textContent).not.toContain("/subscribe");
    expect(container.textContent).not.toContain("Request Key");
    expect(container.textContent).not.toContain("Pending Requests");
    expect(container.textContent).not.toContain("Revoke");
  });

  function revokeButtons(): HTMLButtonElement[] {
    return [...container.querySelectorAll("button")].filter(
      (button) => button.textContent === "Revoke" || button.textContent === "...",
    ) as HTMLButtonElement[];
  }
});

function props(): ComponentProps<typeof IMBridgeTab> {
  const noop = () => undefined;
  return {
    isDark: false,
    groupId: "group-a",
    imStatus: {
      group_id: "group-a",
      enabled: true,
      configured: true,
      running: true,
      platform: "dingtalk",
      subscribers: 2,
    },
    imPlatform: "dingtalk",
    onPlatformChange: noop,
    imBotTokenEnv: "",
    setImBotTokenEnv: noop,
    imAppTokenEnv: "",
    setImAppTokenEnv: noop,
    imFeishuDomain: "",
    setImFeishuDomain: noop,
    imFeishuAppId: "",
    setImFeishuAppId: noop,
    imFeishuAppSecret: "",
    setImFeishuAppSecret: noop,
    imDingtalkAppKey: "key",
    setImDingtalkAppKey: noop,
    imDingtalkAppSecret: "secret",
    setImDingtalkAppSecret: noop,
    imDingtalkRobotCode: "",
    setImDingtalkRobotCode: noop,
    imWecomBotId: "",
    setImWecomBotId: noop,
    imWecomSecret: "",
    setImWecomSecret: noop,
    imWeixinAccountId: "",
    setImWeixinAccountId: noop,
    weixinLoginStatus: null,
    onStartWeixinLogin: noop,
    onVerifyWeixin: noop,
    onLogoutWeixin: noop,
    imBusy: false,
    onSaveConfig: noop,
    onRemoveConfig: noop,
    onStartBridge: noop,
    onStopBridge: noop,
  };
}
