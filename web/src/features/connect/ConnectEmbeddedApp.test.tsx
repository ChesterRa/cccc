// @vitest-environment happy-dom
import { act, StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { ConnectEmbeddedApp } from "./ConnectEmbeddedApp";
import { CONNECT_CHANNEL } from "./protocol";
import { useGroupStore } from "../../stores";
import { useModalStore } from "../../stores/useModalStore";
import { useComposerStore } from "../../stores/useComposerStore";
import { beginPendingPageWork, usePendingPageWork } from "../../stores/usePendingPageWork";
import { setWorkspaceDirty, useWorkspaceNavigation } from "../../stores/workspaceNavigation";
import { frameAccessPaused } from "./frameAccess";

const mocks = vi.hoisted(() => ({ request: vi.fn(), admitted: true }));
vi.mock("../../App", () => ({
  default: () => (
    <div>
      Native workbench
      <input aria-label="fixture draft" />
    </div>
  ),
}));
vi.mock("../../components/AuthGate", () => ({
  AuthGate: ({ children }: { children: ReactNode }) => (mocks.admitted ? children : null),
}));
vi.mock("../../services/api/base", () => ({
  apiJson: mocks.request,
  apiFrameControl: mocks.request,
}));
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
  useComposerStore.setState(useComposerStore.getInitialState(), true);
  usePendingPageWork.setState({ owners: new Set() });
  useWorkspaceNavigation.setState({ owners: new Set(), pending: null });
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

it("reports in-flight page work after the workbench unmounts and until it settles", async () => {
  await act(async () => root.render(<ConnectEmbeddedApp />));
  let finish!: () => void;
  await act(async () => {
    finish = beginPendingPageWork();
  });
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: true }),
    "https://entry.test",
  );
  mocks.admitted = false;
  await act(async () => root.render(<ConnectEmbeddedApp />));
  await act(async () => finish());
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: false }),
    "https://entry.test",
  );
});
afterEach(async () => {
  await act(async () => root.unmount());
  vi.useRealTimers();
  host.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  window.history.replaceState(null, "", "/");
  useModalStore.setState({ groupConnectionsId: null });
});

it("reports a late failed send after the sign-in gate has unmounted the workbench", async () => {
  await act(async () => root.render(<ConnectEmbeddedApp />));
  vi.mocked(parent.postMessage).mockClear();
  mocks.admitted = false;
  await act(async () => root.render(<ConnectEmbeddedApp />));
  await act(async () =>
    useComposerStore.setState({
      failedSendsByGroup: {
        a: [
          {
            id: "fixture",
            originGroupId: "a",
            destGroupId: "a",
            retryKey: null,
            composerText: "Unconfirmed message",
            composerFiles: [],
            toText: "",
            replyTarget: null,
            quotedPresentationRef: null,
            quotedVoiceDocumentRef: null,
            messageMode: "send",
          },
        ],
      },
    }),
  );
  expect(parent.postMessage).toHaveBeenCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: true }),
    "https://entry.test",
  );
  await act(async () => useComposerStore.setState({ failedSendsByGroup: {} }));
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: false }),
    "https://entry.test",
  );
});

it("reports dirty while either a file edit or a failed send remains", async () => {
  await act(async () => root.render(<ConnectEmbeddedApp />));
  const owner = Symbol("fixture editor");
  await act(async () => setWorkspaceDirty(owner, true));
  await act(async () =>
    useComposerStore.setState({
      failedSendsByGroup: {
        a: [
          {
            id: "fixture",
            originGroupId: "a",
            destGroupId: "a",
            retryKey: null,
            composerText: "Unconfirmed message",
            composerFiles: [],
            toText: "",
            replyTarget: null,
            quotedPresentationRef: null,
            quotedVoiceDocumentRef: null,
            messageMode: "send",
          },
        ],
      },
    }),
  );
  await act(async () => setWorkspaceDirty(owner, false));
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: true }),
    "https://entry.test",
  );
  await act(async () => useComposerStore.setState({ failedSendsByGroup: {} }));
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: false }),
    "https://entry.test",
  );
});

