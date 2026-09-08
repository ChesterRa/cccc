import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { apiJson } from "./base";
import { setIMConfig } from "./im";

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
