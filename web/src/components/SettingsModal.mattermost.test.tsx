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
  fetchWeixinLoginStatus: vi.fn(),
  startWeixinLogin: vi.fn(),
  verifyWeixinLogin: vi.fn(),
  logoutWeixin: vi.fn(),
}));

const weixinStatus = {
  status: "logged_out",
  logged_in: false,
  account_id: "",
  qrcode_url: "",
  qr_ascii: "",
  error: "",
  running: false,
  pid: null,
  updated_at: "2026-09-14T00:00:00Z",
};

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
    for (const method of [
      "fetchWeixinLoginStatus",
      "startWeixinLogin",
      "verifyWeixinLogin",
      "logoutWeixin",
    ] as const) {
      vi.mocked(api[method]).mockResolvedValue({ ok: true, result: weixinStatus });
    }
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
  const choose = async (platform: "mattermost" | "telegram" | "slack" | "weixin") => {
    await act(async () => props().onPlatformChange(platform));
  };

  const invoke = (action: string) => {
    const p = props();
    return Promise.resolve(
      action === "save"
        ? p.onSaveConfig()
        : action === "remove"
          ? p.onRemoveConfig()
          : action === "stop"
            ? p.onStopBridge()
            : p.onStartBridge(),
    );
  };

  it.each(["remove", "stop"])(
    "shows Mattermost %s failures without discarding newer edits",
    async (action) => {
      for (const failure of ["application", "transport"] as const) {
        for (const platform of ["mattermost", "slack"] as const) {
          await renderGroup(`failure-${action}-${failure}-${platform}`);
          await choose(platform);
          let release!: () => void;
          const gate = new Promise<void>((resolve) => {
            release = resolve;
          });
          const method = action === "remove" ? "unsetIMConfig" : "stopIMBridge";
          vi.mocked(api[method]).mockImplementationOnce(async () => {
            await gate;
            if (failure === "transport") throw new Error("private transport details");
            return { ok: false, error: { code: "rejected", message: "Removal rejected" } };
          });
          let request!: Promise<void>;
          await act(async () => {
            request = invoke(action);
          });
          await act(async () => {
            props().setImMattermostUrl("https://new-draft.example.test");
            props().setImBotTokenEnv("NEW_DRAFT_TOKEN");
          });
          const reads = vi.mocked(api.fetchIMConfig).mock.calls.length;
          await act(async () => {
            release();
            await request;
          });
          expect(props().imBusy).toBe(false);
          if (platform === "mattermost") {
            expect(props().imConfigError).toBe(
              failure === "application" ? "Removal rejected" : "imBridge.mattermostConfigFailed",
            );
            expect(props().imMattermostUrl).toBe("https://new-draft.example.test");
            expect(props().imBotTokenEnv).toBe("NEW_DRAFT_TOKEN");
            expect(api.fetchIMConfig).toHaveBeenCalledTimes(reads);
          } else {
            expect(props().imConfigError).toBeUndefined();
          }
        }
      }
    },
  );

  it.each(["login", "logout", "verify"] as const)(
    "orders Weixin %s and Mattermost mutations in both directions",
    async (action) => {
      for (const mattermostFirst of [true, false]) {
        const group = `weixin-order-${action}-${mattermostFirst}`;
        let savedPlatform: "mattermost" | "weixin" = mattermostFirst ? "mattermost" : "weixin";
        vi.mocked(api.fetchIMStatus).mockImplementation(async () => ({
          ok: true,
          result: {
            group_id: group,
            configured: true,
            platform: savedPlatform,
            running: false,
            enabled: false,
            subscribers: 0,
          },
        }));
        vi.mocked(api.fetchIMConfig).mockImplementation(async () => ({
          ok: true,
          result: {
            im: {
              platform: savedPlatform,
              mattermost_url: "https://saved.example.test",
              bot_token_env: "SAVED_TOKEN",
            },
          },
        }));
        const order: string[] = [];
        const apiName =
          action === "login"
            ? "startWeixinLogin"
            : action === "logout"
              ? "logoutWeixin"
              : "verifyWeixinLogin";
        const weixin = () =>
          Promise.resolve(
            action === "login"
              ? props().onStartWeixinLogin()
              : action === "logout"
                ? props().onLogoutWeixin()
                : props().onVerifyWeixin("synthetic-code"),
          );
        let release!: () => void;
        const gate = new Promise<void>((resolve) => {
          release = resolve;
        });
        let first = true;
        vi.mocked(api.setIMConfig).mockImplementation(async (_group, platform) => {
          const blocked = first && (mattermostFirst || action === "login");
          first = false;
          if (blocked) {
            order.push("old-sent");
            await gate;
            order.push("old-done");
          } else order.push("new-sent");
          savedPlatform = platform as typeof savedPlatform;
          return { ok: true, result: {} };
        });
        vi.mocked(api[apiName]).mockImplementation(async () => {
          if (!mattermostFirst && action !== "login" && first) {
            first = false;
            order.push("old-sent");
            await gate;
            order.push("old-done");
          } else order.push("weixin-sent");
          return { ok: true, result: weixinStatus };
        });
        await renderGroup(group);
        await choose(mattermostFirst ? "mattermost" : "weixin");
        let old!: Promise<void>;
        await act(async () => {
          old = mattermostFirst ? invoke("save") : weixin();
        });
        expect(order).toEqual(["old-sent"]);
        await choose(mattermostFirst ? "weixin" : "mattermost");
        let next!: Promise<void>;
        await act(async () => {
          next = mattermostFirst ? weixin() : invoke("save");
        });
        expect.soft(order).toEqual(["old-sent"]);
        await act(async () => {
          release();
          await Promise.all([old, next]);
        });
        expect(order.slice(0, 2)).toEqual(["old-sent", "old-done"]);
        expect(order.length).toBeGreaterThan(2);
        if (!mattermostFirst || action === "login") {
          expect(savedPlatform).toBe(mattermostFirst ? "weixin" : "mattermost");
          expect(props().imPlatform).toBe(savedPlatform);
        }
        if (!mattermostFirst && action === "login") expect(order).not.toContain("weixin-sent");
        expect(props().imBusy).toBe(false);
      }
    },
  );

  it("orders automatic Weixin startup before a later Mattermost save", async () => {
    let savedPlatform: "weixin" | "mattermost" = "weixin";
    vi.mocked(api.fetchIMStatus).mockImplementation(async () => ({
      ok: true,
      result: {
        group_id: "auto-order",
        configured: true,
        platform: savedPlatform,
        running: false,
        enabled: savedPlatform === "weixin",
        subscribers: 0,
      },
    }));
    vi.mocked(api.fetchIMConfig).mockImplementation(async () => ({
      ok: true,
      result: { im: { platform: savedPlatform } },
    }));
    vi.mocked(api.fetchWeixinLoginStatus).mockResolvedValue({
      ok: true,
      result: { ...weixinStatus, logged_in: true },
    });
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    const order: string[] = [];
    vi.mocked(api.startIMBridge).mockImplementationOnce(async () => {
      order.push("auto-sent");
      await gate;
      order.push("auto-done");
      return { ok: true, result: {} };
    });
    vi.mocked(api.setIMConfig).mockImplementationOnce(async () => {
      order.push("mattermost-sent");
      savedPlatform = "mattermost";
      return { ok: true, result: {} };
    });
    await renderGroup("auto-order");
    await vi.waitFor(() => expect(order).toEqual(["auto-sent"]));
    await choose("mattermost");
    let next!: Promise<void>;
    await act(async () => {
      next = invoke("save");
    });
    expect.soft(order).toEqual(["auto-sent"]);
    await act(async () => {
      release();
      await next;
    });
    expect(order).toEqual(["auto-sent", "auto-done", "mattermost-sent"]);
    expect(props().imPlatform).toBe("mattermost");
    expect(props().imBusy).toBe(false);
  });

  it("keeps native legacy continuations but invalidates a Weixin login after visiting Mattermost", async () => {
    for (const viaMattermost of [false, true]) {
      await renderGroup(`weixin-return-${viaMattermost}`);
      await choose("weixin");
      let release!: () => void;
      const gate = new Promise<void>((resolve) => {
        release = resolve;
      });
      vi.mocked(api.setIMConfig).mockImplementationOnce(async () => {
        await gate;
        return { ok: true, result: {} };
      });
      const before = vi.mocked(api.startWeixinLogin).mock.calls.length;
      let pending!: Promise<void>;
      await act(async () => {
        pending = Promise.resolve(props().onStartWeixinLogin());
      });
      await choose(viaMattermost ? "mattermost" : "telegram");
      await choose("weixin");
      await act(async () => {
        release();
        await pending;
      });
      expect(api.startWeixinLogin).toHaveBeenCalledTimes(before + (viaMattermost ? 0 : 1));
    }
  });

  it.each(["save", "start-save", "start", "stop", "remove", "status", "config"])(
    "keeps new edits and releases busy when %s finishes in the same view",
    async (phase) => {
      for (const field of ["url", "token"] as const) {
        await renderGroup(`draft-${phase}-${field}`);
        await choose("mattermost");
        await act(async () => {
          props().setImMattermostUrl("https://before.example.test");
          props().setImBotTokenEnv("BEFORE_TOKEN");
        });
        const method =
          phase === "status"
            ? "fetchIMStatus"
            : phase === "config"
              ? "fetchIMConfig"
              : phase === "start"
                ? "startIMBridge"
                : phase === "stop"
                  ? "stopIMBridge"
                  : phase === "remove"
                    ? "unsetIMConfig"
                    : "setIMConfig";
        let release!: () => void;
        const pending = new Promise<void>((resolve) => {
          release = resolve;
        });
        // Delay the selected boundary and return the old saved config afterward; an empty response would hide draft overwrites.
        const status = {
          ok: true as const,
          result: {
            group_id: `draft-${phase}-${field}`,
            configured: phase !== "remove",
            running: phase === "start",
            enabled: phase === "start",
            platform: "mattermost",
            subscribers: 0,
          },
        };
        const config = {
          ok: true as const,
          result: {
            im: {
              platform: "mattermost" as const,
              mattermost_url: "https://before.example.test",
              bot_token_env: "BEFORE_TOKEN",
            },
          },
        };
        vi.mocked(api.fetchIMStatus).mockResolvedValue(status);
        vi.mocked(api.fetchIMConfig).mockResolvedValue(config);
        if (method === "fetchIMStatus") {
          vi.mocked(api.fetchIMStatus).mockImplementationOnce(async () => {
            await pending;
            return status;
          });
        } else if (method === "fetchIMConfig") {
          vi.mocked(api.fetchIMConfig).mockImplementationOnce(async () => {
            await pending;
            return config;
          });
        } else {
          vi.mocked(api[method]).mockImplementationOnce(async () => {
            await pending;
            return { ok: true, result: {} };
          });
        }
        let running!: Promise<void>;
        await act(async () => {
          running = invoke(phase === "status" || phase === "config" ? "save" : phase);
        });
        expect(props().imBusy).toBe(true);
        await act(async () => {
          if (field === "url") props().setImMattermostUrl("https://edited.example.test");
          else props().setImBotTokenEnv("EDITED_TOKEN");
        });
        const before = { url: props().imMattermostUrl, token: props().imBotTokenEnv };
        const reads = vi.mocked(api.fetchIMConfig).mock.calls.length;
        await act(async () => {
          release();
          await running;
        });
        expect(props().imMattermostUrl).toBe(before.url);
        expect(props().imBotTokenEnv).toBe(before.token);
        expect(props().imPlatform).toBe("mattermost");
        expect(props().imBusy).toBe(false);
        expect(props().imStatus).toEqual(status.result);
        expect(api.fetchIMConfig).toHaveBeenCalledTimes(reads);
      }
    },
  );

  it.each(["save", "start-save", "start", "stop", "remove"])(
    "does not send new same-group mutations before the old %s has finished",
    async (oldAction) => {
      for (const visit of ["platform", "return", "remount"] as const) {
        for (const newAction of ["save", "stop", "remove"] as const) {
          await renderGroup(`ordering-${oldAction}-${visit}-${newAction}`);
          await choose("mattermost");
          const order: string[] = [];
          let release!: () => void;
          const gate = new Promise<void>((resolve) => {
            release = resolve;
          });
          const method =
            oldAction === "start"
              ? "startIMBridge"
              : oldAction === "stop"
                ? "stopIMBridge"
                : oldAction === "remove"
                  ? "unsetIMConfig"
                  : "setIMConfig";
          vi.mocked(api[method]).mockImplementationOnce(async () => {
            order.push("old-sent");
            await gate;
            order.push("old-completed");
            return { ok: true, result: {} };
          });
          let first!: Promise<void>;
          await act(async () => {
            first = invoke(oldAction);
          });
          expect(order).toEqual(["old-sent"]);
          if (visit === "remount") {
            await act(async () => root.render(null));
            await renderGroup(`ordering-${oldAction}-${visit}-${newAction}`);
            await choose("mattermost");
          } else {
            await choose("telegram");
            if (visit === "return") await choose("mattermost");
          }
          const nextMethod =
            newAction === "save"
              ? "setIMConfig"
              : newAction === "stop"
                ? "stopIMBridge"
                : "unsetIMConfig";
          vi.mocked(api[nextMethod]).mockImplementationOnce(async () => {
            order.push("new-sent");
            return { ok: true, result: {} };
          });
          let second!: Promise<void>;
          await act(async () => {
            second = invoke(newAction);
          });
          expect.soft(order).toEqual(["old-sent"]);
          await act(async () => {
            release();
            await Promise.all([first, second]);
          });
          expect(order).toEqual(["old-sent", "old-completed", "new-sent"]);
          expect(props().imBusy).toBe(false);
        }
      }
    },
  );

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

  it.each(["status", "config"] as const)(
    "keeps user choices and Mattermost edits while initial %s is pending",
    async (phase) => {
      for (const edit of ["none", "other", "mattermost", "return", "url", "token"] as const) {
        let release!: () => void;
        const pending = new Promise<void>((resolve) => {
          release = resolve;
        });
        const status = {
          ok: true as const,
          result: {
            group_id: "group-a",
            configured: true,
            running: false,
            enabled: false,
            platform: "mattermost",
            subscribers: 0,
          },
        };
        const config = {
          ok: true as const,
          result: {
            im: {
              platform: "mattermost" as const,
              bot_token_env: "STORED_TOKEN",
              mattermost_url: "https://stored.example.test",
            },
          },
        };
        vi.mocked(api.fetchIMStatus).mockImplementationOnce(async () => {
          if (phase === "status") await pending;
          return status;
        });
        vi.mocked(api.fetchIMConfig).mockImplementationOnce(async () => {
          if (phase === "config") await pending;
          return config;
        });
        await renderGroup(`initial-${phase}-${edit}`);
        if (edit === "other")
          await choose(props().imPlatform === "mattermost" ? "telegram" : "slack");
        if (edit === "mattermost" || edit === "return") {
          if (edit === "return") await choose("mattermost");
          await choose("telegram");
          await choose("mattermost");
        }
        if (edit === "url" || edit === "token") {
          await choose("mattermost");
          await act(async () => {
            if (edit === "url") props().setImMattermostUrl("https://edited.example.test");
            else props().setImBotTokenEnv("EDITED_TOKEN");
          });
        }
        const before = {
          platform: props().imPlatform,
          url: props().imMattermostUrl,
          token: props().imBotTokenEnv,
          status: props().imStatus,
        };
        await act(async () => {
          release();
          await pending;
        });
        if (edit === "none") {
          expect(props().imPlatform).toBe("mattermost");
          expect(props().imMattermostUrl).toBe("https://stored.example.test");
          expect(props().imBotTokenEnv).toBe("STORED_TOKEN");
        } else {
          expect.soft(props().imPlatform).toBe(before.platform);
          expect.soft(props().imMattermostUrl).toBe(before.url);
          expect.soft(props().imBotTokenEnv).toBe(before.token);
          expect.soft(props().imStatus).toEqual(before.status);
        }
        expect(props().imBusy).toBe(false);
        expect(api.setIMConfig).not.toHaveBeenCalled();
        expect(api.startIMBridge).not.toHaveBeenCalled();
        // A cancelled status read leaves the config mock unused; remove it before the next scenario.
        vi.mocked(api.fetchIMConfig)
          .mockReset()
          .mockResolvedValue({ ok: true, result: { im: null } });
      }
    },
  );

  it("preserves native initial hydration between legacy platforms", async () => {
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    vi.mocked(api.fetchIMConfig).mockImplementationOnce(async () => {
      await pending;
      return { ok: true, result: { im: { platform: "telegram", bot_token_env: "LEGACY_STORED" } } };
    });
    await renderGroup("legacy-initial");
    await choose("slack");
    await act(async () => {
      release();
      await pending;
    });
    expect(props().imPlatform).toBe("telegram");
    expect(props().imBotTokenEnv).toBe("LEGACY_STORED");
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
          // The new Group still has a pending save; the old finally must not clear its busy state.
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
            if (outcome === "transport") reject(new Error("Old Group transport failure"));
            else
              release(
                outcome === "rejected"
                  ? { ok: false, error: { code: "old_failure", message: "Old Group error" } }
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
        error: { code: "save_failed", message: "Save failed. Try again." },
      });
      const reads = vi.mocked(api.fetchIMConfig).mock.calls.length;
      await act(async () => props().onStartBridge());
      expect(api.startIMBridge).not.toHaveBeenCalled();
      expect(api.fetchIMConfig).toHaveBeenCalledTimes(reads);
      expect(props().imPlatform).toBe("mattermost");
      expect(props().imMattermostUrl).toBe("https://draft.example.test");
      expect(props().imBotTokenEnv).toBe("DRAFT_TOKEN");
      expect(props().imConfigError).toBe("Save failed. Try again.");

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
        error: { code: "connect_failed", message: "Connection failed" },
      });
      await act(async () => props().onStartBridge());
      expect(api.startIMBridge).toHaveBeenCalledOnce();
      expect(api.fetchIMConfig).toHaveBeenCalledTimes(reads + 1);
      expect(props().imConfigError).toBe("Connection failed");
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

    // Keep SettingsModal mounted and change only groupId to cover switches without closing the modal.
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