it("reports active composer text and attachments, but not an empty or whitespace-only draft", async () => {
  await act(async () => root.render(<ConnectEmbeddedApp />));
  expect(parent.postMessage).toHaveBeenCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: false }),
    "https://entry.test",
  );
  vi.mocked(parent.postMessage).mockClear();
  await act(async () => useComposerStore.getState().setComposerText("  \n\t"));
  expect(parent.postMessage).not.toHaveBeenCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: true }),
    "https://entry.test",
  );
  await act(async () => useComposerStore.getState().setComposerText("Draft"));
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: true }),
    "https://entry.test",
  );
  await act(async () => useComposerStore.getState().setComposerText(""));
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: false }),
    "https://entry.test",
  );
  await act(async () =>
    useComposerStore.getState().setComposerFiles([new File(["attachment"], "note.txt")]),
  );
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: true }),
    "https://entry.test",
  );
  await act(async () => useComposerStore.getState().setComposerFiles([]));
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: false }),
    "https://entry.test",
  );
});

it("keeps other Groups' drafts protected and ignores an outdated saved copy of the active draft", async () => {
  useComposerStore.setState({ activeGroupId: "a" });
  await act(async () => root.render(<ConnectEmbeddedApp />));
  await act(async () => useComposerStore.getState().setComposerText("Other Group draft"));
  await act(async () => useComposerStore.getState().switchGroup("a", "b"));
  expect(useComposerStore.getState().composerText).toBe("");
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: true }),
    "https://entry.test",
  );

  await act(async () => useComposerStore.getState().switchGroup("b", "a"));
  await act(async () => useComposerStore.getState().setComposerText(""));
  expect(useComposerStore.getState().drafts.a?.composerText).toBe("Other Group draft");
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: false }),
    "https://entry.test",
  );

  await act(async () =>
    useComposerStore.getState().setComposerFiles([new File(["attachment"], "note.txt")]),
  );
  await act(async () => useComposerStore.getState().switchGroup("a", "b"));
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: true }),
    "https://entry.test",
  );
  await act(async () => useComposerStore.getState().clearDraft("a"));
  expect(parent.postMessage).toHaveBeenLastCalledWith(
    expect.objectContaining({ type: "workspace_dirty", dirty: false }),
    "https://entry.test",
  );
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

