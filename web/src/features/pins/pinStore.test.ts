import { beforeEach, expect, it, vi } from "vite-plus/test";

const api = vi.hoisted(() => ({
  fetchPinnedIds: vi.fn(),
  fetchPinnedMessages: vi.fn(),
  setMessagePinned: vi.fn(),
}));
vi.mock("../../services/api/pins", () => api);

import { applyPinRead, beginPinRead, loadPinnedIds, setPinned, usePinStore } from "./pinStore";

const pin = (eventId: string) => ({ event_id: eventId, pinned_at: "2026-11-03T00:00:00Z" });

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => (resolve = done));
  return { promise, resolve };
}

beforeEach(() => {
  usePinStore.setState({ groups: {} });
  vi.clearAllMocks();
});

it("keeps the Group's pins in the server's order with a lookup by message", async () => {
  api.fetchPinnedIds.mockResolvedValue({ ok: true, result: { pins: [pin("b"), pin("a")] } });
  await loadPinnedIds("g1");
  const group = usePinStore.getState().groups.g1;
  expect(group).toMatchObject({ ids: ["b", "a"], loaded: true, unsupported: false });
  expect(group.pinned).toEqual({ a: true, b: true });
});

it("shares one read between overlapping loads of the same Group", async () => {
  const read = deferred<unknown>();
  api.fetchPinnedIds.mockReturnValue(read.promise);
  const first = loadPinnedIds("g1");
  const second = loadPinnedIds("g1");
  read.resolve({ ok: true, result: { pins: [pin("a")] } });
  await Promise.all([first, second]);
  expect(api.fetchPinnedIds).toHaveBeenCalledTimes(1);
});

it("hides pins where the daemon has none, and keeps what it shows after another failure", async () => {
  api.fetchPinnedIds.mockResolvedValueOnce({ ok: true, result: { pins: [pin("a")] } });
  await loadPinnedIds("g1");
  api.fetchPinnedIds.mockResolvedValueOnce({
    ok: false,
    error: { code: "NETWORK_ERROR", message: "offline" },
  });
  await loadPinnedIds("g1");
  expect(usePinStore.getState().groups.g1).toMatchObject({ ids: ["a"], unsupported: false });
  api.fetchPinnedIds.mockResolvedValueOnce({
    ok: false,
    error: { code: "unknown_op", message: "unknown op" },
  });
  await loadPinnedIds("g2");
  expect(usePinStore.getState().groups.g2).toMatchObject({ ids: [], unsupported: true });
});

it("does not let a read that overlaps a change undo it", async () => {
  const read = deferred<unknown>();
  api.fetchPinnedIds.mockReturnValue(read.promise);
  const loading = loadPinnedIds("g1");
  api.setMessagePinned.mockResolvedValue({ ok: true, result: { pins: [pin("a"), pin("b")] } });
  await expect(setPinned("g1", "b", true)).resolves.toEqual({ ok: true });
  read.resolve({ ok: true, result: { pins: [pin("a")] } });
  await loading;
  expect(usePinStore.getState().groups.g1.ids).toEqual(["a", "b"]);

  // The Pinned view's full read follows the same rule, also while a change is under way.
  const viewRead = beginPinRead("g1");
  const change = deferred<unknown>();
  api.setMessagePinned.mockReturnValue(change.promise);
  const unpinning = setPinned("g1", "b", false);
  expect(applyPinRead("g1", viewRead, [pin("a"), pin("b")])).toBe(false);
  change.resolve({ ok: true, result: { pins: [pin("a")] } });
  await unpinning;
  expect(applyPinRead("g1", viewRead, [pin("a"), pin("b")])).toBe(false);
  expect(usePinStore.getState().groups.g1.ids).toEqual(["a"]);
  // A read started after the change settles is shown.
  expect(applyPinRead("g1", beginPinRead("g1"), [pin("c")])).toBe(true);
  expect(usePinStore.getState().groups.g1.ids).toEqual(["c"]);
});

