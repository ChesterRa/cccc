// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
const mocks = vi.hoisted(() => ({ load: vi.fn(), resume: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../services/api", () => ({
  fetchWebModelBrowserSurfaceSession: (...a: unknown[]) => mocks.load(...a),
  resumeWebModelBrowserDelivery: (...a: unknown[]) => mocks.resume(...a),
  getWebModelBrowserSurfaceWebSocketUrl: (g: string, a: string) => `ws://fixture/${g}/${a}`,
}));
vi.mock("../browser/ProjectedBrowserSurfacePanel", () => ({
  ProjectedBrowserSurfacePanel: ({ sessionIdentity }: { sessionIdentity: string }) => (
    <div data-browser={sessionIdentity} />
  ),
}));
import { WebModelDeliveryRecovery } from "./WebModelDeliveryRecovery";
const prefix = "webModelDelivery.recovery.";
describe("message-local delivery recovery", () => {
  let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
  const props = () => ({
    groupId: "g_one",
    actorLabel: "P0",
    isDark: false,
    status: {
      state: "ambiguous" as const,
      actorId: "P0",
      deliveryId: "batch-one",
      updatedAt: "",
      detail: "",
    },
  });
  const response = (deliveryId = "batch-one", state = "ambiguous") => ({
    ok: true,
    result: {
      browser_session: {
        last_delivery_id: deliveryId,
        last_delivery_status: state,
        can_resume_delivery: state === "ambiguous",
      },
      browser_surface: { active: true },
    },
  });
  const button = (key: string) =>
    [...document.querySelectorAll("button")].find((b) => b.textContent === prefix + key)!;
  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.clearAllMocks();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    mocks.load.mockResolvedValue(response());
    mocks.resume.mockResolvedValue(response("batch-one", "resolved"));
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });
  async function open() {
    await act(async () => root.render(<WebModelDeliveryRecovery {...props()} />));
    expect(mocks.load).not.toHaveBeenCalled();
    await act(async () => button("open").click());
  }
  it("observes only after the visible action and resumes the exact Actor and batch", async () => {
    await open();
    expect(mocks.load).toHaveBeenCalledWith("g_one", "P0", { inspect: false });
    expect(document.querySelector('[data-browser="g_one/P0"]')).not.toBeNull();
    expect(mocks.resume).not.toHaveBeenCalled();
    await act(async () => button("resume").click());
    expect(mocks.resume).toHaveBeenCalledExactlyOnceWith("g_one", "P0", "batch-one");
    expect(button("resume").disabled).toBe(true);
    expect(document.body.textContent).toContain(prefix + "resolved");
  });
  it("does not release a newer pending delivery through an old message", async () => {
    mocks.load.mockResolvedValue(response("batch-two"));
    await open();
    expect(button("resume").disabled).toBe(true);
    expect(document.body.textContent).toContain(prefix + "changed");
    await act(async () => button("resume").click());
    expect(mocks.resume).not.toHaveBeenCalled();
  });
  it("disables manual continuation after automatic confirmation", async () => {
    mocks.load.mockResolvedValue(response("batch-one", "submitted"));
    await open();
    expect(button("resume").disabled).toBe(true);
    expect(document.body.textContent).toContain(prefix + "resolved");
    expect(mocks.resume).not.toHaveBeenCalled();
  });
  it("keeps the dialog and failure visible when the composer still has a draft", async () => {
    mocks.resume.mockResolvedValue({
      ok: false,
      error: { message: "Send or clear the draft first" },
    });
    await open();
    await act(async () => button("resume").click());
    expect(document.querySelector('[role="alert"]')?.textContent).toContain("Send or clear");
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    expect(button("resume").disabled).toBe(false);
  });
  it("does not expose control or start observers in read-only mode", async () => {
    await act(async () => root.render(<WebModelDeliveryRecovery {...props()} readOnly />));
    expect(button("open")).toBeUndefined();
    expect(mocks.load).not.toHaveBeenCalled();
  });
});
