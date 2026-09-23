// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { WebModelConnector } from "../../../services/api";
const mocks = vi.hoisted(() => ({
  browser: vi.fn(),
  connectors: vi.fn(),
  create: vi.fn(),
  revoke: vi.fn(),
  panel: vi.fn(),
  copy: vi.fn(),
}));
vi.mock("../../../utils/copy", () => ({ copyTextToClipboard: mocks.copy }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (k: string) => k }) }));
vi.mock("../../../services/api", () => ({
  fetchWebModelConnectors: () => mocks.connectors(),
  sharedWebModelBrowser: (...args: unknown[]) => mocks.browser(...args),
  createWebModelConnector: () => mocks.create(),
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
      (b) => b.textContent === `webModelShared.${name}`,
    )!;
  it("can set up shared login without any Actor and does not start a browser on mount", async () => {
    await act(async () => root.render(<WebModelConnectorsTab isDark={false} />));
    expect(mocks.browser).toHaveBeenCalledExactlyOnceWith();
    expect(mocks.create).not.toHaveBeenCalled();
    expect(host.querySelector("[data-testid=viewer]")).toBeNull();
    mocks.browser.mockResolvedValue({
      ok: true,
      result: { browser_session: { active: true, verification_required: true } },
    });
    await act(async () => button("open").click());
    expect(mocks.browser).toHaveBeenLastCalledWith("open", false);
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
    expect(mocks.browser).toHaveBeenLastCalledWith("status", true);
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
});
