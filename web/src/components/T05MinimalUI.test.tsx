// @vitest-environment happy-dom
import { act, useEffect, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createInstance } from "i18next";
import { I18nextProvider } from "react-i18next";
import { beforeEach, afterEach, describe, expect, it, vi } from "vite-plus/test";
import settings from "../i18n/locales/zh/settings.json";
import actors from "../i18n/locales/zh/actors.json";
import common from "../i18n/locales/zh/common.json";
import WebModelConnectorsTab from "./modals/settings/WebModelConnectorsTab";
import { GroupMembersMenu } from "./layout/GroupMembersMenu";
import type { Actor } from "../types";
import { useModalA11y } from "../hooks/useModalA11y";

const mocks = vi.hoisted(() => ({
  sharedWebModelBrowser: vi.fn(),
  fetchGroups: vi.fn(),
  fetchActors: vi.fn(),
  fetchWebModelConnectors: vi.fn(),
  fetchRemoteAccessState: vi.fn(),
  fetchWebModelBrowserSession: vi.fn(),
  fetchWebModelBrowserSurfaceSession: vi.fn(),
  openWebModelBrowserSurfaceSession: vi.fn(),
  closeWebModelBrowserSurfaceSession: vi.fn(),
  createWebModelConnector: vi.fn(),
  revokeWebModelConnector: vi.fn(),
  bindCurrentWebModelBrowserConversation: vi.fn(),
  fetchRuntimes: vi.fn(),
  copy: vi.fn(),
  previewStart: vi.fn(),
  openModal: vi.fn(),
  setRole: vi.fn(),
  setRuntime: vi.fn(),
  setCommand: vi.fn(),
  group: { selectedGroupId: "g_a", actors: [] as unknown[], setRuntimes: vi.fn() },
  showError: vi.fn(),
}));
vi.mock("../services/api", () => ({
  ...mocks,
  getWebModelBrowserSurfaceWebSocketUrl: () => "ws://local.invalid/preview",
  getSharedWebModelBrowserWebSocketUrl: () => "ws://local.invalid/shared-preview",
}));
vi.mock("../utils/copy", () => ({ copyTextToClipboard: mocks.copy }));
vi.mock("../stores", () => ({
  useFormStore: {
    getState: () => ({
      setNewActorRole: mocks.setRole,
      setEditActorRuntime: mocks.setRuntime,
      setEditActorCommand: mocks.setCommand,
    }),
  },
  useGroupStore: { getState: () => mocks.group },
  useModalStore: { getState: () => ({ openModal: mocks.openModal }) },
  useUIStore: { getState: () => ({ showError: mocks.showError }) },
}));
vi.mock("./browser/ProjectedBrowserSurfacePanel", () => ({
  ProjectedBrowserSurfacePanel: ({
    loadSession,
    startSession,
  }: {
    loadSession: () => Promise<unknown>;
    startSession?: unknown;
  }) => {
    useEffect(() => {
      mocks.previewStart(startSession);
      void loadSession();
    }, [loadSession, startSession]);
    return <div data-testid="native-preview">Preview</div>;
  },
}));
const ok = <T,>(result: T) => ({ ok: true as const, result });
const runtimeResponse = ok({
  runtimes: [{ name: "opencode", available: true, recommended_command: "opencode" }],
});
const lead = {
  id: "lead",
  title: "组长甲",
  runtime: "web_model",
  role: "foreman",
  enabled: true,
  running: false,
} as Actor;
const other = { ...lead, title: "组长乙" };
const session = (gid: string) => ({
  active: true,
  ready: true,
  login_required: false,
  tab_url: "https://chatgpt.com/c/other-live-chat",
  conversation_url: `https://chatgpt.com/c/${gid}`,
  delivery_target: {
    kind: "existing_chat",
    state: "bound_existing_chat",
    url: `https://chatgpt.com/c/${gid}`,
  },
});
let host: HTMLDivElement;
let root: Root;
const wait = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
const find = (selector: string) => {
  const el = document.querySelector<HTMLElement>(selector);
  expect(el, selector).not.toBeNull();
  return el!;
};
const click = async (selector: string) => {
  await act(async () => find(selector).click());
  await wait();
};
const clickText = async (text: string) => {
  const el = [...host.querySelectorAll("button")].find((item) =>
    item.textContent?.startsWith(text),
  );
  expect(el, text).toBeTruthy();
  await act(async () => el!.click());
  await wait();
};
const groupTrigger = () => find('#t05-web-group [role="combobox"]');
const choose = async (gid: string) => {
  const names: Record<string, string> = { g_a: "甲组", g_b: "乙组", g_empty: "空组" };
  await click('#t05-web-group [role="combobox"]');
  const option = [...document.querySelectorAll<HTMLElement>('[role="option"]')].find(
    (item) => item.textContent === names[gid],
  );
  expect(option, `missing group ${gid}`).toBeTruthy();
  await act(async () => option!.click());
  await wait();
};
const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { resolve, promise };
};
async function render(
  content: ReactNode = <WebModelConnectorsTab isDark={false} currentGroupId="g_a" />,
) {
  const i18n = createInstance();
  await i18n.init({
    lng: "zh",
    resources: { zh: { settings, actors, common } },
    interpolation: { escapeValue: false },
  });
  await act(async () => root.render(<I18nextProvider i18n={i18n}>{content}</I18nextProvider>));
  await wait();
}
beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal(
    "confirm",
    vi.fn(() => true),
  );
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  mocks.group.selectedGroupId = "g_a";
  mocks.group.actors = [lead];
  mocks.fetchGroups.mockResolvedValue(
    ok({
      groups: [
        { group_id: "g_a", title: "甲组" },
        { group_id: "g_b", title: "乙组" },
        { group_id: "g_empty", title: "空组" },
      ],
    }),
  );
  mocks.fetchActors.mockImplementation(async (gid: string) =>
    ok({ actors: gid === "g_empty" ? [] : [gid === "g_a" ? lead : other] }),
  );
  mocks.fetchWebModelConnectors.mockResolvedValue(
    ok({
      connectors: ["g_a", "g_b"].map((gid) => ({
        connector_id: `conn-${gid}`,
        group_id: gid,
        actor_id: "lead",
        session_bound: true,
        connector_url_with_token: `https://example.invalid/mcp/${gid}`,
      })),
    }),
  );
  mocks.fetchRemoteAccessState.mockResolvedValue(ok({ remote_access: { config: {} } }));
  mocks.fetchWebModelBrowserSession.mockImplementation(async (gid: string) =>
    ok({ browser_session: session(gid) }),
  );
  mocks.fetchWebModelBrowserSurfaceSession.mockImplementation(async (gid: string) =>
    ok({ browser_session: session(gid), browser_surface: { active: true, state: "ready" } }),
  );
  mocks.sharedWebModelBrowser.mockResolvedValue(
    ok({
      browser_session: {
        active: true,
        ready: true,
        login_required: false,
        tab_url: "https://chatgpt.com/c/shared-live",
      },
      browser_surface: { active: true, state: "ready" },
    }),
  );
  mocks.revokeWebModelConnector.mockResolvedValue(ok({ revoked: true }));
  mocks.bindCurrentWebModelBrowserConversation.mockResolvedValue(
    ok({ browser_session: session("g_a") }),
  );
  mocks.copy.mockResolvedValue(true);
  mocks.fetchRuntimes.mockResolvedValue(runtimeResponse);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("minimal overlay on upstream UI", () => {
  it("keeps one ordered setup, copies the native URL and switches groups without binding", async () => {
    await render();
    const labels = ["1. 登录 ChatGPT", "2. 选择工作组", "3. 连接 CCCC MCP app", "4. 选择投递目标"];
    const steps = ["account", "group", "connection", "target"].map((step, i) => {
      const element = find(`[data-setup-step="${step}"]`);
      expect(element.textContent).toContain(labels[i]);
      return element;
    });
    for (let i = 1; i < steps.length; i++) {
      expect(
        steps[i - 1].compareDocumentPosition(steps[i]) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
    }
    expect(groupTrigger().getAttribute("aria-label")).toBe("选择工作组");
    expect(host.querySelector('[data-t05-change="copy-binding"]')).toBeNull();
    await clickText("复制 MCP URL");
    expect(mocks.copy).toHaveBeenCalledExactlyOnceWith("https://example.invalid/mcp/g_a");
    await choose("g_b");
    expect((find('input[placeholder="https://chatgpt.com/c/..."]') as HTMLInputElement).value).toBe(
      "https://chatgpt.com/c/g_b",
    );
    await clickText("刷新");
    expect(groupTrigger().textContent).toContain("乙组");
    await choose("g_empty");
    expect(host.textContent).toContain("本组没有网页成员");
    expect(host.textContent).not.toContain("chatgpt.com/c/g_b");
    expect(mocks.openWebModelBrowserSurfaceSession).not.toHaveBeenCalled();
    expect(mocks.bindCurrentWebModelBrowserConversation).not.toHaveBeenCalled();
  });

  it("discards a late actor response after switching groups", async () => {
    const delayedActors = deferred<ReturnType<typeof ok<{ actors: Actor[] }>>>();
    mocks.fetchActors.mockImplementation((gid: string) =>
      gid === "g_a" ? delayedActors.promise : Promise.resolve(ok({ actors: [other] })),
    );
    await render();
    await choose("g_b");
    await act(async () => delayedActors.resolve(ok({ actors: [lead] })));
    await wait();
    expect(host.textContent).not.toContain("组长甲 的 ChatGPT");
    expect(groupTrigger().textContent).toContain("乙组");
  });
  it("keeps preview read-only and cancels shared-browser actions for all groups", async () => {
    await render();
    await click('[data-t05-change="preview-toggle"]');
    expect(mocks.previewStart).toHaveBeenCalledWith(undefined);
    vi.mocked(window.confirm).mockReturnValue(false);
    for (const [action, warning] of [
      ["restart", "所有组"],
      ["close", "所有使用它的工作组"],
    ]) {
      await click(`[data-t05-change="${action}-shared-browser"]`);
      expect(window.confirm).toHaveBeenLastCalledWith(expect.stringContaining(warning));
    }
    await click('[data-t05-change="preview-toggle"]');
    expect(document.querySelector('[data-testid="native-preview"]')).toBeNull();
    expect(mocks.openWebModelBrowserSurfaceSession).not.toHaveBeenCalled();
    expect(mocks.closeWebModelBrowserSurfaceSession).not.toHaveBeenCalled();
    expect(
      mocks.sharedWebModelBrowser.mock.calls.some(
        ([action]) => action === "open" || action === "close",
      ),
    ).toBe(false);
  });
  it("keeps access setup and shared login usable with no groups", async () => {
    mocks.fetchGroups.mockResolvedValue(ok({ groups: [] }));
    const openAccess = vi.fn();
    await render(<WebModelConnectorsTab isDark={false} onOpenWebAccess={openAccess} />);
    const access = find('[data-testid="web-access-prerequisite"]');
    const account = find('[data-setup-step="account"]');
    const group = find('[data-t05-change="web-group-selector"]');
    expect(access.closest("details")).toBeNull();
    expect(access.compareDocumentPosition(account) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(account.compareDocumentPosition(group) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await clickText("打开 Web Access");
    expect(openAccess).toHaveBeenCalledOnce();
    expect(mocks.sharedWebModelBrowser.mock.calls.some(([action]) => action === "open")).toBe(
      false,
    );
    await click('[data-t05-change="open-shared-browser"]');
    expect(mocks.sharedWebModelBrowser).toHaveBeenCalledWith("open", {
      width: 1366,
      height: 900,
      inspect: true,
    });
    expect(mocks.openWebModelBrowserSurfaceSession).not.toHaveBeenCalled();
    expect(mocks.createWebModelConnector).not.toHaveBeenCalled();
  });
  it("keeps original direct setup controls visible and only folds the instructions; ready Web Access is not falsely missing", async () => {
    mocks.fetchWebModelConnectors.mockResolvedValue(ok({ connectors: [] }));
    mocks.fetchRemoteAccessState.mockResolvedValue(
      ok({
        remote_access: {
          config: { web_public_url: "https://example.invalid", access_token_configured: true },
        },
      }),
    );
    await render();
    expect(host.querySelector('[data-testid="web-access-prerequisite"]')).toBeNull();
    const button = [...host.querySelectorAll("button")].find(
      (item) => item.textContent === "创建 MCP URL",
    )!;
    expect(button.closest("details")).toBeNull();
    expect(button.disabled).toBe(false);
    const note = find('[data-t05-change="legacy-setup"]') as HTMLDetailsElement;
    expect(note.open).toBe(false);
    expect(note.textContent).toContain("权限");
    await act(async () => note.querySelector("summary")!.click());
    expect(note.open).toBe(true);
    expect(host.querySelectorAll('input[name="chatgpt-delivery-target"]').length).toBe(2);
  });
});
describe("group members shortcut", () => {
  it("opens native member details and the native add dialog rather than managing members in global settings", async () => {
    const inspect = vi.fn();
    const edit = vi.fn();
    await render(
      <GroupMembersMenu
        groupId="g_a"
        actors={[lead]}
        readOnly={false}
        onOpenActor={inspect}
        onEditActor={edit}
      />,
    );
    await click('[data-t05-change="members-entry"]');
    await click('[data-t05-change="member-details"]');
    expect(inspect).toHaveBeenCalledWith("lead");
    await click('[data-t05-change="members-entry"]');
    await click('[data-t05-change="add-member"]');
    expect(mocks.openModal).toHaveBeenCalledWith("addActor");
    expect(mocks.setRole).toHaveBeenCalledWith("peer");
    expect(edit).not.toHaveBeenCalled();
    await click('[data-t05-change="members-entry"]');
    await click('[data-t05-change="change-foreman"]');
    await click('[data-t05-change="foreman-local"]');
    expect(edit).toHaveBeenCalledWith(lead);
    expect(mocks.setRuntime).toHaveBeenCalledWith("opencode");
    const pending = deferred<typeof runtimeResponse>();
    mocks.fetchRuntimes.mockReturnValue(pending.promise);
    await click('[data-t05-change="members-entry"]');
    await click('[data-t05-change="change-foreman"]');
    await click('[data-t05-change="foreman-local"]');
    mocks.group.selectedGroupId = "g_b";
    await act(async () => pending.resolve(runtimeResponse));
    await wait();
    expect(edit).toHaveBeenCalledTimes(1);
  });
});

describe("shared login, role, and confirmation ownership", () => {
  it.each([false, true])(
    "binds target newChat=%s only after confirmation, preserving the draft on cancel",
    async (newChat) => {
      await render();
      const url = find('input[placeholder="https://chatgpt.com/c/..."]') as HTMLInputElement;
      const radio = host.querySelectorAll<HTMLInputElement>(
        'input[name="chatgpt-delivery-target"]',
      )[1];
      expect(url.value).toBe("https://chatgpt.com/c/g_a");
      if (newChat) {
        await act(async () => radio.click());
        await wait();
      } else {
        await click('[data-testid="use-current-browser-chat"]');
        expect(url.value).toBe("https://chatgpt.com/c/shared-live");
      }
      expect(window.confirm).not.toHaveBeenCalled();
      expect(mocks.bindCurrentWebModelBrowserConversation).not.toHaveBeenCalled();
      vi.mocked(window.confirm).mockReturnValue(false);
      await click('[data-t05-change="save-return-target"]');
      expect(mocks.bindCurrentWebModelBrowserConversation).not.toHaveBeenCalled();
      if (newChat) {
        expect(window.confirm).toHaveBeenCalledWith(
          expect.stringContaining("https://chatgpt.com/c/g_a"),
        );
        expect(radio.checked).toBe(true);
        await choose("g_b");
        expect(groupTrigger().textContent).toContain("甲组");
      } else {
        expect(url.value).toBe("https://chatgpt.com/c/shared-live");
      }
      vi.mocked(window.confirm).mockReturnValue(true);
      await click('[data-t05-change="save-return-target"]');
      expect(mocks.bindCurrentWebModelBrowserConversation).toHaveBeenCalledExactlyOnceWith(
        expect.objectContaining({
          groupId: "g_a",
          actorId: "lead",
          newChat,
          ...(!newChat && { conversationUrl: "https://chatgpt.com/c/shared-live" }),
        }),
      );
      expect(
        mocks.sharedWebModelBrowser.mock.calls.some(
          ([action]) => action === "open" || action === "close",
        ),
      ).toBe(false);
    },
  );

  it("shows a web peer under a local leader and never takes group connection status as shared login", async () => {
    const peer = { ...lead, id: "web-peer", role: "peer", title: "网页组员" } as Actor;
    mocks.fetchActors.mockImplementation(async (gid: string) =>
      ok({
        actors: gid === "g_a" ? [lead] : [{ ...lead, id: "local-lead", runtime: "opencode" }, peer],
      }),
    );
    mocks.fetchWebModelBrowserSession.mockResolvedValue(
      ok({ browser_session: { active: false, ready: false, login_required: true } }),
    );
    await render();
    const before = find('[data-testid="shared-login-status"]').textContent;
    await choose("g_b");
    expect(find('[data-testid="shared-login-status"]').textContent).toBe(before);
    expect(find('[data-testid="web-member-role"]').textContent).toContain("网页组员 · 组员");
    expect(
      mocks.sharedWebModelBrowser.mock.calls.every(
        (call) => call.length === 0 || call[0] === "status",
      ),
    ).toBe(true);
  });
  it("requires confirmation before disconnecting exactly one member, without logging out the shared browser", async () => {
    await render();
    await choose("g_b");
    vi.mocked(window.confirm).mockReturnValue(false);
    await click('[data-t05-change="disconnect-chat"]');
    expect(window.confirm).toHaveBeenLastCalledWith(expect.stringContaining("乙组"));
    expect(mocks.revokeWebModelConnector).not.toHaveBeenCalled();
    vi.mocked(window.confirm).mockReturnValue(true);
    await click('[data-t05-change="disconnect-chat"]');
    expect(mocks.revokeWebModelConnector).toHaveBeenCalledExactlyOnceWith("conn-g_b");
    expect(mocks.sharedWebModelBrowser.mock.calls.some((call) => call[0] === "close")).toBe(false);
  });
});

describe("group picker inside settings", () => {
  it("uses the first Escape for the group menu and only the second for settings", async () => {
    const onClose = vi.fn();
    function Dialog() {
      const { modalRef } = useModalA11y(true, onClose);
      return (
        <div ref={modalRef}>
          <WebModelConnectorsTab isDark={false} currentGroupId="g_a" />
        </div>
      );
    }
    await render(<Dialog />);
    await click('#t05-web-group [role="combobox"]');
    expect(document.querySelector('[role="option"]')).not.toBeNull();
    await act(async () =>
      document.activeElement!.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      ),
    );
    await wait();
    expect(document.querySelector('[role="option"]')).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    await act(async () =>
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      ),
    );
    expect(onClose).toHaveBeenCalledOnce();
  });
});
