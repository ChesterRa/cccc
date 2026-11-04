// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import * as api from "../services/api";
import { PendingPageWorkUnloadGuard } from "../components/app/PendingPageWorkUnloadGuard";
import { useComposerStore, useGroupStore, useUIStore } from "../stores";
import { usePendingPageWork } from "../stores/usePendingPageWork";
import { useChatTab } from "./useChatTab";
import type { Actor } from "../types";

// Boundary: the HTTP helpers; the hook's own send guards and routing run for real.
vi.mock("../services/api", () => ({
  sendMessage: vi.fn(),
  replyMessage: vi.fn(),
  sendCrossGroupMessage: vi.fn(),
  useGroupCapability: vi.fn(),
}));
vi.mock("./useSlashCommandState", () => ({
  useSlashCommandState: () => ({
    slashCommands: [
      {
        name: "install",
        command: "/install",
        capabilityId: "skill:cccc:install",
        sourceType: "builtin_command",
      },
      {
        name: "lookup",
        command: "/lookup",
        capabilityId: "tool:lookup",
        toolName: "lookup",
        sourceType: "dynamic_tool",
      },
    ],
    refreshSlashCommands: async () => {},
  }),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const actors = [{ id: "peer-1", runtime: "custom", role: "peer", enabled: false } as Actor];
let recipientActors = actors;
let root: Root;
let host: HTMLDivElement;
let chat: ReturnType<typeof useChatTab>;
function ChatHarness() {
  const selectedGroupId = useGroupStore((s) => s.selectedGroupId);
  chat = useChatTab({ selectedGroupId, selectedGroupRunning: false, actors, recipientActors });
  return null;
}
function Harness({ showChat = true }: { showChat?: boolean }) {
  return (
    <>
      <PendingPageWorkUnloadGuard />
      {showChat ? <ChatHarness /> : null}
    </>
  );
}

beforeEach(async () => {
  vi.resetAllMocks();
  usePendingPageWork.setState({ owners: new Set() });
  recipientActors = actors;
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

const failed = { ok: false as const, error: { code: "NETWORK_ERROR", message: "disconnected" } };
function delayedSend() {
  let finish!: (value: typeof failed | { ok: true; result: object }) => void;
  vi.mocked(api.sendMessage).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  return finish;
}

function leavingIsBlocked() {
  const event = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(event);
  return event.defaultPrevented;
}

it.each([
  ["message", "message A"],
  ["slash message", "/install example"],
  ["dynamic tool", "/lookup example"],
])("warns while a %s is in flight, including after a Group switch", async (kind, text) => {
  let complete: () => void;
  if (kind === "dynamic tool") {
    let finish!: (value: { ok: true; result: Record<string, unknown> }) => void;
    vi.mocked(api.useGroupCapability).mockReturnValueOnce(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    complete = () => finish({ ok: true, result: {} });
  } else {
    const finish = delayedSend();
    complete = () => finish({ ok: true, result: {} });
  }
  await act(async () => useComposerStore.getState().setComposerText(text));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    expect(leavingIsBlocked()).toBe(true);
  });
  await act(async () => {
    useGroupStore.setState({ selectedGroupId: "g-b" });
    useComposerStore.getState().switchGroup("g-a", "g-b");
  });
  expect(leavingIsBlocked()).toBe(true);
  await act(async () => {
    complete();
    await pending;
  });
  expect(leavingIsBlocked()).toBe(false);
});

it("keeps the leave warning while an in-flight send outlives its chat component", async () => {
  const finish = delayedSend();
  await act(async () => useComposerStore.getState().setComposerText("message A"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    root.render(<Harness showChat={false} />);
  });
  expect(leavingIsBlocked()).toBe(true);
  await act(async () => {
    finish({ ok: true, result: {} });
    await pending;
  });
  expect(leavingIsBlocked()).toBe(false);
});

it("T123: restores the original remote destination when a new cross-Group send fails", async () => {
  vi.mocked(api.sendCrossGroupMessage)
    .mockResolvedValueOnce(failed)
    .mockResolvedValueOnce({ ok: true, result: {} });
  await act(async () => {
    useComposerStore.getState().setDestGroupId("g-b");
    useComposerStore.getState().setComposerText("private remote draft");
  });
  await act(async () => {
    await chat.sendMessage();
  });
  expect(useComposerStore.getState().destGroupId).toBe("g-b");
  await act(async () => {
    await chat.sendMessage();
  });
  expect(vi.mocked(api.sendCrossGroupMessage).mock.calls.map((args) => args[1])).toEqual([
    "g-b",
    "g-b",
  ]);
  const ids = vi.mocked(api.sendCrossGroupMessage).mock.calls.map((args) => args[6]?.clientId);
  expect(ids[0]).toMatch(/^local_/);
  expect(ids[1]).toBe(ids[0]);
  expect(api.sendMessage).not.toHaveBeenCalled();
});

it("keeps a recovered cross-Group destination and recipients through a Group switch", async () => {
  vi.mocked(api.sendCrossGroupMessage)
    .mockResolvedValueOnce(failed)
    .mockResolvedValueOnce({ ok: true, result: {} });
  await act(async () => {
    useComposerStore.getState().setDestGroupId("g-b");
    useComposerStore.getState().setToText("peer-1");
    useComposerStore.getState().setComposerText("private remote draft");
  });
  await act(async () => {
    await chat.sendMessage();
  });
  await act(async () => {
    useGroupStore.setState({ selectedGroupId: "g-c" });
    useComposerStore.getState().switchGroup("g-a", "g-c");
  });
  await act(async () => {
    useGroupStore.setState({ selectedGroupId: "g-a" });
    useComposerStore.getState().switchGroup("g-c", "g-a");
  });
  expect(useComposerStore.getState()).toMatchObject({
    destGroupId: "g-b",
    toText: "peer-1",
    composerText: "private remote draft",
  });
  await act(async () => {
    await chat.sendMessage();
  });
  expect(vi.mocked(api.sendCrossGroupMessage).mock.calls.map((args) => [args[1], args[3]])).toEqual(
    [
      ["g-b", ["peer-1"]],
      ["g-b", ["peer-1"]],
    ],
  );
  const ids = vi.mocked(api.sendCrossGroupMessage).mock.calls.map((args) => args[6]?.clientId);
  expect(ids[0]).toMatch(/^local_/);
  expect(ids[1]).toBe(ids[0]);
  expect(api.sendMessage).not.toHaveBeenCalled();
});

it("does not reuse an ID after an inactive Group's draft is edited and restored", async () => {
  vi.mocked(api.sendMessage).mockResolvedValue(failed);
  await act(async () => useComposerStore.getState().setComposerText("message A"));
  await act(async () => {
    await chat.sendMessage();
  });
  const firstId = vi.mocked(api.sendMessage).mock.calls[0][5];
  await act(async () => {
    useGroupStore.setState({ selectedGroupId: "g-b" });
    useComposerStore.getState().switchGroup("g-a", "g-b");
    useComposerStore
      .getState()
      .upsertDraft("g-a", (draft) => draft && { ...draft, composerText: "edited" });
    useComposerStore
      .getState()
      .upsertDraft("g-a", (draft) => draft && { ...draft, composerText: "message A" });
    useGroupStore.setState({ selectedGroupId: "g-a" });
    useComposerStore.getState().switchGroup("g-b", "g-a");
  });
  await act(async () => {
    await chat.sendMessage();
  });
  expect(vi.mocked(api.sendMessage).mock.calls[1][5]).not.toBe(firstId);
});

it("does not silently drop an unavailable recipient when retrying a cross-Group draft", async () => {
  vi.mocked(api.sendCrossGroupMessage)
    .mockResolvedValueOnce(failed)
    .mockResolvedValueOnce({ ok: true, result: {} });
  await act(async () => {
    useComposerStore.getState().setDestGroupId("g-b");
    useComposerStore.getState().setToText("peer-1");
    useComposerStore.getState().setComposerText("private remote draft");
  });
  await act(async () => {
    await chat.sendMessage();
  });
  expect(vi.mocked(api.sendCrossGroupMessage).mock.calls[0][3]).toEqual(["peer-1"]);
  recipientActors = [];
  await act(async () => root.render(<Harness />));
  await act(async () => {
    await chat.sendMessage();
  });
  expect(api.sendCrossGroupMessage).toHaveBeenCalledTimes(1);
  expect(useComposerStore.getState()).toMatchObject({
    destGroupId: "g-b",
    toText: "peer-1",
    composerText: "private remote draft",
  });
});

it("keeps a cross-Group reply's actual destination after recovery and editing", async () => {
  vi.mocked(api.sendCrossGroupMessage)
    .mockResolvedValueOnce(failed)
    .mockResolvedValueOnce({ ok: true, result: {} });
  await act(async () => {
    useComposerStore
      .getState()
      .setReplyTarget({
        eventId: "source-event",
        by: "user",
        text: "original question",
        remoteDstGroupId: "g-b",
        remoteDstTo: ["peer-1"],
        remoteReplyToEventId: "remote-event",
      });
    useComposerStore.getState().setComposerText("original answer");
  });
  await act(async () => {
    await chat.sendMessage();
  });
  expect(useComposerStore.getState().replyTarget?.remoteDstGroupId).toBe("g-b");
  await act(async () => {
    // The real textarea's hash-routing handler pins this field to the source Group.
    useComposerStore.getState().setDestGroupId("g-a");
    useComposerStore.getState().setComposerText("edited answer");
  });
  await act(async () => {
    await chat.sendMessage();
  });
  const calls = vi.mocked(api.sendCrossGroupMessage).mock.calls;
  expect(calls.map((args) => [args[0], args[1], args[3]])).toEqual([
    ["g-a", "g-b", ["peer-1"]],
    ["g-a", "g-b", ["peer-1"]],
  ]);
  expect(calls[1][6]).toMatchObject({
    replyTo: "source-event",
    quoteText: "original question",
    remoteReplyToEventId: "remote-event",
  });
  expect(api.sendMessage).not.toHaveBeenCalled();
});

it("T123: does not replace a new draft when an earlier ordinary send fails", async () => {
  const finish = delayedSend();
  await act(async () => useComposerStore.getState().setComposerText("message A"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  expect(leavingIsBlocked()).toBe(true);
  await act(async () => useComposerStore.getState().setComposerText("new draft B"));
  await act(async () => {
    finish(failed);
    await pending;
  });
  expect(useComposerStore.getState().composerText).toBe("new draft B");
  expect(
    useComposerStore.getState().failedSendsByGroup["g-a"]?.map((item) => item.composerText),
  ).toEqual(["message A"]);
  expect(useUIStore.getState().errorMsg).toBe("sendResultUnknown");
  expect(leavingIsBlocked()).toBe(true);
  const failedId = useComposerStore.getState().failedSendsByGroup["g-a"][0].id;
  act(() => useComposerStore.getState().discardFailedSend("g-a", failedId));
  expect(leavingIsBlocked()).toBe(false);
});

it("keeps a newly attached file when an earlier send fails and the text is still empty", async () => {
  const finish = delayedSend();
  const attachment = new File(["new file"], "draft.txt", { type: "text/plain" });
  await act(async () => useComposerStore.getState().setComposerText("message A"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  await act(async () => useComposerStore.getState().setComposerFiles([attachment]));
  await act(async () => {
    finish(failed);
    await pending;
  });
  expect(useComposerStore.getState().composerFiles).toEqual([attachment]);
  expect(useComposerStore.getState().composerText).toBe("");
  expect(useComposerStore.getState().failedSendsByGroup["g-a"][0].composerText).toBe("message A");
});

it("keeps multiple failures and requires an empty composer before recovery", async () => {
  const finishA = delayedSend();
  await act(async () => useComposerStore.getState().setComposerText("message A"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  await act(async () => useComposerStore.getState().setComposerText("message B"));
  await act(async () => {
    finishA(failed);
    await pending;
  });
  const first = useComposerStore.getState().failedSendsByGroup["g-a"][0];
  expect(useComposerStore.getState().restoreFailedSend("g-a", first.id)).toBe(false);
  expect(useComposerStore.getState().composerText).toBe("message B");

  const finishB = delayedSend();
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  await act(async () => useComposerStore.getState().setComposerText("draft C"));
  await act(async () => {
    finishB(failed);
    await pending;
  });
  expect(
    useComposerStore.getState().failedSendsByGroup["g-a"].map((item) => item.composerText),
  ).toEqual(["message A", "message B"]);
  expect(useComposerStore.getState().composerText).toBe("draft C");
});

it("reuses an untouched recovered request and keeps one queue entry after it fails again", async () => {
  const finishA = delayedSend();
  await act(async () => useComposerStore.getState().setComposerText("message A"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  await act(async () => useComposerStore.getState().setComposerText("draft B"));
  await act(async () => {
    finishA(failed);
    await pending;
  });
  const first = useComposerStore.getState().failedSendsByGroup["g-a"][0];
  expect(first.id).toBe(vi.mocked(api.sendMessage).mock.calls[0][5]);

  await act(async () => useComposerStore.getState().clearComposer());
  await act(async () =>
    expect(useComposerStore.getState().restoreFailedSend("g-a", first.id)).toBe(true),
  );
  const finishRetry = delayedSend();
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  await act(async () => useComposerStore.getState().setComposerText("draft C"));
  await act(async () => {
    finishRetry(failed);
    await pending;
  });
  const queue = useComposerStore.getState().failedSendsByGroup["g-a"];
  expect(queue.map((item) => item.id)).toEqual([first.id]);
  expect(vi.mocked(api.sendMessage).mock.calls[1][5]).toBe(first.id);
  expect(useComposerStore.getState().composerText).toBe("draft C");
});

it("captures an ordinary send before a new draft is typed in the same turn", async () => {
  const finish = delayedSend();
  await act(async () => useComposerStore.getState().setComposerText("message A"));
  await act(async () => {
    const pending = chat.sendMessage();
    useComposerStore.getState().setComposerText("draft B");
    finish({ ok: true, result: {} });
    await pending;
  });
  expect(vi.mocked(api.sendMessage).mock.calls[0].slice(0, 2)).toEqual(["g-a", "message A"]);
  expect(useComposerStore.getState().composerText).toBe("draft B");
});

it("captures an ordinary send before the Group changes in the same turn", async () => {
  const finish = delayedSend();
  await act(async () => useComposerStore.getState().setComposerText("message A"));
  await act(async () => {
    const pending = chat.sendMessage();
    useGroupStore.setState({ selectedGroupId: "g-b" });
    useComposerStore.getState().switchGroup("g-a", "g-b");
    useComposerStore.getState().setComposerText("draft B");
    finish({ ok: true, result: {} });
    await pending;
  });
  expect(vi.mocked(api.sendMessage).mock.calls[0].slice(0, 2)).toEqual(["g-a", "message A"]);
  expect(useComposerStore.getState().composerText).toBe("draft B");
});

it("T123: does not replace an origin-Group draft when an earlier send fails after switching", async () => {
  const finish = delayedSend();
  await act(async () => useComposerStore.getState().setComposerText("message A"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  await act(async () => {
    useComposerStore.getState().setComposerText("new draft B");
    useGroupStore.setState({
      selectedGroupId: "g-b",
      groups: [{ group_id: "g-a", title: "Original Group" }],
    });
    useComposerStore.getState().switchGroup("g-a", "g-b");
  });
  await act(async () => {
    finish(failed);
    await pending;
  });
  expect(useComposerStore.getState().drafts["g-a"]?.composerText).toBe("new draft B");
  expect(
    useComposerStore.getState().failedSendsByGroup["g-a"]?.map((item) => item.composerText),
  ).toEqual(["message A"]);
  expect(useUIStore.getState().errorMsg).toMatch(/^Original Group: /);
});

it("labels a thrown send error with its origin after a Group switch", async () => {
  let reject!: (error: Error) => void;
  vi.mocked(api.sendMessage).mockReturnValueOnce(new Promise((_, fail) => (reject = fail)));
  await act(async () => useComposerStore.getState().setComposerText("message A"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
    useGroupStore.setState({ selectedGroupId: "g-b" });
    useComposerStore.getState().switchGroup("g-a", "g-b");
  });
  await act(async () => {
    reject(new Error("transport closed"));
    await pending;
  });
  expect(useUIStore.getState().errorMsg).toBe("g-a: transport closed");
  expect(useComposerStore.getState().failedSendsByGroup["g-a"][0].composerText).toBe("message A");
});

it("keeps a late failure after the chat component has unmounted", async () => {
  const finish = delayedSend();
  await act(async () => useComposerStore.getState().setComposerText("message A"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  await act(async () => {
    useComposerStore.getState().setComposerText("new draft B");
    useGroupStore.setState({ selectedGroupId: "g-b" });
    useComposerStore.getState().switchGroup("g-a", "g-b");
    root.unmount();
  });
  await act(async () => {
    finish(failed);
    await pending;
  });
  expect(useComposerStore.getState().drafts["g-a"]?.composerText).toBe("new draft B");
  expect(
    useComposerStore.getState().failedSendsByGroup["g-a"].map((item) => item.composerText),
  ).toEqual(["message A"]);
});

it("T123: does not replace a new draft when a slash send fails", async () => {
  const finish = delayedSend();
  await act(async () => useComposerStore.getState().setComposerText("/install example"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  expect(api.sendMessage).toHaveBeenCalledTimes(1);
  await act(async () => useComposerStore.getState().setComposerText("new draft B"));
  await act(async () => {
    finish(failed);
    await pending;
  });
  expect(useComposerStore.getState().composerText).toBe("new draft B");
  expect(
    useComposerStore.getState().failedSendsByGroup["g-a"]?.map((item) => item.composerText),
  ).toEqual(["/install example"]);
});

it("T123: does not clear an origin-Group draft when a slash send succeeds after switching", async () => {
  const finish = delayedSend();
  await act(async () => useComposerStore.getState().setComposerText("/install example"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  expect(api.sendMessage).toHaveBeenCalledTimes(1);
  await act(async () => {
    useComposerStore.getState().setComposerText("new draft B");
    useGroupStore.setState({ selectedGroupId: "g-b" });
    useComposerStore.getState().switchGroup("g-a", "g-b");
  });
  await act(async () => {
    finish({ ok: true, result: {} });
    await pending;
  });
  expect(useComposerStore.getState().drafts["g-a"]?.composerText).toBe("new draft B");
});

it("labels a slash send failure with its origin when it returns after a Group switch", async () => {
  const finish = delayedSend();
  await act(async () => useComposerStore.getState().setComposerText("/install example"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
    useGroupStore.setState({ selectedGroupId: "g-b" });
    useComposerStore.getState().switchGroup("g-a", "g-b");
  });
  await act(async () => {
    finish(failed);
    await pending;
  });
  expect(useUIStore.getState().errorMsg).toMatch(/^g-a: /);
  expect(useComposerStore.getState().failedSendsByGroup["g-a"][0].composerText).toBe(
    "/install example",
  );
});

it("T123: does not clear another Group's draft when a dynamic tool succeeds", async () => {
  let finish!: (value: { ok: true; result: Record<string, unknown> }) => void;
  vi.mocked(api.useGroupCapability).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  await act(async () => useComposerStore.getState().setComposerText("/lookup example"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  expect(api.useGroupCapability).toHaveBeenCalledTimes(1);
  await act(async () => {
    useGroupStore.setState({ selectedGroupId: "g-b" });
    useComposerStore.getState().switchGroup("g-a", "g-b");
    useComposerStore.getState().setComposerText("other Group draft B");
  });
  await act(async () => {
    finish({ ok: true, result: {} });
    await pending;
  });
  expect(useComposerStore.getState().composerText).toBe("other Group draft B");
});

it("retains a failed dynamic command if its composer changed while it ran", async () => {
  let finish!: (value: typeof failed) => void;
  vi.mocked(api.useGroupCapability).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  await act(async () => useComposerStore.getState().setComposerText("/lookup example"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
  });
  await act(async () => useComposerStore.getState().setComposerText("new draft B"));
  await act(async () => {
    finish(failed);
    await pending;
  });
  expect(useComposerStore.getState().composerText).toBe("new draft B");
  expect(
    useComposerStore.getState().failedSendsByGroup["g-a"].map((item) => item.composerText),
  ).toEqual(["/lookup example"]);
});

it("labels a dynamic tool failure with its origin when it returns after a Group switch", async () => {
  let finish!: (value: typeof failed) => void;
  vi.mocked(api.useGroupCapability).mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  await act(async () => useComposerStore.getState().setComposerText("/lookup example"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
    useGroupStore.setState({ selectedGroupId: "g-b" });
    useComposerStore.getState().switchGroup("g-a", "g-b");
  });
  await act(async () => {
    finish(failed);
    await pending;
  });
  expect(useUIStore.getState().errorMsg).toMatch(/^g-a: /);
  expect(useComposerStore.getState().failedSendsByGroup["g-a"][0].composerText).toBe(
    "/lookup example",
  );
});

it("labels a thrown dynamic tool error with its origin after a Group switch", async () => {
  let reject!: (error: Error) => void;
  vi.mocked(api.useGroupCapability).mockReturnValueOnce(new Promise((_, fail) => (reject = fail)));
  await act(async () => useComposerStore.getState().setComposerText("/lookup example"));
  let pending!: Promise<void>;
  await act(async () => {
    pending = chat.sendMessage();
    await Promise.resolve();
    useGroupStore.setState({ selectedGroupId: "g-b" });
    useComposerStore.getState().switchGroup("g-a", "g-b");
  });
  await act(async () => {
    reject(new Error("transport closed"));
    await pending;
  });
  expect(useUIStore.getState().errorMsg).toBe("g-a: transport closed");
  expect(useComposerStore.getState().failedSendsByGroup["g-a"][0].composerText).toBe(
    "/lookup example",
  );
});

it("does not block a dynamic tool on an unrelated stale message recipient", async () => {
  vi.mocked(api.useGroupCapability).mockResolvedValueOnce({ ok: true, result: {} });
  await act(async () => {
    useComposerStore.getState().setToText("missing-peer");
    useComposerStore.getState().setComposerText("/lookup example");
  });
  await act(async () => {
    await chat.sendMessage();
  });
  expect(api.useGroupCapability).toHaveBeenCalledTimes(1);
});

it("reuses the ID for an unchanged failed slash message", async () => {
  vi.mocked(api.sendMessage).mockResolvedValue(failed);
  await act(async () => useComposerStore.getState().setComposerText("/install example"));
  await act(async () => {
    await chat.sendMessage();
  });
  expect(useComposerStore.getState().composerText).toBe("/install example");
  await act(async () => {
    await chat.sendMessage();
  });
  const ids = vi.mocked(api.sendMessage).mock.calls.map((args) => args[5]);
  expect(ids[0]).toMatch(/^local_/);
  expect(ids[1]).toBe(ids[0]);
});
afterEach(async () => {
  await act(async () => {
    root.unmount();
  });
  host.remove();
});

it("sends composer files to another Group instead of refusing them", async () => {
  vi.mocked(api.sendCrossGroupMessage).mockResolvedValue({ ok: true, result: {} });
  const file = new File(["png"], "chart.png", { type: "image/png" });
  await act(async () => {
    useComposerStore.getState().setDestGroupId("g-b");
    useComposerStore.getState().setComposerText("see chart");
    useComposerStore.getState().setComposerFiles([file]);
  });

  await act(async () => {
    await chat.sendMessage();
  });

  expect(useUIStore.getState().errorMsg).toBe("");
  expect(api.sendCrossGroupMessage).toHaveBeenCalledTimes(1);
  const [source, destination, text, , , files] = vi.mocked(api.sendCrossGroupMessage).mock.calls[0];
  expect([source, destination, text]).toEqual(["g-a", "g-b", "see chart"]);
  expect(files).toEqual([file]);
});