// A transport failure is not proof that the signed frame has been revoked.
const responseFailure = (code: string, httpStatus?: number) => ({
  ok: false,
  error: { code, message: "fixture", ...(httpStatus === undefined ? {} : { httpStatus }) },
});
function frameSequence(codes: string[], httpStatus?: number) {
  let reads = 0;
  mocks.request.mockImplementation(async (path: string) => {
    if (path.startsWith("/api/v1/connect/frame?")) {
      const code = codes[Math.min(reads++, codes.length - 1)];
      return code === "ok" ? { ok: true, result: {} } : responseFailure(code, httpStatus);
    }
    return { ok: true, result: {} };
  });
  return () => reads;
}
async function tick(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

async function useRealFrameApi(responses: Response[]) {
  const { apiJson } =
    await vi.importActual<typeof import("../../services/api/base")>("../../services/api/base");
  const fetchFrame = vi.fn();
  for (const response of responses) fetchFrame.mockResolvedValueOnce(response);
  vi.stubGlobal("fetch", fetchFrame);
  mocks.request.mockImplementation(async (path: string, init?: RequestInit) => {
    if (path.startsWith("/api/v1/connect/frame?")) return apiJson(path, init);
    return { ok: true, result: {} };
  });
  return fetchFrame;
}

const successfulFrameResponse = () =>
  new Response(JSON.stringify({ ok: true, result: {} }), { status: 200 });

it.each([
  [502, "<html>Bad Gateway</html>"],
  [503, ""],
  [504, JSON.stringify({ message: "Gateway Timeout" })],
  [503, JSON.stringify({ ok: false, error: { code: "upstream_unavailable", message: "busy" } })],
  [503, JSON.stringify({ ok: false, error: { code: "connect_frame_expired", message: "denied" } })],
])("keeps the workbench through an HTTP %i frame check failure", async (status, body) => {
  vi.useFakeTimers();
  const fetchFrame = await useRealFrameApi([
    successfulFrameResponse(),
    new Response(body, { status }),
    successfulFrameResponse(),
  ]);
  await act(async () => root.render(<ConnectEmbeddedApp />));
  const input = host.querySelector<HTMLInputElement>("[aria-label='fixture draft']")!;
  expect(input).not.toBeNull();
  input.value = "an unfinished edit";
  vi.mocked(parent.postMessage).mockClear();
  await tick(15000);
  expect(fetchFrame).toHaveBeenCalledTimes(2);
  expect(host.querySelector("[aria-label='fixture draft']")).toBe(input);
  expect(input.value).toBe("an unfinished edit");
  expect(parent.postMessage).not.toHaveBeenCalledWith(
    expect.objectContaining({ type: "expired" }),
    expect.anything(),
  );
  await tick(15000);
  expect(fetchFrame).toHaveBeenCalledTimes(3);
  expect(host.querySelector("[aria-label='fixture draft']")).toBe(input);
});

it("does not admit the workbench when its first frame check returns a gateway error", async () => {
  vi.useFakeTimers();
  const fetchFrame = await useRealFrameApi([
    new Response("<html>Bad Gateway</html>", { status: 502 }),
    successfulFrameResponse(),
  ]);
  await act(async () => root.render(<ConnectEmbeddedApp />));
  expect(host.textContent).toContain("connecting");
  expect(host.textContent).not.toContain("Native workbench");
  await tick(15000);
  expect(fetchFrame).toHaveBeenCalledTimes(2);
  expect(host.textContent).toContain("Native workbench");
});

it("reports a 401 to the login gate without terminating the frame", async () => {
  vi.useFakeTimers();
  const { onAuthRequired } =
    await vi.importActual<typeof import("../../services/api/base")>("../../services/api/base");
  const onLoginRequired = vi.fn();
  const unsubscribe = onAuthRequired(onLoginRequired);
  try {
    const fetchFrame = await useRealFrameApi([
      successfulFrameResponse(),
      new Response(JSON.stringify({ ok: false, error: { code: "auth_required" } }), {
        status: 401,
      }),
    ]);
    await act(async () => root.render(<ConnectEmbeddedApp />));
    vi.mocked(parent.postMessage).mockClear();
    await tick(15000);
    expect(fetchFrame).toHaveBeenCalledTimes(2);
    expect(onLoginRequired).toHaveBeenCalled();
    expect(host.textContent).toContain("Native workbench");
    expect(parent.postMessage).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: "expired" }),
      expect.anything(),
    );
    expect(parent.postMessage).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: "identity_changed" }),
      expect.anything(),
    );
  } finally {
    unsubscribe();
  }
});

it.each([
  [403, "<html>Forbidden</html>"],
  [403, JSON.stringify({ ok: false, error: { code: "connect_frame_expired", message: "denied" } })],
])("closes the workbench for an explicit HTTP %i denial", async (status, body) => {
  vi.useFakeTimers();
  const fetchFrame = await useRealFrameApi([
    successfulFrameResponse(),
    new Response(body, { status }),
  ]);
  await act(async () => root.render(<ConnectEmbeddedApp />));
  vi.mocked(parent.postMessage).mockClear();
  await tick(15000);
  expect(fetchFrame).toHaveBeenCalledTimes(2);
  expect(host.textContent).toContain("connect.reopen");
  expect(parent.postMessage).toHaveBeenCalledWith(
    expect.objectContaining({ type: "expired" }),
    "https://entry.test",
  );
});

