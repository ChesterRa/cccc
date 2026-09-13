// @vitest-environment happy-dom
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { SettingsModal } from "./SettingsModal";
import type { IMBridgeTab } from "./modals/settings/IMBridgeTab";
import { writeSettingsLastLocation } from "./modals/settings/settingsLastLocation";
import * as api from "../services/api";

const bridge = vi.hoisted(() => ({ props: null as ComponentProps<typeof IMBridgeTab> | null }));
vi.mock("./modals/settings/IMBridgeTab", () => ({
  IMBridgeTab: (props: ComponentProps<typeof IMBridgeTab>) => {
    bridge.props = props;
    return <div data-testid="bridge">{props.groupId}</div>;
  },
}));
vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});
vi.mock("../services/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../services/api")>()),
  fetchIMStatus: vi.fn(),
  fetchIMConfig: vi.fn(),
  fetchWebAccessSession: vi.fn(),
  fetchObservability: vi.fn(),
  fetchActors: vi.fn(),
  setIMConfig: vi.fn(),
}));

describe("SettingsModal Mattermost draft group isolation", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    writeSettingsLastLocation({ scope: "group", groupTab: "im", globalTab: "account" });
    vi.mocked(api.fetchIMStatus).mockResolvedValue({
      ok: true,
      result: {
        group_id: "group-a",
        configured: false,
        running: false,
        enabled: false,
        platform: "",
        subscribers: 0,
      },
    });
    vi.mocked(api.fetchIMConfig).mockResolvedValue({ ok: true, result: { im: null } });
    const unavailable = {
      ok: false as const,
      error: { code: "unavailable", message: "Unavailable in fixture" },
    };
    vi.mocked(api.fetchWebAccessSession).mockResolvedValue(unavailable);
    vi.mocked(api.fetchObservability).mockResolvedValue(unavailable);
    vi.mocked(api.fetchActors).mockResolvedValue({ ok: true, result: { actors: [] } });
    vi.mocked(api.setIMConfig).mockResolvedValue({ ok: true, result: {} });
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    window.localStorage.clear();
    bridge.props = null;
    vi.clearAllMocks();
  });

  const props = () => {
    if (!bridge.props) throw new Error("IM Bridge not rendered");
    return bridge.props;
  };
  const renderGroup = async (groupId: string) => {
    await act(async () => {
      root.render(
        <SettingsModal
          isOpen
          onClose={() => {}}
          settings={null}
          onUpdateSettings={async () => {}}
          busy={false}
          isDark={false}
          groupId={groupId}
        />,
      );
    });
    await vi.waitFor(() => expect(container.textContent).toContain(groupId));
  };
  const choose = async (platform: "mattermost" | "telegram") => {
    await act(async () => props().onPlatformChange(platform));
  };

  it("clears unsaved Mattermost edits on group transition even without editing the other group", async () => {
    await renderGroup("group-a");
    await choose("mattermost");
    await act(async () => {
      props().setImMattermostUrl("https://a.example.test");
      props().setImBotTokenEnv("GROUP_A_BOT_TOKEN");
    });
    await choose("telegram");
    await renderGroup("group-b");
    await renderGroup("group-a");
    await choose("mattermost");
    expect(props().imMattermostUrl).toBe("");
    expect(props().imBotTokenEnv).toBe("");
  });

  it("restores same-group edits but never saves another group's cached URL or token", async () => {
    await renderGroup("group-a");
    await choose("mattermost");
    await act(async () => {
      props().setImMattermostUrl("https://a.example.test");
      props().setImBotTokenEnv("GROUP_A_BOT_TOKEN");
    });
    await choose("telegram");
    await choose("mattermost");
    expect(props().imMattermostUrl).toBe("https://a.example.test");
    expect(props().imBotTokenEnv).toBe("GROUP_A_BOT_TOKEN");
    await choose("telegram");

    // 保持同一个 SettingsModal 挂载，只改变 groupId，覆盖普通关窗重开以外的路径。
    await renderGroup("group-b");
    await choose("mattermost");
    expect(props().imMattermostUrl).toBe("");
    expect(props().imBotTokenEnv).toBe("");
    await act(async () => {
      props().setImMattermostUrl("https://b.example.test");
      props().setImBotTokenEnv("GROUP_B_BOT_TOKEN");
    });
    await act(async () => props().onSaveConfig());
    expect(api.setIMConfig).toHaveBeenLastCalledWith(
      "group-b",
      "mattermost",
      "GROUP_B_BOT_TOKEN",
      "",
      expect.objectContaining({ mattermost_url: "https://b.example.test" }),
    );
    await choose("telegram");
    await renderGroup("group-a");
    await choose("mattermost");
    expect(props().imMattermostUrl).toBe("");
    expect(props().imBotTokenEnv).toBe("");
  });
});
