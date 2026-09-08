import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import * as api from "../../../services/api";
import { canStartIMBridge, saveAndStartIMBridge, type IMConfigSaveRequest } from "./imBridgeConfig";

vi.mock("../../../services/api", () => ({ setIMConfig: vi.fn(), startIMBridge: vi.fn() }));

describe("canStartIMBridge", () => {
  it("requires Weixin login before starting", () => {
    expect(canStartIMBridge("weixin", false)).toBe(false);
    expect(canStartIMBridge("weixin", true)).toBe(true);
  });

  it("does not apply the Weixin login gate to other platforms", () => {
    expect(canStartIMBridge("dingtalk", false)).toBe(true);
    expect(canStartIMBridge("telegram", false)).toBe(true);
    expect(canStartIMBridge("mattermost", false)).toBe(true);
  });
});

describe("Mattermost configuration draft", () => {
  const draft: IMConfigSaveRequest = {
    groupId: "g_test",
    platform: "mattermost",
    botTokenEnv: "MATTERMOST_BOT_TOKEN",
    appTokenEnv: "",
    mattermostUrl: "https://mm.example.test/chat",
    feishuDomain: "",
    feishuAppId: "",
    feishuAppSecret: "",
    dingtalkAppKey: "",
    dingtalkAppSecret: "",
    dingtalkRobotCode: "",
    wecomBotId: "",
    wecomSecret: "",
    weixinAccountId: "",
  };

  beforeEach(() => vi.resetAllMocks());

  it("saves the site and token before starting the worker", async () => {
    vi.mocked(api.setIMConfig).mockResolvedValue({ ok: true, result: {} });
    vi.mocked(api.startIMBridge).mockResolvedValue({ ok: true, result: {} });
    await saveAndStartIMBridge(draft);

    expect(api.setIMConfig).toHaveBeenCalledWith(
      "g_test",
      "mattermost",
      "MATTERMOST_BOT_TOKEN",
      "",
      expect.objectContaining({ mattermost_url: draft.mattermostUrl }),
    );
    expect(api.startIMBridge).toHaveBeenCalledWith("g_test");
    expect(vi.mocked(api.setIMConfig).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(api.startIMBridge).mock.invocationCallOrder[0],
    );
  });

  it("does not start when configuration validation fails", async () => {
    const failure = { ok: false as const, error: { code: "invalid", message: "invalid site" } };
    vi.mocked(api.setIMConfig).mockResolvedValue(failure);
    expect(await saveAndStartIMBridge(draft)).toEqual(failure);
    expect(api.startIMBridge).not.toHaveBeenCalled();
  });
});