it("keeps the admitted workspace and its draft through a transient network failure", async () => {
  vi.useFakeTimers();
  const reads = frameSequence(["ok", "NETWORK_ERROR", "ok"]);
  await act(async () => root.render(<ConnectEmbeddedApp />));
  const input = host.querySelector<HTMLInputElement>("[aria-label='fixture draft']")!;
  expect(input).not.toBeNull();
  input.value = "an unfinished edit";
  vi.mocked(parent.postMessage).mockClear();
  await tick(15000);
  expect(reads()).toBe(2);
  expect(host.querySelector("[aria-label='fixture draft']")).toBe(input);
  expect(input.value).toBe("an unfinished edit");
  expect(parent.postMessage).not.toHaveBeenCalledWith(
    expect.objectContaining({ type: "expired" }),
    expect.anything(),
  );
  await tick(15000);
  expect(reads()).toBe(3);
  expect(host.querySelector("[aria-label='fixture draft']")).toBe(input);
});

it("shows connection progress and retries if the first frame check has a network error", async () => {
  vi.useFakeTimers();
  const reads = frameSequence(["NETWORK_ERROR", "ok"]);
  vi.mocked(parent.postMessage).mockClear();
  await act(async () => root.render(<ConnectEmbeddedApp />));
  expect(host.textContent).toContain("connecting");
  expect(host.textContent).not.toContain("Native workbench");
  expect(parent.postMessage).not.toHaveBeenCalledWith(
    expect.objectContaining({ type: "expired" }),
    expect.anything(),
  );
  await tick(15000);
  expect(reads()).toBe(2);
  expect(host.textContent).toContain("Native workbench");
});

it.each([
  ["network", "NETWORK_ERROR", undefined],
  ["gateway", "HTTP_ERROR", 502],
] as const)("expires at the proof deadline if the %s never recovers", async (_, code, status) => {
  vi.useFakeTimers();
  const reads = frameSequence(["ok", code], status);
  await act(async () => root.render(<ConnectEmbeddedApp />));
  vi.mocked(parent.postMessage).mockClear();
  await tick(119000);
  expect(host.textContent).toContain("Native workbench");
  expect(parent.postMessage).not.toHaveBeenCalledWith(
    expect.objectContaining({ type: "expired" }),
    expect.anything(),
  );
  await tick(2000);
  expect(host.textContent).toContain("connect.reopen");
  expect(parent.postMessage).toHaveBeenCalledWith(
    expect.objectContaining({ type: "expired" }),
    "https://entry.test",
  );
  const before = reads();
  await tick(60000);
  expect(reads()).toBe(before);
});

it("closes for a 403 even if reading the response body is interrupted", async () => {
  vi.useFakeTimers();
  const denied = new Response("partial", { status: 403 });
  vi.spyOn(denied, "text").mockRejectedValue(new DOMException("interrupted", "NetworkError"));
  const fetchFrame = await useRealFrameApi([successfulFrameResponse(), denied]);
  await act(async () => root.render(<ConnectEmbeddedApp />));
  vi.mocked(parent.postMessage).mockClear();
  await tick(15000);
  expect(fetchFrame).toHaveBeenCalledTimes(2);
  expect(host.textContent).toContain("connect.reopen");
  expect(parent.postMessage).toHaveBeenCalledWith(
    expect.objectContaining({ type: "expired" }),
    "https://entry.test",
  );
});

it.each([
  "connect_frame_expired",
  "bootstrap_required",
  "HTTP_ERROR",
  "EMPTY_RESPONSE",
  "PARSE_ERROR",
])("still closes immediately on frame check error %s", async (code) => {
  vi.useFakeTimers();
  const reads = frameSequence(["ok", code]);
  await act(async () => root.render(<ConnectEmbeddedApp />));
  vi.mocked(parent.postMessage).mockClear();
  await tick(15000);
  expect(reads()).toBe(2);
  expect(host.textContent).toContain("connect.reopen");
  expect(parent.postMessage).toHaveBeenCalledWith(
    expect.objectContaining({ type: "expired" }),
    "https://entry.test",
  );
});

