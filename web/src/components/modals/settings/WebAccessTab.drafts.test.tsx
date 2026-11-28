// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import * as api from "../../../services/api";
import type { RemoteAccessState } from "../../../types";
import { WebAccessTab } from "./WebAccessTab";

vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  const i18n = { language: "en", resolvedLanguage: "en" };
  return { useTranslation: () => ({ t, i18n }) };
});
vi.mock("../../../services/api", async (original) => ({
  ...(await original<typeof import("../../../services/api")>()),
  fetchWebAccessSession: vi.fn(async () => ({
    ok: true,
    result: { web_access_session: { can_access_global_settings: true } },
  })),
  fetchMembership: vi.fn(async () => ({ ok: true, result: { membership: { logged_in: false } } })),
  fetchGroups: vi.fn(async () => ({ ok: true, result: { groups: [] } })),
  fetchAccessTokens: vi.fn(),
  createAccessToken: vi.fn(),
  deleteAccessToken: vi.fn(),
  fetchRemoteAccessState: vi.fn(),
  updateRemoteAccessConfig: vi.fn(),
}));

const localState: RemoteAccessState = {
  provider: "off",
  mode: "tailnet_only",
  require_access_token: true,
  enabled: false,
  status: "stopped",
  config: { web_host: "127.0.0.1", web_port: 8848, web_public_url: "" },
};
const privateState: RemoteAccessState = {
  ...localState,
  provider: "manual",
  config: { ...localState.config, web_host: "0.0.0.0" },
};
const adminEntry = {
  token_id: "admin",
  user_id: "owner",
  is_admin: true,
  allowed_groups: [],
  created_at: "2026-09-18T00:00:00Z",
};

