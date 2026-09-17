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
      result: {
        access_tokens: [
          {
            token_id: "admin",
            user_id: "owner",
            is_admin: true,
            allowed_groups: [],
            created_at: "2026-09-18T00:00:00Z",
          },
        ],
      },
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

  const render = async () => {
    await act(async () => root.render(<WebAccessTab isDark={false} onOpenAccount={() => {}} />));
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
});
