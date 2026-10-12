// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { WebModelConnector, WebModelView } from "../../../services/api";
const mocks = vi.hoisted(() => ({
  browser: vi.fn(),
  connectors: vi.fn(),
  create: vi.fn(),
  revoke: vi.fn(),
  panel: vi.fn(),
  copy: vi.fn(),
}));
vi.mock("../../../utils/copy", () => ({ copyTextToClipboard: mocks.copy }));
vi.mock("react-i18next", () => ({
  // Shows the interpolated URL, so tests can tell which address a notice names.
  useTranslation: () => ({
    t: (k: string, o?: { url?: string }) => (o?.url ? `${k}<${o.url}>` : k),
  }),
}));
vi.mock("../../../services/api", () => ({
  fetchWebModelConnectors: () => mocks.connectors(),
  sharedWebModelBrowser: (...args: unknown[]) => mocks.browser(...args),
  createWebModelConnector: (...args: unknown[]) => mocks.create(...args),
  revokeWebModelConnector: (...args: unknown[]) => mocks.revoke(...args),
  sharedWebModelBrowserWebSocketUrl: () => "ws://fixture/shared",
}));
vi.mock("../../browser/ProjectedBrowserSurfacePanel", () => ({
  ProjectedBrowserSurfacePanel: (props: unknown) => {
    mocks.panel(props);
    return <div data-testid="viewer" />;
  },
}));
import WebModelConnectorsTab from "./WebModelConnectorsTab";
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}
const empty = { ok: true, result: { connectors: [] as WebModelConnector[] } };
const existing = { connector_id: "shared", bound_actor_count: 2 };
const created = {
  ok: true,
  result: {
    connector: { ...existing, connector_url_path_token: "https://fixture.test/token/test-only" },
  },
};
describe("shared Web Model login and connector", () => {
  let host: HTMLDivElement, root: Root;
  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.resetAllMocks();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    mocks.connectors.mockResolvedValue({ ok: true, result: { connectors: [] } });
    mocks.browser.mockResolvedValue({ ok: true, result: { browser_session: { active: false } } });
    mocks.create.mockResolvedValue(created);
    mocks.copy.mockResolvedValue(true);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });
  const button = (name: string) =>
    Array.from(host.querySelectorAll("button")).find(
      (b) => !b.closest("[hidden]") && b.textContent === `webModelShared.${name}`,
    )!;
  it("can set up shared login without any Actor and does not start a browser on mount", async () => {
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    expect(mocks.browser).toHaveBeenCalledExactlyOnceWith("status", false, "chatgpt_web");
    expect(mocks.create).not.toHaveBeenCalled();
    expect(host.querySelector("[data-testid=viewer]")).toBeNull();
    mocks.browser.mockResolvedValue({
      ok: true,
      result: { browser_session: { active: true, verification_required: true } },
    });
    await act(async () => button("open").click());
    expect(mocks.browser).toHaveBeenLastCalledWith("open", false, "chatgpt_web");
    expect(host.textContent).toContain("webModelShared.verification");
    expect(host.querySelector("[data-testid=viewer]")).not.toBeNull();
    await act(async () => button("open").click());
    expect(mocks.browser.mock.calls.every((c) => c[0] !== "close")).toBe(true);
  });
  it("checking login does not refresh or replace the browser viewer", async () => {
    mocks.browser.mockResolvedValue({ ok: true, result: { browser_session: { active: true } } });
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    const viewer = host.querySelector("[data-testid=viewer]");
    await act(async () => button("check").click());
    expect(mocks.browser).toHaveBeenLastCalledWith("status", true, "chatgpt_web");
    expect(host.querySelector("[data-testid=viewer]")).toBe(viewer);
    expect(mocks.panel.mock.calls.every(([props]) => props.refreshNonce === 0)).toBe(true);
  });
  it("requires explicit confirmation for credential rotation and preserves error visibility", async () => {
    mocks.connectors.mockResolvedValue({
      ok: true,
      result: { connectors: [{ connector_id: "shared", bound_actor_count: 2 }] },
    });
    mocks.create.mockResolvedValue({ ok: false, error: { message: "Save failed" } });
    await act(async () => root.render(<WebModelConnectorsTab isDark={true} />));
    expect(host.textContent).toContain("webModelShared.notSeen");
    expect(host.querySelector("details")?.open).toBe(false);
    await act(async () => {
      host.querySelector("summary")!.click();
    });
    await act(async () => button("rotate").click());
    expect(mocks.create).not.toHaveBeenCalled();
    await act(async () => button("confirm").click());
    expect(mocks.create).toHaveBeenCalledOnce();
    expect(host.textContent).toContain("Save failed");
    expect(host.textContent).toContain("2 webModelShared.pairedActors");
  });
  it("keeps an open login viewer when only the connector read fails", async () => {
    mocks.connectors.mockResolvedValueOnce({ ok: false, error: { message: "Read failed" } });
    mocks.browser.mockResolvedValue({ ok: true, result: { browser_session: { active: true } } });
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    expect(host.textContent).toContain("Read failed");
    expect(host.querySelector("[data-testid=viewer]")).not.toBeNull();
    // The login state comes with a successful read; without one it is not claimed.
    expect(host.querySelector("[data-login-state]")).toBeNull();
  });
  it("unmounts the viewer when this settings tab is inactive", async () => {
    mocks.browser.mockResolvedValue({ ok: true, result: { browser_session: { active: true } } });
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    expect(host.querySelector("[data-testid=viewer]")).not.toBeNull();
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} isActive={false} />));
    expect(host.querySelector("[data-testid=viewer]")).toBeNull();
    expect(mocks.browser).toHaveBeenCalledOnce();
  });
  it("blocks creation while the initial connector list is unknown, then requires rotation confirmation", async () => {
    const read = deferred<typeof empty>();
    mocks.connectors.mockReturnValueOnce(read.promise);
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    expect(button("create").disabled).toBe(true);
    await act(async () => button("create").click());
    expect(mocks.create).not.toHaveBeenCalled();
    await act(async () => read.resolve({ ok: true, result: { connectors: [existing] } }));
    expect(button("create")).toBeUndefined();
    await act(async () => host.querySelector("summary")!.click());
    await act(async () => button("rotate").click());
    expect(mocks.create).not.toHaveBeenCalled();
    await act(async () => button("cancel").click());
    expect(mocks.create).not.toHaveBeenCalled();
    await act(async () => button("rotate").click());
    await act(async () => button("confirm").click());
    expect(mocks.create).toHaveBeenCalledOnce();
    await act(async () => button("copy").click());
    expect(mocks.copy).toHaveBeenCalledExactlyOnceWith(
      created.result.connector.connector_url_path_token,
    );
  });
  it.each(["response", "network"])(
    "keeps a failed %s load closed to writes until a successful retry",
    async (failure) => {
      if (failure === "network") mocks.connectors.mockRejectedValueOnce(new Error("Read failed"));
      else mocks.connectors.mockResolvedValueOnce({ ok: false, error: { message: "Read failed" } });
      await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
      expect(host.textContent).toContain("Read failed");
      expect(button("create").disabled).toBe(true);
      await act(async () => button("create").click());
      expect(mocks.create).not.toHaveBeenCalled();
      await act(async () => {
        Array.from(host.querySelectorAll("button"))
          .find((b) => b.textContent === "common:retry")!
          .click();
      });
      expect(host.textContent).not.toContain("Read failed");
      expect(button("create").disabled).toBe(false);
      await act(async () => button("create").click());
      expect(mocks.create).toHaveBeenCalledOnce();
      expect(button("copy")).toBeDefined();
    },
  );
  it("preserves a pending creation across tab reactivation without a read overwriting its one-time URL", async () => {
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    const write = deferred<typeof created>();
    const read = deferred<typeof empty>();
    mocks.create.mockReturnValueOnce(write.promise);
    mocks.connectors.mockReturnValueOnce(read.promise);
    await act(async () => button("create").click());
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} isActive={false} />));
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    await act(async () => write.resolve(created));
    expect(button("copy")).toBeDefined();
    await act(async () => read.resolve(empty));
    expect(button("copy")).toBeDefined();
    expect(mocks.connectors).toHaveBeenCalledOnce();
    await act(async () => button("copy").click());
    expect(mocks.copy).toHaveBeenCalledExactlyOnceWith(
      created.result.connector.connector_url_path_token,
    );
  });
  it("ignores a superseded activation's read after a new credential is created", async () => {
    const old = deferred<typeof empty>();
    mocks.connectors.mockReturnValueOnce(old.promise);
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} isActive={false} />));
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    await act(async () => button("create").click());
    await act(async () => old.resolve(empty));
    expect(button("copy")).toBeDefined();
    expect(button("create")).toBeUndefined();
  });
  it("submits only one credential mutation before React renders the busy state", async () => {
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    const write = deferred<typeof created>();
    mocks.create.mockReturnValue(write.promise);
    await act(async () => {
      button("create").click();
      button("create").click();
    });
    expect(mocks.create).toHaveBeenCalledOnce();
    await act(async () => write.resolve(created));
  });
  it("switches providers from a radio group, including with arrow keys", async () => {
    const onProviderChange = vi.fn();
    await act(async () =>
      root.render(<WebModelConnectorsTab isDark={false} onProviderChange={onProviderChange} />),
    );
    const radios = Array.from(host.querySelectorAll('[role="radiogroup"] [role="radio"]'));
    expect(radios.map((r) => [r.textContent, r.getAttribute("aria-checked")])).toEqual([
      ["ChatGPT", "true"],
      ["Grok Bot", "false"],
      ["Dots", "false"],
    ]);
    await act(async () => {
      radios[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    });
    expect(onProviderChange).toHaveBeenCalledExactlyOnceWith("grok_web");
  });
  it.each([false, true])(
    "moves focus from the hidden Dots management action to ChatGPT (controlled=%s)",
    async (controlled) => {
      function SettingsFixture() {
        const [provider, setProvider] = useState<WebModelView>("chatgpt_web");
        return (
          <WebModelConnectorsTab
            isDark={false}
            {...(controlled ? { provider, onProviderChange: setProvider } : {})}
          />
        );
      }
      await act(async () => root.render(<SettingsFixture />));
      const radios = Array.from(
        host.querySelectorAll<HTMLButtonElement>('[role="radiogroup"] [role="radio"]'),
      );
      await act(async () => radios[2].click());
      const manage = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
        (element) =>
          !element.closest("[hidden]") && element.textContent === "dotsSettings.manageConnector",
      )!;
      manage.focus();
      expect(document.activeElement).toBe(manage);
      await act(async () => manage.click());
      expect(radios[0].getAttribute("aria-checked")).toBe("true");
      expect(manage.closest("[hidden]")).not.toBeNull();
      expect(document.activeElement).toBe(radios[0]);
      expect(mocks.browser.mock.calls.every((call) => call[0] === "status")).toBe(true);
      expect(mocks.create).not.toHaveBeenCalled();
    },
  );
  it("boxes the migration warning, the web access prerequisite and the Actor guidance", async () => {
    const onOpenWebAccess = vi.fn();
    mocks.connectors.mockResolvedValue({
      ok: true,
      result: { connectors: [], requires_reconfiguration: true },
    });
    await act(async () =>
      root.render(<WebModelConnectorsTab isDark={false} onOpenWebAccess={onOpenWebAccess} />),
    );
    const page = host.querySelector("div:not([hidden]) > [data-settings-page]")!;
    const notices = (tone: string) =>
      Array.from(page.querySelectorAll(`[data-settings-notice="${tone}"]`));
    expect(notices("warning").map((n) => n.textContent)).toEqual([
      "webModelShared.upgrade",
      "webModelShared.publicUrlMissingwebModelShared.webAccess",
    ]);
    // Creating the connector is the card's own action, not a button stacked under the hint.
    const connectorCard = button("create").closest("[data-settings-card]")!;
    expect(connectorCard.querySelector("h3")?.textContent).toBe("webModelShared.connector");
    await act(async () => button("webAccess").click());
    expect(onOpenWebAccess).toHaveBeenCalledOnce();
    expect(notices("info").map((n) => n.textContent)).toEqual(["webModelShared.actorHint"]);
    expect(page.querySelector('[role="alert"]')).toBeNull();
  });
  it.each([
    ["https://cccc.example.test", false],
    ["", true],
    ["http://cccc.example.test", true],
    ["https://localhost:8848", true],
  ])(
    "asks for public HTTPS Web access from the configured address %j, before any connector exists",
    async (base, warned) => {
      mocks.connectors.mockResolvedValue({
        ok: true,
        result: { connectors: [], connector_base_url: base },
      });
      await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
      expect(button("create")).toBeDefined();
      const page = host.querySelector("div:not([hidden]) > [data-settings-page]")!;
      expect(page.textContent?.includes("webModelShared.publicUrl")).toBe(warned);
    },
  );
  it.each([
    "https://192.168.1.20",
    "https://10.0.0.1",
    "https://172.20.0.4",
    "https://127.0.0.2",
    "https://169.254.1.2",
    "https://0.0.0.0",
    "https://host.docker.internal",
    "https://demo.local",
  ])("keeps the external-address warning for local HTTPS address %s", async (base) => {
    mocks.connectors.mockResolvedValue({
      ok: true,
      result: { connectors: [], connector_base_url: `${base}/private-connector-path` },
    });
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    const page = host.querySelector("div:not([hidden]) > [data-settings-page]")!;
    expect(page.textContent).toContain(`webModelShared.publicUrlInvalid<${base}>`);
    expect(page.textContent).not.toContain("private-connector-path");
    expect(mocks.create).not.toHaveBeenCalled();
    expect(mocks.browser).toHaveBeenCalledExactlyOnceWith("status", false, "chatgpt_web");
  });
  it.each([
    [
      "an older server reporting no base",
      undefined,
      "https://cccc.example.test/mcp/web-model/shared",
      false,
    ],
    ["an emptied Web access address", "", "https://cccc.example.test/mcp/web-model/shared", true],
    ["an older server with a relative URL", undefined, "/mcp/web-model/shared", true],
  ])(
    "judges an existing connector's URL only for %s",
    async (_case, base, connectorUrl, warned) => {
      mocks.connectors.mockResolvedValue({
        ok: true,
        result: {
          connectors: [{ ...existing, connector_url: connectorUrl }],
          ...(base === undefined ? {} : { connector_base_url: base }),
        },
      });
      await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
      const page = host.querySelector("div:not([hidden]) > [data-settings-page]")!;
      expect(page.textContent).toContain("webModelShared.configured");
      expect(page.textContent?.includes("webModelShared.publicUrl")).toBe(warned);
    },
  );
  it.each([
    ["chatgpt_web", { connector_base_url: "" }, null, "webModelShared.publicUrlMissing"],
    [
      "chatgpt_web",
      { connector_base_url: "http://cccc.example.test/" },
      null,
      "webModelShared.publicUrlInvalid<http://cccc.example.test>",
    ],
    [
      "chatgpt_web",
      { connector_base_url: "https://localhost:8848" },
      null,
      "webModelShared.publicUrlInvalid<https://localhost:8848>",
    ],
    // An older server reports no base: its connector URL takes the same branches.
    [
      "chatgpt_web",
      {},
      "http://cccc.example.test/mcp/web-model/shared",
      "webModelShared.publicUrlInvalid<http://cccc.example.test>",
    ],
    ["chatgpt_web", {}, "/mcp/web-model/shared", "webModelShared.publicUrlMissing"],
    // Addresses without an http(s) origin are no Web address; the connector path never shows.
    [
      "chatgpt_web",
      { connector_base_url: "data:text/plain,cccc" },
      null,
      "webModelShared.publicUrlMissing",
    ],
    ["chatgpt_web", {}, "data:text/plain,/mcp/web-model/shared", "webModelShared.publicUrlMissing"],
    [
      "chatgpt_web",
      {},
      "mailto:ops@cccc.example.test/mcp/web-model/shared",
      "webModelShared.publicUrlMissing",
    ],
    ["grok_web", { connector_base_url: "" }, null, "grokShared.publicUrlMissing"],
    [
      "grok_web",
      { connector_base_url: "http://cccc.example.test" },
      null,
      "grokShared.publicUrlInvalid<http://cccc.example.test>",
    ],
  ] as const)(
    "%s: %j with connector URL %j says %s",
    async (provider, base, connectorUrl, expected) => {
      mocks.connectors.mockResolvedValue({
        ok: true,
        result: {
          connectors: connectorUrl ? [{ ...existing, provider, connector_url: connectorUrl }] : [],
          ...base,
        },
      });
      await act(async () =>
        root.render(<WebModelConnectorsTab isDark={false} provider={provider} />),
      );
      const page = host.querySelector("div:not([hidden]) > [data-settings-page]")!;
      const notices = Array.from(page.querySelectorAll('[data-settings-notice="warning"]'))
        .map((notice) => notice.textContent)
        .filter((text) => text?.includes("publicUrl"));
      expect(notices).toEqual([expected]);
    },
  );
  it("reports a failed load in one alert that carries its retry", async () => {
    mocks.connectors.mockResolvedValueOnce({ ok: false, error: { message: "Read failed" } });
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    const alert = host.querySelector('div:not([hidden]) [role="alert"]')!;
    expect(alert.getAttribute("data-settings-notice")).toBe("error");
    expect(alert.textContent).toBe("Read failedcommon:retry");
  });
  it("scopes provider actions and keeps one-time URLs when switching providers", async () => {
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    await act(async () => button("create").click());
    expect(mocks.create).toHaveBeenLastCalledWith("chatgpt_web");
    mocks.connectors.mockResolvedValue({ ok: true, result: { connectors: [existing] } });
    const select = (name: string) =>
      Array.from(host.querySelectorAll("button")).find((b) => b.textContent === name)!;
    await act(async () => select("Grok Bot").click());
    expect(mocks.browser).toHaveBeenLastCalledWith("status", false, "grok_web");
    expect(button("copy")).toBeUndefined();
    expect(button("create").disabled).toBe(false);
    await act(async () => button("create").click());
    expect(mocks.create).toHaveBeenLastCalledWith("grok_web");
    await act(async () => select("ChatGPT").click());
    expect(button("copy")).toBeDefined();
    await act(async () => button("copy").click());
    expect(mocks.copy).toHaveBeenLastCalledWith("https://fixture.test/token/test-only");
  });
  it("lays out the one manual ChatGPT plugin step with every value one click away", async () => {
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    await act(async () => button("create").click());
    const card = host.querySelector("[data-chatgpt-plugin-setup]")!;
    const fields = Array.from(card.querySelectorAll("[data-plugin-field]")).map((field) =>
      field.getAttribute("data-plugin-field"),
    );
    expect(fields).toEqual(["name", "url", "auth"]);
    expect(card.querySelector('[data-plugin-field="auth"]')!.textContent).toContain(
      "No authentication",
    );
    expect(card.querySelector("a")!.getAttribute("href")).toBe(
      "https://chatgpt.com/settings/plugins-settings",
    );
    // The private URL is only ever copied, never rendered.
    expect(host.textContent).not.toContain("test-only");
    await act(async () => button("pluginSetup.copyName").click());
    expect(mocks.copy).toHaveBeenLastCalledWith("CCCC");
    await act(async () => button("copy").click());
    expect(mocks.copy).toHaveBeenLastCalledWith("https://fixture.test/token/test-only");
  });
});
