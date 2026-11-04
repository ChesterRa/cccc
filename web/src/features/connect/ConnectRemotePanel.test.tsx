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
          account_id: "owner",
          proof: {
            frame_id: frameId,
            target_device_id: instance.device_id,
            expires_at: new Date(Date.now() + 120000).toISOString(),
            nonce: crypto.randomUUID(),
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
            account_id: "owner",
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
  await fromTarget({ type: "admitted", account_id: "owner" });
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

it("retains the remote leave guard across sign-in until the frame reports work is clear", async () => {
  await fromTarget({ type: "workspace_dirty", dirty: true });
  await fromTarget({ type: "locked" });
  expect(useWorkspaceNavigation.getState().owners.size).toBe(1);
  // Moving within the same remote instance does not destroy its document.
  await act(async () => state.select("b", "another-group"));
  expect(state.selected?.groupId).toBe("another-group");
  await act(async () => requestWorkspaceNavigation(() => state.selectLocal()));
  expect(state.selected?.instanceId).toBe("b");
  await act(async () => finishWorkspaceNavigation(false));
  expect(state.selected?.instanceId).toBe("b");
  await fromTarget({ type: "workspace_dirty", dirty: false });
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

it("keeps the same dirty iframe through directory expiry and confirms recovery on both sides", async () => {
  const frame = host.querySelector("iframe")!;
  const post = vi.mocked(frame.contentWindow!.postMessage);
  await fromTarget({ type: "workspace_dirty", dirty: true });
  const originalRequest = mocks.request.getMockImplementation()!;
  let offline = true;
  mocks.request.mockImplementation((path: string, options: unknown) => {
    if (path === "/api/v1/connect" && offline)
      return Promise.resolve({
        ok: true,
        result: {
          connect: { instance_id: "a", directory: null, error_code: "connect_directory_expired" },
        },
      });
    return originalRequest(path, options);
  });
  await act(async () => vi.advanceTimersByTimeAsync(15000));
  expect(state.available).toBe(false);
  expect(host.querySelector("iframe")).toBe(frame);
  expect(frame.className).toContain("invisible");
  expect(useWorkspaceNavigation.getState().owners.size).toBe(1);
  expect(post).toHaveBeenCalledWith(
    expect.objectContaining({ type: "suspend" }),
    instance.public_origin,
  );

  offline = false;
  await act(async () => vi.advanceTimersByTimeAsync(15000));
  const resumes = post.mock.calls
    .map(([message]) => message)
    .filter((message) => message.type === "resume");
  const resume = resumes[resumes.length - 1];
  expect(resume.account_id).toBe("owner");
  await fromTarget({ type: "resume_ready", nonce: resume.proof.nonce });
  expect(post).toHaveBeenCalledWith(
    expect.objectContaining({ type: "resume_commit", nonce: resume.proof.nonce }),
    instance.public_origin,
  );
  expect(frame.className).toContain("invisible");
  await fromTarget({ type: "resumed", nonce: resume.proof.nonce });
  expect(host.querySelector("iframe")).toBe(frame);
  expect(frame.className).not.toContain("invisible");
  expect(useWorkspaceNavigation.getState().owners.size).toBe(1);
});

it("hides the same iframe when the entry directory expires before the frame proof", async () => {
  const frame = host.querySelector("iframe")!;
  const originalRequest = mocks.request.getMockImplementation()!;
  const expiresAt = new Date(Date.now() + 16000).toISOString();
  mocks.request.mockImplementation((path: string, options: unknown) => {
    if (path === "/api/v1/connect")
      return Promise.resolve({
        ok: true,
        result: {
          connect: {
            instance_id: "a",
            directory: { account_id: "owner", expires_at: expiresAt, instances: [instance] },
          },
        },
      });
    return originalRequest(path, options);
  });
  await act(async () => vi.advanceTimersByTimeAsync(15000));
  expect(state.available).toBe(true);
  expect(frame.className).not.toContain("invisible");
  await act(async () => vi.advanceTimersByTimeAsync(1100));
  expect(state.available).toBe(false);
  expect(host.querySelector("iframe")).toBe(frame);
  expect(frame.className).toContain("invisible");
  expect(frame.hasAttribute("inert")).toBe(true);
});

it("asks before reopening a frame that reports unsaved work", async () => {
  const frame = host.querySelector("iframe");
  await fromTarget({ type: "workspace_dirty", dirty: true });
  await fromTarget({ type: "reload" });
  expect(host.querySelector("iframe")).toBe(frame);
  expect(useWorkspaceNavigation.getState().pending).not.toBeNull();
  await act(async () => finishWorkspaceNavigation(false));
  expect(host.querySelector("iframe")).toBe(frame);
  await fromTarget({ type: "reload" });
  await act(async () => finishWorkspaceNavigation(true));
  expect(host.querySelector("iframe")).not.toBe(frame);
});

it("ignores a recovery acknowledgement after entry authority is lost again", async () => {
  const frame = host.querySelector("iframe")!;
  const post = vi.mocked(frame.contentWindow!.postMessage);
  const originalRequest = mocks.request.getMockImplementation()!;
  let offline = true;
  mocks.request.mockImplementation((path: string, options: unknown) => {
    if (path === "/api/v1/connect" && offline)
      return Promise.resolve({
        ok: true,
        result: {
          connect: { instance_id: "a", directory: null, error_code: "connect_directory_expired" },
        },
      });
    return originalRequest(path, options);
  });
  await act(async () => vi.advanceTimersByTimeAsync(15000));
  offline = false;
  await act(async () => vi.advanceTimersByTimeAsync(15000));
  const resumes = post.mock.calls
    .map(([message]) => message)
    .filter((message) => message.type === "resume");
  const oldNonce = resumes[resumes.length - 1].proof.nonce;
  offline = true;
  await act(async () => vi.advanceTimersByTimeAsync(15000));
  post.mockClear();
  await fromTarget({ type: "resume_ready", nonce: oldNonce });
  expect(post.mock.calls.some(([message]) => message.type === "resume_commit")).toBe(false);
  expect(frame.className).toContain("invisible");
});

it("suspends again when the entry directory expires before the target confirms recovery", async () => {
  const frame = host.querySelector("iframe")!;
  const post = vi.mocked(frame.contentWindow!.postMessage);
  await fromTarget({ type: "expired", recoverable: true });
  await act(async () => vi.advanceTimersByTimeAsync(15000));
  const resumes = post.mock.calls
    .map(([message]) => message)
    .filter((message) => message.type === "resume");
  const nonce = resumes[resumes.length - 1].proof.nonce;
  await fromTarget({ type: "resume_ready", nonce });
  post.mockClear();
  vi.setSystemTime(Date.now() + 121000);
  await fromTarget({ type: "resumed", nonce });
  expect(post).toHaveBeenCalledWith(
    expect.objectContaining({ type: "suspend" }),
    instance.public_origin,
  );
  expect(frame.className).toContain("invisible");
});

it("keeps dirty work through an unavailable open and retries while suspended", async () => {
  const frame = host.querySelector("iframe")!;
  const originalRequest = mocks.request.getMockImplementation()!;
  await fromTarget({ type: "workspace_dirty", dirty: true });
  await fromTarget({ type: "expired", recoverable: true });
  mocks.request.mockImplementation((path: string, options: unknown) => {
    if (path === "/api/v1/connect/open")
      return Promise.resolve({
        ok: false,
        error: { code: "unavailable", message: "offline", httpStatus: 503 },
      });
    return originalRequest(path, options);
  });
  await act(async () => vi.advanceTimersByTimeAsync(30000));
  expect(
    mocks.request.mock.calls.filter(([path]) => path === "/api/v1/connect/open").length,
  ).toBeGreaterThan(2);
  expect(host.querySelector("iframe")).toBe(frame);
  expect(frame.className).toContain("invisible");
  expect(useWorkspaceNavigation.getState().owners.size).toBe(1);
});

it("closes an older target that cannot confirm an account for recovery", async () => {
  await act(async () => state.selectLocal());
  await act(async () => state.select("b"));
  vi.spyOn(host.querySelector("iframe")!.contentWindow!, "postMessage").mockImplementation(
    () => {},
  );
  await fromTarget({ type: "ready", instance_id: "b", device_id: "device-b" });
  await fromTarget({ type: "expired", recoverable: true });
  expect(host.querySelector("iframe")).toBeNull();
  expect(host.textContent).toContain("connect.reopen");
});

it.each(["success", "failure"])(
  "ignores a late open %s after the frame ends and permits retry",
  async (outcome) => {
    const frame = host.querySelector("iframe")!;
    const originalRequest = mocks.request.getMockImplementation()!;
    let completeOpen!: (value: unknown) => void;
    mocks.request.mockImplementation((path: string, options: unknown) => {
      if (path === "/api/v1/connect/open")
        return new Promise((resolve) => {
          completeOpen = resolve;
        });
      return originalRequest(path, options);
    });

    await act(async () => vi.advanceTimersByTimeAsync(45000));
    expect(completeOpen).toBeTypeOf("function");
    await fromTarget({ type: "identity_changed" });
    expect(host.querySelector("iframe")).toBeNull();
    expect(host.textContent).toContain("connect.reopen");

    await act(async () => {
      completeOpen(
        outcome === "success"
          ? {
              ok: true,
              result: {
                origin: instance.public_origin,
                url: "about:blank",
                account_id: "owner",
                proof: {
                  frame_id: frameId,
                  target_device_id: instance.device_id,
                  expires_at: new Date(Date.now() + 120000).toISOString(),
                  nonce: crypto.randomUUID(),
                },
              },
            }
          : { ok: false, error: { code: "unavailable", message: "late failure", httpStatus: 503 } },
      );
    });
    expect(host.querySelector("iframe")).toBeNull();
    expect(host.textContent).toContain("connect.reopen");
    expect(frame.isConnected).toBe(false);

    const openCount = mocks.request.mock.calls.filter(
      ([path]) => path === "/api/v1/connect/open",
    ).length;
    await act(async () => vi.advanceTimersByTimeAsync(60000));
    expect(
      mocks.request.mock.calls.filter(([path]) => path === "/api/v1/connect/open"),
    ).toHaveLength(openCount);
    expect(host.textContent).toContain("connect.reopen");

    mocks.request.mockImplementation(originalRequest);
    const retry = [...host.querySelectorAll("button")].find(
      (button) => button.textContent === "connect.retry",
    );
    expect(retry).toBeDefined();
    await act(async () => retry!.click());
    expect(host.querySelector("iframe")).not.toBeNull();
  },
);