it.each(["admin_required", "auth_required"])(
  "leaves an admitted frame for the login gate to handle %s",
  async (code) => {
    vi.useFakeTimers();
    const reads = frameSequence(["ok", code, "ok"]);
    await act(async () => root.render(<ConnectEmbeddedApp />));
    vi.mocked(parent.postMessage).mockClear();
    await tick(15000);
    expect(host.textContent).toContain("Native workbench");
    expect(parent.postMessage).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: "expired" }),
      expect.anything(),
    );
    expect(parent.postMessage).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: "identity_changed" }),
      expect.anything(),
    );
    await tick(15000);
    expect(reads()).toBe(3);
  },
);

it("uses the renewed proof deadline while intermittent network failures continue", async () => {
  vi.useFakeTimers();
  frameSequence(["ok", "NETWORK_ERROR"]);
  await act(async () => root.render(<ConnectEmbeddedApp />));
  await tick(90000);
  const renewed = {
    frame_id: "frame",
    target_instance_id: "target",
    target_device_id: "device",
    parent_origin: "https://entry.test",
    expires_at: new Date(Date.now() + 120000).toISOString(),
    signature: "fixture-renewed",
  };
  await act(async () =>
    window.dispatchEvent(
      new MessageEvent("message", {
        source: parent,
        origin: "https://entry.test",
        data: { channel: CONNECT_CHANNEL, frame_id: "frame", type: "renew", proof: renewed },
      }),
    ),
  );
  vi.mocked(parent.postMessage).mockClear();
  await tick(31000);
  expect(host.textContent).toContain("Native workbench");
  expect(parent.postMessage).not.toHaveBeenCalledWith(
    expect.objectContaining({ type: "expired" }),
    expect.anything(),
  );
  await tick(90000);
  expect(host.textContent).toContain("connect.reopen");
  expect(parent.postMessage).toHaveBeenCalledWith(
    expect.objectContaining({ type: "expired" }),
    "https://entry.test",
  );
});

it("does not retry or restore a workbench after an in-flight check outlives admission", async () => {
  vi.useFakeTimers();
  let finish: ((value: unknown) => void) | undefined;
  let reads = 0;
  mocks.request.mockImplementation(async (path: string) => {
    if (path.startsWith("/api/v1/connect/frame?")) {
      reads += 1;
      return new Promise((resolve) => {
        finish = resolve;
      });
    }
    return { ok: true, result: {} };
  });
  await act(async () => root.render(<ConnectEmbeddedApp />));
  expect(reads).toBe(1);
  mocks.admitted = false;
  await act(async () => root.render(<ConnectEmbeddedApp />));
  vi.mocked(parent.postMessage).mockClear();
  await act(async () => {
    finish?.(responseFailure("NETWORK_ERROR"));
  });
  await tick(30000);
  expect(reads).toBe(1);
  expect(host.textContent).not.toContain("Native workbench");
  expect(parent.postMessage).not.toHaveBeenCalledWith(
    expect.objectContaining({ type: "expired" }),
    expect.anything(),
  );
});

