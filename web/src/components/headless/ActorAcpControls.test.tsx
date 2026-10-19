// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { ActorAcpControls } from "./ActorAcpControls";
const api = vi.hoisted(() => ({ call: vi.fn() }));
vi.mock("../../services/api/base", () => ({ apiJson: (...args: unknown[]) => api.call(...args) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  api.call.mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});
const state = (working = false, permissions: unknown[] = []) => ({
  ok: true,
  result: { state: { generation: "fixture", working, permissions } },
});
const mount = () =>
  act(async () => root.render(<ActorAcpControls groupId="g" actorId="a" visible />));
it("does not reserve an empty control area while idle", async () => {
  api.call.mockResolvedValue(state());
  await mount();
  expect(host.childElementCount).toBe(0);
});
it("keeps a failed cancellation visible after the runtime becomes idle", async () => {
  api.call.mockResolvedValue(state(true));
  await mount();
  api.call.mockResolvedValueOnce({
    ok: false,
    error: { message: "Synthetic cancellation failure" },
  });
  await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
  api.call.mockResolvedValue(state());
  await act(async () => vi.advanceTimersByTimeAsync(1200));
  expect(host.textContent).toContain("Synthetic cancellation failure");
  expect(host.querySelector("button")).toBeNull();
});
it("keeps pending decisions actionable while the runtime is idle", async () => {
  api.call.mockResolvedValue(
    state(false, [{ request_id: "request", kind: "tool", title: "Fixture tool" }]),
  );
  await mount();
  expect(host.textContent).toContain("Fixture tool");
  expect(host.querySelectorAll("button")).toHaveLength(2);
  api.call.mockResolvedValue({ ok: true, result: {} });
  await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
  expect(JSON.parse(api.call.mock.calls.at(-1)![1].body)).toEqual({
    action: "permission",
    request_id: "request",
    allow: true,
    generation: "fixture",
  });
});
