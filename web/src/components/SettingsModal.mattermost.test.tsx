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
  startIMBridge: vi.fn(),
  stopIMBridge: vi.fn(),
  unsetIMConfig: vi.fn(),
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
    vi.mocked(api.startIMBridge).mockResolvedValue({ ok: true, result: {} });
    vi.mocked(api.stopIMBridge).mockResolvedValue({ ok: true, result: {} });
    vi.mocked(api.unsetIMConfig).mockResolvedValue({ ok: true, result: {} });
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

  it.each(["save", "start-save", "start", "stop", "remove"] as const)(
    "ignores stale %s continuations across Group visits, including failures",
    async (action) => {
      for (const visit of ["other", "return", "remount", "platform", "platform-return"] as const) {
        for (const outcome of ["success", "rejected", "transport"] as const) {
          vi.mocked(api.fetchIMConfig).mockImplementation(async (gid) => ({
            ok: true,
            result: {
              im: {
                platform: "mattermost",
                mattermost_url: `https://${gid}.example.test`,
                bot_token_env: `${gid}_TOKEN`,
              },
            },
          }));
          await renderGroup("group-a");
          let release!: (value: Awaited<ReturnType<typeof api.setIMConfig>>) => void;
          let reject!: (reason: Error) => void;
          const pending = new Promise<Awaited<ReturnType<typeof api.setIMConfig>>>(
            (resolve, fail) => {
              release = resolve;
              reject = fail;
            },
          );
          const method =
            action === "start"
              ? "startIMBridge"
              : action === "stop"
                ? "stopIMBridge"
                : action === "remove"
                  ? "unsetIMConfig"
                  : "setIMConfig";
          vi.mocked(api[method]).mockReturnValueOnce(pending);
          let running!: Promise<void>;
          await act(async () => {
            const p = props();
            running = Promise.resolve(
              action === "save"
                ? p.onSaveConfig()
                : action === "stop"
                  ? p.onStopBridge()
                  : action === "remove"
                    ? p.onRemoveConfig()
                    : p.onStartBridge(),
            );
          });
          expect(props().imBusy).toBe(true);
          const platformVisit = visit === "platform" || visit === "platform-return";
          if (platformVisit) {
            await choose("telegram");
            if (visit === "platform-return") await choose("mattermost");
          } else {
            if (visit === "remount")
              await act(async () => {
                root.render(null);
              });
            await renderGroup("group-b");
            if (visit === "return") await renderGroup("group-a");
          }
          const target = visit === "return" || platformVisit ? "group-a" : "group-b";
          const platform = visit === "platform" ? "telegram" : "mattermost";
          expect.soft(props().imBusy).toBe(false);
          await act(async () => {
            props().setImMattermostUrl("https://new-draft.example.test");
            props().setImBotTokenEnv("NEW_DRAFT_TOKEN");
          });
          // 新组的另一个保存仍在等待，旧 finally 不能替它清掉 busy。
          let finishNew!: (value: Awaited<ReturnType<typeof api.setIMConfig>>) => void;
          vi.mocked(api.setIMConfig).mockReturnValueOnce(
            new Promise((resolve) => {
              finishNew = resolve;
            }),
          );
          let newRunning!: Promise<void>;
          await act(async () => {
            newRunning = Promise.resolve(props().onSaveConfig());
          });
          const reads = vi.mocked(api.fetchIMConfig).mock.calls.length;
          const starts = vi.mocked(api.startIMBridge).mock.calls.length;
          await act(async () => {
            if (outcome === "transport") reject(new Error("旧组传输失败"));
            else
              release(
                outcome === "rejected"
                  ? { ok: false, error: { code: "old_failure", message: "旧组错误" } }
                  : { ok: true, result: {} },
              );
            await running;
          });
          expect(props().groupId).toBe(target);
          expect(props().imPlatform).toBe(platform);
          expect(props().imMattermostUrl).toBe("https://new-draft.example.test");
          expect(props().imBotTokenEnv).toBe("NEW_DRAFT_TOKEN");
          expect(props().imConfigError).toBeUndefined();
          expect(props().imBusy).toBe(true);
          expect(api.fetchIMConfig).toHaveBeenCalledTimes(reads);
          expect(api.startIMBridge).toHaveBeenCalledTimes(starts);
          await act(async () => {
            finishNew({ ok: true, result: {} });
            await newRunning;
          });
          expect(props().imBusy).toBe(false);
        }
      }
    },
  );

  it.each(["status", "config"] as const)(
    "ignores old %s readback after a management action leaves its scope",
    async (phase) => {
      for (const visit of ["other", "return", "remount", "platform", "platform-return"] as const) {
        await renderGroup("group-a");
        await choose("mattermost");
        let release!: () => void;
        const pending = new Promise<void>((resolve) => {
          release = resolve;
        });
        if (phase === "config") {
          vi.mocked(api.fetchIMConfig).mockImplementationOnce(async () => {
            await pending;
            return {
              ok: true,
              result: {
                im: {
                  platform: "mattermost",
                  mattermost_url: "https://old.example.test",
                  bot_token_env: "OLD_GROUP_TOKEN",
                },
              },
            };
          });
        } else {
          vi.mocked(api.fetchIMStatus).mockImplementationOnce(async () => {
            await pending;
            return {
              ok: true,
              result: {
                group_id: "group-a",
                configured: true,
                running: true,
                enabled: true,
                platform: "mattermost",
                subscribers: 0,
              },
            };
          });
        }
        let running!: Promise<void>;
        await act(async () => {
          running = Promise.resolve(props().onSaveConfig());
        });
        const platformVisit = visit === "platform" || visit === "platform-return";
        if (platformVisit) {
          await choose("telegram");
          if (visit === "platform-return") await choose("mattermost");
        } else {
          if (visit === "remount")
            await act(async () => {
              root.render(null);
            });
          await renderGroup("group-b");
          if (visit === "return") await renderGroup("group-a");
          await choose("mattermost");
        }
        await act(async () => {
          props().setImMattermostUrl("https://current.example.test");
          props().setImBotTokenEnv("CURRENT_GROUP_TOKEN");
        });
        const reads = vi.mocked(api.fetchIMConfig).mock.calls.length;
        const status = props().imStatus;
        await act(async () => {
          release();
          await running;
        });
        expect(props().groupId).toBe(visit === "return" || platformVisit ? "group-a" : "group-b");
        expect(props().imPlatform).toBe(visit === "platform" ? "telegram" : "mattermost");
        expect(props().imStatus).toEqual(status);
        expect(api.fetchIMConfig).toHaveBeenCalledTimes(reads);
        expect(props().imMattermostUrl).toBe("https://current.example.test");
        expect(props().imBotTokenEnv).toBe("CURRENT_GROUP_TOKEN");
        expect(props().imBusy).toBe(false);
      }
    },
  );

  it.each(["telegram", "mattermost"] as const)(
    "preserves the Mattermost draft when Start cannot save over %s",
    async (platform) => {
      vi.mocked(api.fetchIMConfig).mockResolvedValue({
        ok: true,
        result: {
          im: { platform, bot_token_env: "OLD_TOKEN", mattermost_url: "https://old.example.test" },
        },
      });
      await renderGroup("group-a");
      await choose("mattermost");
      await act(async () => {
        props().setImMattermostUrl("https://draft.example.test");
        props().setImBotTokenEnv("DRAFT_TOKEN");
      });
      vi.mocked(api.setIMConfig).mockResolvedValue({
        ok: false,
        error: { code: "save_failed", message: "保存失败，请重试" },
      });
      const reads = vi.mocked(api.fetchIMConfig).mock.calls.length;
      await act(async () => props().onStartBridge());
      expect(api.startIMBridge).not.toHaveBeenCalled();
      expect(api.fetchIMConfig).toHaveBeenCalledTimes(reads);
      expect(props().imPlatform).toBe("mattermost");
      expect(props().imMattermostUrl).toBe("https://draft.example.test");
      expect(props().imBotTokenEnv).toBe("DRAFT_TOKEN");
      expect(props().imConfigError).toBe("保存失败，请重试");

      vi.mocked(api.setIMConfig).mockResolvedValue({ ok: true, result: {} });
      vi.mocked(api.fetchIMConfig).mockResolvedValue({
        ok: true,
        result: {
          im: {
            platform: "mattermost",
            bot_token_env: "DRAFT_TOKEN",
            mattermost_url: "https://draft.example.test",
          },
        },
      });
      vi.mocked(api.startIMBridge).mockResolvedValue({
        ok: false,
        error: { code: "connect_failed", message: "连接失败" },
      });
      await act(async () => props().onStartBridge());
      expect(api.startIMBridge).toHaveBeenCalledOnce();
      expect(api.fetchIMConfig).toHaveBeenCalledTimes(reads + 1);
      expect(props().imConfigError).toBe("连接失败");
    },
  );

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