it("does not let an earlier read that answers last undo a later one", async () => {
  // The Group's ID list is read, then the Pinned view reads the full list after another device
  // pinned "b"; the full read answers first.
  const ids = deferred<unknown>();
  api.fetchPinnedIds.mockReturnValue(ids.promise);
  const loading = loadPinnedIds("g1");
  const viewRead = beginPinRead("g1");
  expect(applyPinRead("g1", viewRead, [pin("a"), pin("b")])).toBe(true);
  ids.resolve({ ok: true, result: { pins: [pin("a")] } });
  await loading;
  expect(usePinStore.getState().groups.g1.ids).toEqual(["a", "b"]);
});

it("sends a Group's changes one at a time, so every answer can be shown in turn", async () => {
  const first = deferred<unknown>();
  const second = deferred<unknown>();
  api.setMessagePinned.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  const pinA = setPinned("g1", "a", true);
  const pinB = setPinned("g1", "b", true);
  await Promise.resolve();
  expect(api.setMessagePinned).toHaveBeenCalledTimes(1);
  first.resolve({ ok: true, result: { pins: [pin("a")] } });
  await pinA;
  expect(usePinStore.getState().groups.g1.ids).toEqual(["a"]);
  await Promise.resolve();
  expect(api.setMessagePinned).toHaveBeenCalledTimes(2);
  second.resolve({ ok: true, result: { pins: [pin("a"), pin("b")] } });
  await pinB;
  expect(usePinStore.getState().groups.g1.ids).toEqual(["a", "b"]);
});

it("reports a refused change without touching the pins", async () => {
  usePinStore.getState().replace("g1", [pin("a")]);
  api.fetchPinnedIds.mockResolvedValue({ ok: true, result: { pins: [pin("a")] } });
  api.setMessagePinned.mockResolvedValue({
    ok: false,
    error: { code: "pin_limit_reached", message: "limit" },
  });
  await expect(setPinned("g1", "b", true)).resolves.toEqual({
    ok: false,
    code: "pin_limit_reached",
    message: "limit",
  });
  expect(api.setMessagePinned).toHaveBeenCalledWith("g1", "b", true);
  expect(usePinStore.getState().groups.g1.ids).toEqual(["a"]);
  await vi.waitFor(() => expect(api.fetchPinnedIds).toHaveBeenCalledTimes(1));
});

it("reads the pins again after a failed change, once queued changes and reads have settled", async () => {
  // The Group's first read is under way when a change is refused and another one queues
  // behind it. The refusal sets that read aside, so the pins are read once more afterwards.
  const firstRead = deferred<unknown>();
  api.fetchPinnedIds
    .mockResolvedValue({ ok: true, result: { pins: [pin("a"), pin("c")] } })
    .mockReturnValueOnce(firstRead.promise);
  const loading = loadPinnedIds("g2");
  const queued = deferred<unknown>();
  api.setMessagePinned
    .mockResolvedValueOnce({ ok: false, error: { code: "pin_limit_reached", message: "limit" } })
    .mockReturnValueOnce(queued.promise);
  const refused = setPinned("g2", "b", true);
  const following = setPinned("g2", "c", true);
  await expect(refused).resolves.toMatchObject({ ok: false });
  firstRead.resolve({ ok: true, result: { pins: [pin("a")] } });
  await loading;
  expect(usePinStore.getState().groups.g2).toBeUndefined();
  // No second read while the queued change is still under way.
  await Promise.resolve();
  expect(api.fetchPinnedIds).toHaveBeenCalledTimes(1);
  queued.resolve({ ok: false, error: { code: "network", message: "lost" } });
  await following;
  await vi.waitFor(() => expect(usePinStore.getState().groups.g2?.ids).toEqual(["a", "c"]));
  // Each failed change asks for at most one more read, and a read never asks for another.
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(api.fetchPinnedIds.mock.calls.length).toBeLessThanOrEqual(3);
  expect(usePinStore.getState().groups.g2?.ids).toEqual(["a", "c"]);
});
