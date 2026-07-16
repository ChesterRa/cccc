import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import type { IMConfigSaveRequest } from "../../../../src/components/modals/settings/imBridgeConfig";
import {
  saveAndStartIMBridge,
  saveIMConfigDraft,
} from "../../../../src/components/modals/settings/imBridgeConfig";
import * as api from "../../../../src/services/api";

vi.mock("../../../../src/services/api", () => ({ setIMConfig: vi.fn(), startIMBridge: vi.fn() }));

const makeConfig = (overrides: Partial<IMConfigSaveRequest> = {}): IMConfigSaveRequest => ({
  groupId: "g-demo",
  platform: "wecom",
  botTokenEnv: "",
  appTokenEnv: "",
  feishuDomain: "https://open.feishu.cn",
  feishuAppId: "",
  feishuAppSecret: "",
  dingtalkAppKey: "",
  dingtalkAppSecret: "",
  dingtalkRobotCode: "",
  wecomBotId: "corp123",
  wecomSecret: "sec456",
  ...overrides,
});

describe("imBridgeConfig", () => {
  afterEach(() => {
    vi.clearAllMocks();
  });

  it("passes the current draft fields to setIMConfig", async () => {
    vi.mocked(api.setIMConfig).mockResolvedValue({ ok: true, result: {} });

    await saveIMConfigDraft(makeConfig());

    expect(api.setIMConfig).toHaveBeenCalledWith(
      "g-demo",
      "wecom",
      "",
      "",
      expect.objectContaining({ wecom_bot_id: "corp123", wecom_secret: "sec456" }),
    );
  });

  it("saves before starting the bridge", async () => {
    const calls: string[] = [];
    vi.mocked(api.setIMConfig).mockImplementation(async () => {
      calls.push("save");
      return { ok: true, result: {} };
    });
    vi.mocked(api.startIMBridge).mockImplementation(async () => {
      calls.push("start");
      return { ok: true, result: {} };
    });

    await saveAndStartIMBridge(makeConfig());

    expect(calls).toEqual(["save", "start"]);
    expect(api.startIMBridge).toHaveBeenCalledWith("g-demo");
  });

  it("does not start the bridge if saving fails", async () => {
    vi.mocked(api.setIMConfig).mockResolvedValue({
      ok: false,
      error: { code: "save_failed", message: "save failed" },
    });

    const resp = await saveAndStartIMBridge(makeConfig());

    expect(resp.ok).toBe(false);
    expect(api.startIMBridge).not.toHaveBeenCalled();
  });
});