it("retains an admitted document until the same-account recovery handshake commits", async () => {
  vi.useFakeTimers();
  mocks.request.mockImplementation(async (path: string) => {
    if (path.startsWith("/api/v1/connect/frame?"))
      return { ok: true, result: { account_id: "owner" } };
    if (path === "/api/v1/connect")
      return { ok: true, result: { connect: { directory: { account_id: "owner" } } } };
    return { ok: true, result: {} };
  });
  await act(async () => root.render(<ConnectEmbeddedApp />));
  const input = host.querySelector<HTMLInputElement>("input")!;
  input.value = "unsent work";
  const encoded = new URLSearchParams(window.location.search).get("proof")!;
  const original = JSON.parse(atob(encoded));
  await tick(120000);
  expect(host.querySelector("input")).toBe(input);
  expect(document.body.inert).toBe(true);
  expect(parent.postMessage).toHaveBeenCalledWith(
    expect.objectContaining({ type: "expired", recoverable: true }),
    "https://entry.test",
  );
  const next = {
    ...original,
    nonce: "fresh",
    expires_at: new Date(Date.now() + 120000).toISOString(),
  };
  await act(async () =>
    window.dispatchEvent(
      new MessageEvent("message", {
        source: parent,
        origin: "https://entry.test",
        data: {
          channel: CONNECT_CHANNEL,
          frame_id: "frame",
          type: "resume",
          account_id: "owner",
          proof: next,
        },
      }),
    ),
  );
  expect(parent.postMessage).toHaveBeenCalledWith(
    expect.objectContaining({ type: "resume_ready", nonce: "fresh" }),
    "https://entry.test",
  );
  expect(document.body.inert).toBe(true);
  await act(async () =>
    window.dispatchEvent(
      new MessageEvent("message", {
        source: parent,
        origin: "https://entry.test",
        data: {
          channel: CONNECT_CHANNEL,
          frame_id: "frame",
          type: "resume_commit",
          nonce: "fresh",
        },
      }),
    ),
  );
  expect(document.body.inert).toBe(false);
  expect(host.querySelector("input")).toBe(input);
  expect(input.value).toBe("unsent work");
  expect(parent.postMessage).toHaveBeenCalledWith(
    expect.objectContaining({ type: "resumed", nonce: "fresh" }),
    "https://entry.test",
  );
  const reads = () =>
    mocks.request.mock.calls.filter(([path]) => path.startsWith("/api/v1/connect/frame?")).length;
  const before = reads();
  await tick(15000);
  expect(reads()).toBeGreaterThan(before);
});

it("does not restore an old document under a different target account", async () => {
  vi.useFakeTimers();
  let changed = false;
  mocks.request.mockImplementation(async (path: string) => {
    if (path.startsWith("/api/v1/connect/frame?"))
      return { ok: true, result: { account_id: "owner" } };
    if (path === "/api/v1/connect")
      return {
        ok: true,
        result: { connect: { directory: { account_id: changed ? "other" : "owner" } } },
      };
    return { ok: true, result: {} };
  });
  await act(async () => root.render(<ConnectEmbeddedApp />));
  const original = JSON.parse(atob(new URLSearchParams(window.location.search).get("proof")!));
  await tick(120000);
  changed = true;
  await act(async () =>
    window.dispatchEvent(
      new MessageEvent("message", {
        source: parent,
        origin: "https://entry.test",
        data: {
          channel: CONNECT_CHANNEL,
          frame_id: "frame",
          type: "resume",
          account_id: "owner",
          proof: {
            ...original,
            nonce: "fresh",
            expires_at: new Date(Date.now() + 120000).toISOString(),
          },
        },
      }),
    ),
  );
  expect(parent.postMessage).toHaveBeenCalledWith(
    expect.objectContaining({ type: "identity_changed" }),
    "https://entry.test",
  );
  expect(host.textContent).toContain("connect.reopen");
  expect(mocks.request.mock.calls.some(([path]) => path === "/api/v1/connect/frame/resume")).toBe(
    false,
  );
  expect(frameAccessPaused()).toBe(true);
  const fetch = vi.fn();
  vi.stubGlobal("fetch", fetch);
  const actual =
    await vi.importActual<typeof import("../../services/api/base")>("../../services/api/base");
  expect((await actual.apiJson("/api/v1/groups/g/settings", { method: "PUT" })).ok).toBe(false);
  expect(fetch).not.toHaveBeenCalled();
});

it("keeps the deadline guard active after StrictMode replays its effects", async () => {
  vi.useFakeTimers();
  await act(async () =>
    root.render(
      <StrictMode>
        <ConnectEmbeddedApp />
      </StrictMode>,
    ),
  );
  vi.setSystemTime(Date.now() + 120001);
  await act(async () => {
    expect(frameAccessPaused()).toBe(true);
  });
});
