// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import * as api from "../services/api";
import { useComposerStore, useGroupStore, useUIStore } from "../stores";
import { useChatTab } from "./useChatTab";
import type { Actor } from "../types";

// Boundary: the HTTP helpers; the hook's own send state runs for real.
vi.mock("../services/api", () => ({
  sendMessage: vi.fn(),
  replyMessage: vi.fn(),
  sendCrossGroupMessage: vi.fn(),
}));
vi.mock("./useSlashCommandState", () => ({
  useSlashCommandState: () => ({ items: [], loading: false }),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
// A runtime that gets queued reply placeholders, so a replayed send can be checked for them.
const actors = [{ id: "codex-1", runtime: "codex", role: "foreman", enabled: true } as Actor];
let root: Root;
let host: HTMLDivElement;
let chat: ReturnType<typeof useChatTab>;
function Harness() {
  const selectedGroupId = useGroupStore((s) => s.selectedGroupId);
  chat = useChatTab({
    selectedGroupId,
    selectedGroupRunning: false,
    actors,
    recipientActors: actors,
  });
  return null;
}

const lost = { ok: false as const, error: { code: "NETWORK_ERROR", message: "connection reset" } };
const accepted = (clientId: string) => ({
  ok: true as const,
  result: {
    event: {
      id: "event-1",
      kind: "chat.message",
      ts: "2026-11-04T00:00:00Z",
      by: "user",
      group_id: "g-a",
      data: { text: "ship it", client_id: clientId },
    },
  },
});
const clientIds = () => vi.mocked(api.sendMessage).mock.calls.map((call) => call[5]);
async function send(text?: string) {
  if (text !== undefined) await act(async () => useComposerStore.getState().setComposerText(text));
  await act(async () => {
    await chat.sendMessage();
  });
}

beforeEach(async () => {
  vi.clearAllMocks();
  useComposerStore.setState({ ...useComposerStore.getInitialState() });
  useGroupStore.setState({
    selectedGroupId: "g-a",
    groups: [],
    groupDoc: null,
    groupSettings: null,
    groupContext: null,
    chatByGroup: {},
  });
  useUIStore.setState({ chatSessions: {}, busy: "", errorMsg: "" });
  useComposerStore.getState().switchGroup(null, "g-a");
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => {
    root.render(<Harness />);
  });
});
afterEach(async () => {
  await act(async () => {
    root.unmount();
  });
  host.remove();
});

it("resends an unchanged draft after a lost response under the same client ID", async () => {
  vi.mocked(api.sendMessage)
    .mockResolvedValueOnce(lost)
    .mockImplementationOnce(async (...args) => accepted(String(args[5])));
  await send("ship it");
  expect(useComposerStore.getState().composerText).toBe("ship it");
  await send();
  const [first, retry] = clientIds();
  expect(first).toMatch(/^local_/);
  // The daemon returns the message it already accepted under this ID instead of a second one.
  expect(retry).toBe(first);
  expect(useComposerStore.getState().composerText).toBe("");
});

it("treats an edited draft or a later message as new", async () => {
  vi.mocked(api.sendMessage)
    .mockResolvedValueOnce(lost)
    .mockImplementation(async (...args) => accepted(String(args[5])));
  await send("ship it");
  await send("ship it today");
  await send("ship it today");
  const [failed, edited, later] = clientIds();
  expect(edited).not.toBe(failed);
  expect(later).not.toBe(edited);
});

it("does not reuse the ID once the restored draft was edited, even back to the same text", async () => {
  vi.mocked(api.sendMessage)
    .mockResolvedValueOnce(lost)
    .mockImplementation(async (...args) => accepted(String(args[5])));
  await send("ship it");
  await act(async () => useComposerStore.getState().setComposerText(""));
  await send("ship it");
  const [failed, retyped] = clientIds();
  // The first attempt may have been accepted; the retyped message must not be taken for it.
  expect(retyped).not.toBe(failed);
});

it("treats a different file with the same name, size, type and time as a new message", async () => {
  const lastModified = Date.parse("2026-11-04T00:00:00Z");
  const first = new File(["first"], "notes.txt", { type: "text/plain", lastModified });
  const other = new File(["other"], "notes.txt", { type: "text/plain", lastModified });
  vi.mocked(api.sendMessage)
    .mockResolvedValueOnce(lost)
    .mockImplementation(async (...args) => accepted(String(args[5])));
  await act(async () => useComposerStore.getState().setComposerFiles([first]));
  await send("see notes");
  expect(useComposerStore.getState().composerFiles).toEqual([first]);
  await act(async () => useComposerStore.getState().setComposerFiles([other]));
  await send();
  const [failed, replaced] = clientIds();
  expect(replaced).not.toBe(failed);
});

it("keeps the ID for a long draft that is sent as a time-named attachment", async () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  try {
    vi.setSystemTime(Date.parse("2026-11-04T00:00:00Z"));
    vi.mocked(api.sendMessage)
      .mockResolvedValueOnce(lost)
      .mockImplementation(async (...args) => accepted(String(args[5])));
    await send("x".repeat(70_000));
    vi.setSystemTime(Date.parse("2026-11-04T00:00:03Z"));
    await send();
    const [failed, retried] = clientIds();
    const names = vi
      .mocked(api.sendMessage)
      .mock.calls.map((call) => (call[3] as File[] | undefined)?.[0]?.name);
    // The generated file name changes with the clock; the retry identity must not.
    expect(names[0]).not.toBe(names[1]);
    expect(retried).toBe(failed);
  } finally {
    vi.useRealTimers();
  }
});

it("does not reuse the ID when a remote reply gains its remote parent", async () => {
  // A receipt that arrives later fills in the remote parent; the retried reply would go elsewhere.
  const target = {
    eventId: "event-0",
    by: "peer-1",
    text: "question",
    remoteDstGroupId: "g-remote",
    remoteDstTo: ["peer-x"],
  };
  vi.mocked(api.sendCrossGroupMessage)
    .mockResolvedValueOnce(lost)
    .mockResolvedValue({ ok: true, result: {} });
  await act(async () => useComposerStore.getState().setReplyTarget(target));
  await send("answer");
  await act(async () =>
    useComposerStore.getState().setReplyTarget({ ...target, remoteReplyToEventId: "remote-0" }),
  );
  await send();
  const [failed, retried] = vi
    .mocked(api.sendCrossGroupMessage)
    .mock.calls.map((call) => (call[6] as { clientId?: string } | undefined)?.clientId);
  expect(failed).toMatch(/^local_/);
  expect(retried).not.toBe(failed);
});

it("adds no queued reply placeholder when the daemon returns the accepted message", async () => {
  vi.mocked(api.sendMessage)
    .mockResolvedValueOnce(lost)
    .mockImplementationOnce(async (...args) => ({
      ...accepted(String(args[5])),
      result: { ...accepted(String(args[5])).result, duplicate: true },
    }));
  await send("ship it");
  await send();
  const streaming = useGroupStore.getState().chatByGroup["g-a"]?.streamingEvents ?? [];
  // The replay starts no new work, so nothing may stay queued for it.
  expect(streaming.filter((event) => event.by === "codex-1")).toEqual([]);
});

it("keeps the queued reply placeholder for an ordinary accepted send", async () => {
  // Control for the replay case above: this harness does create placeholders.
  vi.mocked(api.sendMessage).mockImplementationOnce(async (...args) => accepted(String(args[5])));
  await send("ship it");
  const streaming = useGroupStore.getState().chatByGroup["g-a"]?.streamingEvents ?? [];
  expect(streaming.some((event) => event.by === "codex-1")).toBe(true);
});

it("keeps a real stream that arrives before the duplicate response", async () => {
  let finish!: (value: ReturnType<typeof accepted> & { result: { duplicate: boolean } }) => void;
  vi.mocked(api.sendMessage)
    .mockResolvedValueOnce(lost)
    .mockImplementationOnce(() => new Promise((resolve) => (finish = resolve)));
  await send("ship it");
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  await act(async () =>
    useGroupStore
      .getState()
      .promoteStreamingEventToStream("codex-1", "event-1", "real-stream", "g-a"),
  );
  const response = accepted(String(clientIds()[0]));
  await act(async () => {
    finish({ ...response, result: { ...response.result, duplicate: true } });
    await pending;
  });
  const streaming = useGroupStore.getState().chatByGroup["g-a"]?.streamingEvents ?? [];
  expect(
    streaming.some((event) => (event.data as { stream_id?: string })?.stream_id === "real-stream"),
  ).toBe(true);
});