describe("Web Access connection drafts", () => {
  let root: ReturnType<typeof createRoot>;
  let host: HTMLDivElement;
  let saved: RemoteAccessState;

  beforeEach(() => {
    vi.clearAllMocks();
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    saved = localState;
    vi.mocked(api.fetchAccessTokens).mockResolvedValue({
      ok: true,
      result: { access_tokens: [adminEntry] },
    });
    vi.mocked(api.fetchRemoteAccessState).mockImplementation(async () => ({
      ok: true,
      result: { remote_access: { ...saved } },
    }));
    vi.mocked(api.updateRemoteAccessConfig).mockImplementation(async (draft) => {
      saved = {
        ...saved,
        provider: draft.provider ?? saved.provider,
        mode: draft.mode ?? saved.mode,
        config: {
          web_host: draft.webHost,
          web_port: draft.webPort,
          web_public_url: draft.webPublicUrl,
        },
      };
      return { ok: true, result: { remote_access: saved } };
    });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  const render = async (view: "overview" | "tokens" = "overview") => {
    await act(async () => root.render(<WebAccessTab isDark={false} onOpenAccount={() => {}} />));
    if (view === "tokens") await click("webAccess.accessControlTitle");
  };
  const button = (text: string) => {
    const match = [...host.querySelectorAll("button")].find((item) =>
      item.textContent?.includes(text),
    );
    expect(match, text).toBeDefined();
    return match!;
  };
  const click = async (text: string) => {
    await act(async () => button(text).click());
  };
  const choose = async (goal: string) => click(`webAccess.goals.${goal}.title`);
  const refresh = async () => click("webAccess.refresh");

  it("distinguishes the selected draft from the saved access goal", async () => {
    await render();
    const local = button("webAccess.goals.local.title");
    const lan = button("webAccess.goals.lan.title");
    expect(local.getAttribute("aria-pressed")).toBe("true");
    expect(local.querySelector("[data-settings-choice-saved]")).not.toBeNull();

    await choose("lan");
    expect(lan.getAttribute("aria-pressed")).toBe("true");
    expect(local.getAttribute("aria-pressed")).toBe("false");
    expect(local.querySelector("[data-settings-choice-saved]")).not.toBeNull();
    expect(lan.querySelector("[data-settings-choice-saved]")).toBeNull();

    await click("webAccess.saveChanges");
    expect(local.querySelector("[data-settings-choice-saved]")).toBeNull();
    expect(lan.querySelector("[data-settings-choice-saved]")).not.toBeNull();
  });

  it("opens the access-token dialog in a portal", async () => {
    await render("tokens");
    await click("common:create");
    const dialog = document.querySelector<HTMLElement>(
      '[role="dialog"][aria-label="webAccess.createAccessToken"]',
    );
    expect(dialog).not.toBeNull();
    expect(host.contains(dialog)).toBe(false);
  });

  it("opens and reopens on the overview and keeps token loading and access controls reachable", async () => {
    let finish!: (value: Awaited<ReturnType<typeof api.fetchAccessTokens>>) => void;
    vi.mocked(api.fetchAccessTokens).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await render();
    const tokens = button("webAccess.accessControlTitle");
    const overview = button("webAccess.overviewTab");
    expect(overview.getAttribute("aria-selected")).toBe("true");
    expect(tokens.getAttribute("role")).toBe("tab");
    await act(async () => tokens.click());
    expect(tokens.getAttribute("aria-selected")).toBe("true");
    const panel = [...host.querySelectorAll<HTMLElement>('[role="tabpanel"]')].find(
      (panel) => panel.getAttribute("aria-labelledby") === tokens.id,
    )!;
    expect(panel.hidden).toBe(false);
    expect(panel.querySelector("details")).toBeNull();
    expect(panel.textContent).toContain("common:loading");
    expect(panel.textContent).not.toContain("webAccess.noAccessTokens");
    await act(async () => finish({ ok: true, result: { access_tokens: [adminEntry] } }));
    expect(panel.textContent).toContain("owner");
    await act(async () => overview.click());
    expect(overview.getAttribute("aria-selected")).toBe("true");
    expect(panel.hidden).toBe(true);
    await act(async () => tokens.click());
    expect(panel.hidden).toBe(false);

    await act(async () => root.render(null));
    await render();
    expect(button("webAccess.overviewTab").getAttribute("aria-selected")).toBe("true");
  });

  it("pages tokens and keeps a valid page after deleting or creating an entry", async () => {
    let entries = Array.from({ length: 11 }, (_, index) => ({
      token_id: `token-${index}`,
      user_id: index === 0 ? "owner" : `reader-${String(index).padStart(2, "0")}`,
      is_admin: index === 0,
      allowed_groups: [],
      token_preview: "masked",
      created_at: "2026-09-18T00:00:00Z",
    }));
    vi.mocked(api.fetchAccessTokens).mockImplementation(async () => ({
      ok: true,
      result: { access_tokens: entries },
    }));
    vi.mocked(api.deleteAccessToken).mockImplementation(async (id) => {
      entries = entries.filter((entry) => entry.token_id !== id);
      return {
        ok: true,
        result: { deleted: true, access_tokens_remain: true, deleted_current_session: false },
      };
    });
    vi.mocked(api.createAccessToken).mockImplementation(async (userId) => {
      const created = { ...entries[0], token_id: "created", user_id: userId };
      entries = [...entries, created];
      return { ok: true, result: { access_token: { ...created, token: "synthetic-new-token" } } };
    });
    await render("tokens");
    const panel = () =>
      [...host.querySelectorAll<HTMLElement>('[role="tabpanel"]')].find((panel) => !panel.hidden)!;
    expect(panel().textContent).toContain("reader-09");
    expect(panel().textContent).not.toContain("reader-10");
    expect(button("capabilityCenter.pagination.prev").disabled).toBe(true);
    await click("capabilityCenter.pagination.next");
    expect(panel().textContent).toContain("reader-10");
    expect(panel().textContent).not.toContain("reader-09");
    expect(button("capabilityCenter.pagination.next").disabled).toBe(true);

    await click("webAccess.delete");
    const confirm = [...panel().querySelectorAll<HTMLButtonElement>("button")]
      .filter((button) => button.textContent === "webAccess.delete")
      .at(-1)!;
    await act(async () => confirm.click());
    expect(api.deleteAccessToken).toHaveBeenCalledWith("token-10");
    expect(panel().textContent).toContain("owner");
    expect(panel().querySelector('nav[aria-label="webAccess.accessControlTitle"]')).toBeNull();

    await act(async () => root.render(null));
    await render("tokens");
    expect(panel().querySelector('nav[aria-label="webAccess.accessControlTitle"]')).toBeNull();

    await click("common:create");
    const dialog = document.querySelector<HTMLElement>(
      '[role="dialog"][aria-label="webAccess.createAccessToken"]',
    )!;
    await act(async () => {
      const input = dialog.querySelector<HTMLInputElement>(
        '[placeholder="webAccess.userIdPlaceholder"]',
      )!;
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
        input,
        "new-owner",
      );
      input.dispatchEvent(new Event("input", { bubbles: true }));
      dialog.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click();
    });
    const create = [...dialog.querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.textContent === "webAccess.createAccessToken",
    )!;
    await act(async () => create.click());
    expect(api.createAccessToken).toHaveBeenCalledWith("new-owner", true, [], undefined, undefined);
    const close = [...dialog.querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.textContent === "webAccess.close",
    )!;
    await act(async () => close.click());
    expect(panel().textContent).toContain("1 / 2");
    await click("capabilityCenter.pagination.next");
    expect(panel().textContent).toContain("new-owner");

    await act(async () =>
      root.render(<WebAccessTab isDark={false} focusReach onOpenAccount={() => {}} />),
    );
    expect(button("webAccess.overviewTab").getAttribute("aria-selected")).toBe("true");
  });

  it("opens the restart instructions when a saved change needs a restart", async () => {
    vi.mocked(api.updateRemoteAccessConfig).mockResolvedValue({
      ok: true,
      result: {
        remote_access: { ...privateState, restart_required: true, apply_supported: false },
      },
    });
    await render();
    await choose("lan");
    await click("webAccess.saveChanges");
    const dialog = document.querySelector<HTMLElement>(
      '[role="dialog"][aria-label="webAccess.restartDialogTitle"]',
    );
    expect(dialog).not.toBeNull();
    expect(host.contains(dialog)).toBe(false);
  });

  it.each(["lan", "public"])(
    "keeps a pending %s goal through refresh and saves its connection",
    async (goal) => {
      await render();
      await choose(goal);
      if (goal === "public") {
        const input = host.querySelector<HTMLInputElement>(
          'input[placeholder="https://example.com/ui/"]',
        )!;
        await act(async () => {
          Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
            input,
            "https://workspace.example/ui/",
          );
          input.dispatchEvent(new Event("input", { bubbles: true }));
        });
      }
      await refresh();
      await click("webAccess.saveChanges");
      expect(api.updateRemoteAccessConfig).toHaveBeenCalledWith(
        expect.objectContaining({
          provider: "manual",
          webHost: goal === "lan" ? "0.0.0.0" : "127.0.0.1",
          webPublicUrl: goal === "public" ? "https://workspace.example/ui/" : "",
        }),
      );
      expect(host.querySelector('[role="alert"]')).toBeNull();
    },
  );

  it("keeps a local-only draft when the saved configuration is private", async () => {
    saved = privateState;
    await render();
    await choose("local");
    await refresh();
    await click("webAccess.saveChanges");
    expect(api.updateRemoteAccessConfig).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: "off",
        webHost: "127.0.0.1",
        webPublicUrl: "",
        enabled: false,
      }),
    );
  });

  it("follows remote changes when clean, including after a successful save", async () => {
    await render();
    saved = privateState;
    await refresh();
    expect(host.querySelector('[role="combobox"]')?.getAttribute("aria-label")).toBe(
      "webAccess.providerLabel",
    );
    await choose("local");
    await click("webAccess.saveChanges");
    expect(saved.provider).toBe("off");
    saved = privateState;
    await refresh();
    expect(host.querySelector('[role="combobox"]')?.getAttribute("aria-label")).toBe(
      "webAccess.providerLabel",
    );
  });

  it("still requires an administrator token for an unsaved private connection after refresh", async () => {
    vi.mocked(api.fetchAccessTokens).mockResolvedValue({ ok: true, result: { access_tokens: [] } });
    await render();
    await choose("lan");
    expect(button("webAccess.saveChanges").disabled).toBe(true);
    await refresh();
    expect(button("webAccess.saveChanges").disabled).toBe(true);
    expect(api.updateRemoteAccessConfig).not.toHaveBeenCalled();
  });

  it("does not let a stale in-flight refresh overwrite a newer saved configuration", async () => {
    await render();
    let resolveStaleRefresh!: (value: {
      ok: true;
      result: { remote_access: RemoteAccessState };
    }) => void;
    vi.mocked(api.fetchRemoteAccessState).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveStaleRefresh = resolve;
        }),
    );

    // Start a slow refresh while still on localState
    await act(async () => {
      button("webAccess.refresh").click();
    });

    // User switches to LAN and saves while the old refresh is still in flight
    await choose("lan");
    await click("webAccess.saveChanges");
    expect(saved.provider).toBe("manual");
    expect(host.querySelector('[role="combobox"]')?.getAttribute("aria-label")).toBe(
      "webAccess.providerLabel",
    );

    // Now the stale pre-save refresh resolves with localState ("off")
    await act(async () => {
      resolveStaleRefresh({ ok: true, result: { remote_access: { ...localState } } });
    });

    // The UI must remain on the newly saved LAN configuration ("manual"), not revert to local-only
    expect(host.querySelector('[role="combobox"]')?.getAttribute("aria-label")).toBe(
      "webAccess.providerLabel",
    );
  });

  it("shows the current-browser info button only when a help note applies", async () => {
    vi.mocked(api.fetchWebAccessSession).mockResolvedValueOnce({
      ok: true,
      result: {
        web_access_session: {
          can_access_global_settings: true,
          current_browser_signed_in: true,
          principal_kind: "local",
          login_active: false,
        },
      },
    });
    vi.mocked(api.fetchAccessTokens).mockResolvedValueOnce({
      ok: true,
      result: { access_tokens: [] },
    });
    await render();
    expect(host.querySelector('button[aria-label="webAccess.currentBrowserTitle"]')).toBeNull();

    await act(async () => root.unmount());
    root = createRoot(host);
    vi.mocked(api.fetchWebAccessSession).mockResolvedValueOnce({
      ok: true,
      result: {
        web_access_session: {
          can_access_global_settings: true,
          current_browser_signed_in: true,
          principal_kind: "local",
          login_active: true,
        },
      },
    });
    await render();
    const localWithTokenTrigger = host.querySelector<HTMLButtonElement>(
      'button[aria-label="webAccess.currentBrowserTitle"]',
    );
    expect(localWithTokenTrigger).not.toBeNull();
    await act(async () => localWithTokenTrigger!.click());
    const localPopoverText = document.querySelector("[data-settings-info-popover]")?.textContent;
    expect(localPopoverText).toContain("webAccess.currentBrowserVerifyHint");
    expect(localPopoverText).not.toContain("webAccess.signOutHint");

    await act(async () => root.unmount());
    root = createRoot(host);
    vi.mocked(api.fetchWebAccessSession).mockResolvedValueOnce({
      ok: true,
      result: {
        web_access_session: {
          can_access_global_settings: true,
          current_browser_signed_in: true,
          principal_kind: "token",
          login_active: true,
        },
      },
    });
    await render();
    const signedInTrigger = host.querySelector<HTMLButtonElement>(
      'button[aria-label="webAccess.currentBrowserTitle"]',
    );
    expect(signedInTrigger).not.toBeNull();
    await act(async () => signedInTrigger!.click());
    const signedInPopoverText = document.querySelector("[data-settings-info-popover]")?.textContent;
    expect(signedInPopoverText).toContain("webAccess.currentBrowserVerifyHint");
    expect(signedInPopoverText).toContain("webAccess.signOutHint");
  });
});
