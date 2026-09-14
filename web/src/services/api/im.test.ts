import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { apiJson } from "./base";
import { runIMManagement, setIMConfig } from "./im";

vi.mock("./base", () => ({ apiJson: vi.fn() }));

describe("setIMConfig", () => {
  beforeEach(() => vi.clearAllMocks());

  it("sends the Mattermost site and token without unrelated platform credentials", async () => {
    await setIMConfig("g_test", "mattermost", "MATTERMOST_BOT_TOKEN", "SLACK_APP_TOKEN", {
      mattermost_url: "https://mm.example.test/chat",
      feishu_app_secret: "unrelated-secret",
    });

    expect(apiJson).toHaveBeenCalledWith("/api/im/set", {
      method: "POST",
      body: JSON.stringify({
        group_id: "g_test",
        platform: "mattermost",
        bot_token_env: "MATTERMOST_BOT_TOKEN",
        mattermost_url: "https://mm.example.test/chat",
      }),
    });
  });

  it("does not send a cached Mattermost site when saving Slack", async () => {
    await setIMConfig("g_test", "slack", "SLACK_BOT_TOKEN", "SLACK_APP_TOKEN", {
      mattermost_url: "https://mm.example.test",
    });

    expect(apiJson).toHaveBeenCalledWith("/api/im/set", {
      method: "POST",
      body: JSON.stringify({
        group_id: "g_test",
        platform: "slack",
        bot_token_env: "SLACK_BOT_TOKEN",
        app_token_env: "SLACK_APP_TOKEN",
      }),
    });
  });
});

describe("Mattermost management request ordering", () => {
  it("orders the whole workflow across navigation, failures and both platform directions", async () => {
    for (const firstMattermost of [false, true]) {
      let release!: () => void;
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      const order: string[] = [];
      const first = runIMManagement("ordered", firstMattermost, async () => {
        order.push("save");
        await gate;
        order.push("start");
        throw new Error("rejected after save");
      });
      const rejected = expect(first).rejects.toThrow("rejected after save");
      const second = runIMManagement("ordered", !firstMattermost, async () => {
        order.push("second");
      });
      const third = runIMManagement("ordered", false, async () => {
        order.push("third");
      });
      await Promise.resolve();
      expect(order).toEqual(["save"]);
      release();
      await Promise.all([rejected, second, third]);
      expect(order).toEqual(["save", "start", "second", "third"]);
    }
  });

  it("keeps legacy workflows parallel and other groups independent, also after cleanup", async () => {
    for (let round = 0; round < 2; round++) {
      let release!: () => void;
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      const pending = runIMManagement("legacy", false, () => gate);
      const second = vi.fn(async () => {});
      const other = vi.fn(async () => {});
      await runIMManagement("legacy", false, second);
      await runIMManagement("other", true, other);
      expect(second).toHaveBeenCalledOnce();
      expect(other).toHaveBeenCalledOnce();
      const mattermost = runIMManagement("legacy", true, async () => {});
      release();
      await Promise.all([pending, mattermost]);
    }
  });
});
