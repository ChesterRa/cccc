// @vitest-environment happy-dom
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { ConnectEmbeddedApp } from "./ConnectEmbeddedApp";
import { CONNECT_CHANNEL } from "./protocol";
import { useGroupStore } from "../../stores";
import { useModalStore } from "../../stores/useModalStore";

const mocks = vi.hoisted(() => ({ request: vi.fn(), admitted: true }));
vi.mock("../../App", () => ({ default: () => <div>Native workbench</div> }));
vi.mock("../../components/AuthGate", () => ({
  AuthGate: ({ children }: { children: ReactNode }) => (mocks.admitted ? children : null),
}));
vi.mock("../../services/api/base", () => ({ apiJson: mocks.request }));
vi.mock("../../services/api", () => ({
  fetchGroups: async () => ({ ok: true, result: { groups: useGroupStore.getState().groups } }),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
const parent = { postMessage: vi.fn() } as unknown as Window;
async function select(revision: number, group = "b", origin = "https://entry.test") {
  await act(async () =>
    window.dispatchEvent(
      new MessageEvent("message", {
        source: parent,
        origin,
        data: {
          channel: CONNECT_CHANNEL,
          frame_id: "frame",
          type: "select",
          group_id: group,
          revision,
          action: "connections",
        },
      }),
    ),
  );
}
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  mocks.admitted = true;
  mocks.request.mockReset().mockResolvedValue({ ok: true, result: {} });
  vi.spyOn(window, "parent", "get").mockReturnValue(parent);
  const proof = btoa(
    JSON.stringify({
      frame_id: "frame",
      target_instance_id: "target",
      target_device_id: "device",
      parent_origin: "https://entry.test",
      expires_at: new Date(Date.now() + 120000).toISOString(),
      signature: "fixture",
    }),
  );
  window.history.replaceState(null, "", `/ui/connect?proof=${encodeURIComponent(proof)}`);
  useGroupStore.setState({ selectedGroupId: "a", groups: [{ group_id: "a" }, { group_id: "b" }] });
  useModalStore.setState({ groupConnectionsId: null });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
  window.history.replaceState(null, "", "/");
  useModalStore.setState({ groupConnectionsId: null });
});

it("opens the requested remote Group once and rejects stale, foreign and absent targets", async () => {
  await act(async () => root.render(<ConnectEmbeddedApp />));
  await select(1, "b", "https://wrong.test");
  expect(useModalStore.getState().groupConnectionsId).toBeNull();
  await select(1);
  expect(useGroupStore.getState().selectedGroupId).toBe("b");
  expect(useModalStore.getState().groupConnectionsId).toBe("b");
  useModalStore.getState().setGroupConnections(null);
  await select(1);
  expect(useModalStore.getState().groupConnectionsId).toBeNull();
  await select(3, "a");
  await select(2, "b");
  expect(useModalStore.getState().groupConnectionsId).toBe("a");
  await select(4, "deleted");
  expect(useModalStore.getState().groupConnectionsId).toBeNull();
  expect(useGroupStore.getState().selectedGroupId).toBe("a");
});

it("waits for native administrator admission before applying a menu request", async () => {
  mocks.admitted = false;
  await act(async () => root.render(<ConnectEmbeddedApp />));
  await select(1);
  expect(mocks.request).not.toHaveBeenCalled();
  expect(useModalStore.getState().groupConnectionsId).toBeNull();
  mocks.admitted = true;
  await act(async () => root.render(<ConnectEmbeddedApp />));
  expect(useModalStore.getState().groupConnectionsId).toBe("b");
});
