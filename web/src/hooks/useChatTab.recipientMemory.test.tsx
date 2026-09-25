// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import * as api from "../services/api";
import { useComposerStore, useGroupStore, useUIStore } from "../stores";
import { useChatTab } from "./useChatTab";
import type { Actor } from "../types";

vi.mock("../services/api", () => ({ sendMessage: vi.fn(), replyMessage: vi.fn() }));
vi.mock("./useSlashCommandState", () => ({
  useSlashCommandState: () => ({ items: [], loading: false }),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const actors = ["peer-1", "peer-2"].map(
  (id) => ({ id, runtime: "custom", role: "peer", enabled: false }) as Actor,
);
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
function selectGroup(id: string) {
  const previous = useComposerStore.getState().activeGroupId;
  useGroupStore.setState({ selectedGroupId: id });
  useComposerStore.getState().switchGroup(previous, id);
}
function deferredSend() {
  let resolve!: (value: Awaited<ReturnType<typeof api.sendMessage>>) => void;
  vi.mocked(api.sendMessage).mockReturnValue(
    new Promise((done) => {
      resolve = done;
    }),
  );
  return (ok: boolean) =>
    resolve(
      ok
        ? { ok: true, result: {} }
        : { ok: false, error: { code: "test_failure", message: "test failure" } },
    );
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
  useUIStore.setState({ chatSessions: {}, busy: "" });
  useComposerStore.getState().switchGroup(null, "g-a");
  useComposerStore.getState().setToText("peer-1");
  useComposerStore.getState().setComposerText("first message");
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

describe("composer recipient isolation through actual send callbacks", () => {
  it("keeps recipients and sends the same explicit target twice", async () => {
    vi.mocked(api.sendMessage).mockResolvedValue({ ok: true, result: {} });
    await act(async () => {
      await chat.sendMessage();
    });
    expect(useComposerStore.getState().toText).toBe("peer-1");
    await act(async () => {
      useComposerStore.getState().setComposerText("second message");
    });
    await act(async () => {
      await chat.sendMessage();
    });
    expect(vi.mocked(api.sendMessage).mock.calls.map((args) => args.slice(0, 3))).toEqual([
      ["g-a", "first message", ["peer-1"]],
      ["g-a", "second message", ["peer-1"]],
    ]);
  });

  it("does not reset a new group's recipients or erase the next draft when an old send succeeds", async () => {
    const finish = deferredSend();
    let pending!: Promise<void>;
    await act(async () => {
      pending = chat.sendMessage();
    });
    await act(async () => {
      useComposerStore.getState().setToText("peer-2");
      useComposerStore.getState().setComposerText("next a draft");
      selectGroup("g-b");
      useComposerStore.getState().setToText("peer-2");
      useComposerStore.getState().setComposerText("current b draft");
    });
    await act(async () => {
      finish(true);
      await pending;
    });
    expect(useComposerStore.getState()).toMatchObject({
      activeGroupId: "g-b",
      destGroupId: "g-b",
      toText: "peer-2",
      composerText: "current b draft",
    });
    await act(async () => {
      selectGroup("g-a");
    });
    expect(useComposerStore.getState()).toMatchObject({
      toText: "peer-2",
      composerText: "next a draft",
    });
  });

  it("keeps a newer normal choice when restoring and retrying a failed draft", async () => {
    const finish = deferredSend();
    let pending!: Promise<void>;
    await act(async () => {
      pending = chat.sendMessage();
    });
    await act(async () => {
      useComposerStore.getState().setToText("peer-2");
    });
    await act(async () => {
      finish(false);
      await pending;
    });
    expect(useComposerStore.getState()).toMatchObject({
      toText: "peer-1",
      composerText: "first message",
      normalToTextByGroup: { "g-a": "peer-2" },
    });
    vi.mocked(api.sendMessage).mockResolvedValue({ ok: true, result: {} });
    await act(async () => {
      await chat.sendMessage();
    });
    expect(vi.mocked(api.sendMessage).mock.calls[1].slice(0, 3)).toEqual([
      "g-a",
      "first message",
      ["peer-1"],
    ]);
    expect(useComposerStore.getState().toText).toBe("peer-2");
  });

  it("restores failed sends only in their own group with recipients intact", async () => {
    const finish = deferredSend();
    let pending!: Promise<void>;
    await act(async () => {
      pending = chat.sendMessage();
    });
    await act(async () => {
      selectGroup("g-b");
      useComposerStore.getState().setToText("peer-2");
      useComposerStore.getState().setComposerText("b draft");
    });
    await act(async () => {
      finish(false);
      await pending;
    });
    expect(useComposerStore.getState()).toMatchObject({
      activeGroupId: "g-b",
      destGroupId: "g-b",
      toText: "peer-2",
      composerText: "b draft",
    });
    await act(async () => {
      selectGroup("g-a");
    });
    expect(useComposerStore.getState()).toMatchObject({
      toText: "peer-1",
      composerText: "first message",
    });
    await act(async () => {
      useComposerStore.getState().clearComposer();
    });
    expect(useComposerStore.getState().toText).toBe("peer-1");
  });
});
