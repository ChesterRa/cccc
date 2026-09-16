import {
  finishWorkspaceNavigation,
  requestWorkspaceNavigation,
  useWorkspaceNavigation,
} from "../../stores/workspaceNavigation";
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { ConnectRemotePanel } from "./ConnectRemotePanel";
import { CONNECT_CHANNEL } from "./protocol";
import { useConnectWorkbench } from "./useConnectWorkbench";

const mocks = vi.hoisted(() => ({ request: vi.fn(), access: vi.fn(), t: (key: string) => key }));
vi.mock("../../services/api/base", () => ({ apiJson: mocks.request }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.t }) }));

let root: ReturnType<typeof createRoot>;
let host: HTMLDivElement;
let state: ReturnType<typeof useConnectWorkbench>;
let frameId: string;
const instance = { instance_id: "b", device_id: "device-b", public_origin: "https://b.test" };
function Probe() {
  state = useConnectWorkbench(true, mocks.access);
  return state.selected ? <ConnectRemotePanel workbench={state} onOpenSidebar={() => {}} /> : null;
}
async function fromTarget(data: Record<string, unknown>) {
  await act(async () => {
    window.dispatchEvent(
      new MessageEvent("message", {
        source: host.querySelector("iframe")!.contentWindow,
        origin: instance.public_origin,
        data: { channel: CONNECT_CHANNEL, frame_id: frameId, ...data },
      }),
    );
  });
}
beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  mocks.access.mockReset().mockResolvedValue(true);
  mocks.request.mockReset().mockImplementation(async (path, options) => {
    if (path === "/api/v1/connect/open") {
      frameId = JSON.parse(options.body).frame_id;
      return {
        ok: true,
        result: {
          origin: instance.public_origin,
          url: "about:blank",
          proof: {
            frame_id: frameId,
            target_device_id: instance.device_id,
            expires_at: new Date(Date.now() + 120000).toISOString(),
          },
        },
      };
    }
    return {
      ok: true,
      result: {
        connect: {
          instance_id: "a",
          directory: {
            expires_at: new Date(Date.now() + 120000).toISOString(),
            instances: [instance],
          },
        },
      },
    };
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root.render(<Probe />));
  await act(async () => state.select("b"));
  vi.spyOn(host.querySelector("iframe")!.contentWindow!, "postMessage").mockImplementation(
    () => {},
  );
  await fromTarget({ type: "ready", instance_id: "b", device_id: "device-b" });
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

it("does not let a late default Group report replace a newer sidebar choice", async () => {
  const iframe = host.querySelector("iframe");
  expect(iframe?.hasAttribute("allowfullscreen")).toBe(true);
  const initialRevision = state.selected?.revision;
  await act(async () => state.select("b", "requested"));
  const revision = state.selected?.revision;
  await fromTarget({ type: "selected", group_id: "default", revision: initialRevision });
  expect(state.selected?.groupId).toBe("requested");
  await fromTarget({ type: "selected", group_id: "requested", revision });
  expect(state.selected?.groupId).toBe("requested");
  expect(host.querySelector("iframe")).toBe(iframe);
  expect(mocks.request.mock.calls.filter(([path]) => path === "/api/v1/connect/open")).toHaveLength(
    1,
  );
});

it("accepts target navigation in the current revision without issuing another navigation", async () => {
  const revision = state.selected?.revision;
  await fromTarget({ type: "selected", group_id: "default", revision });
  expect(state.selected?.groupId).toBe("default");
  await fromTarget({ type: "selected", group_id: "created-in-target", revision });
  expect(state.selected?.groupId).toBe("created-in-target");
  expect(state.selected?.revision).toBe(revision);
  await fromTarget({ type: "selected", group_id: "wrong-frame", revision, frame_id: "different" });
  expect(state.selected?.groupId).toBe("created-in-target");
});

it("forwards the remote Group menu action through the existing admitted frame", async () => {
  const frame = host.querySelector("iframe")!;
  const postMessage = frame.contentWindow!.postMessage;
  await act(async () => state.select("b", "group-b", "connections"));
  expect(postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({
      type: "select",
      group_id: "group-b",
      action: "connections",
      revision: state.selected!.revision,
    }),
    "https://b.test",
  );
  expect(host.querySelector("iframe")).toBe(frame);
  await act(async () => state.select("b", "group-b"));
  expect(postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ action: undefined }),
    "https://b.test",
  );
  expect(mocks.request.mock.calls.filter(([path]) => path === "/api/v1/connect/open")).toHaveLength(
    1,
  );
});

it("guards leaving a dirty remote editor and clears its flag when the verified frame is retired", async () => {
  await fromTarget({ type: "workspace_dirty", dirty: true });
  expect(useWorkspaceNavigation.getState().owners.size).toBe(1);
  await act(async () => state.select("c"));
  expect(state.selected?.instanceId).toBe("b");
  await act(async () => finishWorkspaceNavigation(false));
  expect(state.selected?.instanceId).toBe("b");
  await act(async () => requestWorkspaceNavigation(() => state.selectLocal()));
  expect(state.selected?.instanceId).toBe("b");
  await act(async () => finishWorkspaceNavigation(true));
  expect(state.selected).toBeNull();
  expect(useWorkspaceNavigation.getState().owners.size).toBe(0);
});

it("does not let an unverified source create an unsaved-edits prompt", async () => {
  await act(async () =>
    window.dispatchEvent(
      new MessageEvent("message", {
        source: window,
        origin: instance.public_origin,
        data: { channel: CONNECT_CHANNEL, frame_id: frameId, type: "workspace_dirty", dirty: true },
      }),
    ),
  );
  expect(useWorkspaceNavigation.getState().owners.size).toBe(0);
  await fromTarget({ type: "workspace_dirty", dirty: true });
  await fromTarget({ type: "expired" });
  expect(useWorkspaceNavigation.getState().owners.size).toBe(0);
});
