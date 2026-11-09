// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { TFunction } from "i18next";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useGroupStore } from "../../stores";
import { useChatMessageActions } from "./useChatMessageActions";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

afterEach(() => window.history.replaceState({}, "", "/"));

// A message that cannot be loaded leaves the view where it was: the reader is told, and the
// address stops naming the message, as it does when its view is left.
it("reports a message it could not open and keeps the address in step with the view", async () => {
  useGroupStore.setState({ groups: [{ group_id: "g" }] as never, selectedGroupId: "g" });
  const showError = vi.fn();
  let finishLate: (opened: boolean) => void = () => undefined;
  const openChatWindow = vi.fn((_groupId: string, eventId: string) =>
    eventId === "late"
      ? new Promise<boolean>((resolve) => (finishLate = resolve))
      : Promise.resolve(eventId === "shown"),
  );
  let actions: ReturnType<typeof useChatMessageActions> | undefined;
  function Probe() {
    actions = useChatMessageActions({
      selectedGroupId: "g",
      actors: [],
      groupSettings: undefined as never,
      groupGeneration: "",
      setChatAtBottom: vi.fn(),
      hasForeman: true,
      inChatWindow: false,
      t: ((key: string) => key) as unknown as TFunction,
      showError,
      showNotice: vi.fn(),
      setDestGroupId: vi.fn(),
      setReplyToText: vi.fn(),
      setReplyTarget: vi.fn(),
      setRecipientsModal: vi.fn(),
      setRelayModal: vi.fn(),
      openChatWindow,
      closeChatWindow: vi.fn(),
      setShowScrollButton: vi.fn(),
      setChatUnreadCount: vi.fn(),
      setChatScrollSnapshot: vi.fn(),
      setNewActorRole: vi.fn(),
      openModal: vi.fn(),
      loadMoreHistory: vi.fn(async () => undefined),
    });
    return null;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Probe />));
    const event = () => new URL(window.location.href).searchParams.get("event");
    // Let the opening settle, as it does before a person looks again.
    const settle = () => act(() => new Promise((resolve) => setTimeout(resolve, 0)));

    await act(async () => actions!.openSourceMessage("g", "shown"));
    await settle();
    expect(event()).toBe("shown");
    expect(showError).not.toHaveBeenCalled();

    await act(async () => actions!.openSourceMessage("g", "missing"));
    await settle();
    expect(openChatWindow).toHaveBeenLastCalledWith("g", "missing");
    expect(showError).toHaveBeenCalledWith("chat:messageWindowFailed");
    expect(event()).toBeNull();

    // A failure that arrives after the reader moved to another Group says nothing there.
    showError.mockClear();
    await act(async () => actions!.openSourceMessage("g", "late"));
    await act(async () => useGroupStore.setState({ selectedGroupId: "other" }));
    await act(async () => finishLate(false));
    await settle();
    expect(showError).not.toHaveBeenCalled();
    expect(event()).toBe("late");
  } finally {
    await act(async () => root.unmount());
  }
});

type ActionInput = Parameters<typeof useChatMessageActions>[0];
const actionInput = (overrides: Partial<ActionInput>): ActionInput => ({
  selectedGroupId: "g",
  actors: [],
  groupSettings: undefined as never,
  groupGeneration: "",
  setChatAtBottom: vi.fn(),
  hasForeman: true,
  inChatWindow: false,
  t: ((key: string) => key) as unknown as TFunction,
  showError: vi.fn(),
  showNotice: vi.fn(),
  setDestGroupId: vi.fn(),
  setReplyToText: vi.fn(),
  setReplyTarget: vi.fn(),
  setRecipientsModal: vi.fn(),
  setRelayModal: vi.fn(),
  openChatWindow: vi.fn(async () => true),
  closeChatWindow: vi.fn(),
  setShowScrollButton: vi.fn(),
  setChatUnreadCount: vi.fn(),
  setChatScrollSnapshot: vi.fn(),
  setNewActorRole: vi.fn(),
  openModal: vi.fn(),
  loadMoreHistory: vi.fn(async () => undefined),
  ...overrides,
});

// Message rows are memoized, so the actions they receive keep one identity across renders of
// the chat; a call still acts on the Group of the latest committed render.
it("keeps the row actions across renders and calls them with the latest input", async () => {
  useGroupStore.setState({
    groups: [{ group_id: "a" }, { group_id: "b" }] as never,
    selectedGroupId: "a",
  });
  const setRelayModal = vi.fn();
  const setRecipientsModal = vi.fn();
  const setDestGroupId = vi.fn();
  const setReplyToText = vi.fn();
  const renders: ReturnType<typeof useChatMessageActions>[] = [];
  function Probe({ groupId, sendTo }: { groupId: string; sendTo: string }) {
    renders.push(
      useChatMessageActions(
        actionInput({
          selectedGroupId: groupId,
          groupSettings: { default_send_to: sendTo } as never,
          setRelayModal,
          setRecipientsModal,
          setDestGroupId,
          setReplyToText,
        }),
      ),
    );
    return null;
  }
  const root = createRoot(document.createElement("div"));
  try {
    await act(async () => root.render(<Probe groupId="a" sendTo="foreman" />));
    await act(async () => root.render(<Probe groupId="b" sendTo="broadcast" />));
    const [first, latest] = [renders[0], renders[renders.length - 1]];
    // Scroll and view handlers run from effects and keep their dependencies.
    const rowActions = [
      "copyMessageLink",
      "startReply",
      "openSourceMessage",
      "showRecipients",
      "relayMessage",
    ] as const;
    for (const name of rowActions) expect(latest[name], name).toBe(first[name]);
    first.relayMessage({ id: "e1", kind: "chat.message" } as never);
    expect(setRelayModal).toHaveBeenCalledWith("e1", "b", expect.objectContaining({ id: "e1" }));
    first.showRecipients("e2");
    expect(setRecipientsModal).toHaveBeenCalledWith("e2");
    // A reply started from a row that rendered earlier uses the current Group and its default
    // recipients, not the ones of the render that made the row.
    act(() =>
      first.startReply({
        id: "e3",
        kind: "chat.message",
        by: "user",
        data: { text: "hi" },
      } as never),
    );
    expect(setDestGroupId).toHaveBeenLastCalledWith("b");
    expect(setReplyToText).toHaveBeenLastCalledWith("");
  } finally {
    act(() => root.unmount());
  }
});

// An action takes its input once, when it starts: a late result after the chat re-rendered is
// still reported through the input that started it.
it("finishes an action with the input it started with", async () => {
  useGroupStore.setState({ groups: [{ group_id: "g" }] as never, selectedGroupId: "g" });
  let finishLate: (opened: boolean) => void = () => undefined;
  const openChatWindow = vi.fn(() => new Promise<boolean>((resolve) => (finishLate = resolve)));
  const startedWith = vi.fn();
  const renderedLater = vi.fn();
  let actions: ReturnType<typeof useChatMessageActions> | undefined;
  function Probe({ showError }: { showError: (message: string) => void }) {
    actions = useChatMessageActions(actionInput({ showError, openChatWindow }));
    return null;
  }
  const root = createRoot(document.createElement("div"));
  try {
    await act(async () => root.render(<Probe showError={startedWith} />));
    act(() => actions?.openSourceMessage("g", "late"));
    await act(async () => root.render(<Probe showError={renderedLater} />));
    await act(async () => {
      finishLate(false);
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    expect(startedWith).toHaveBeenCalledWith("chat:messageWindowFailed");
    expect(renderedLater).not.toHaveBeenCalled();
  } finally {
    act(() => root.unmount());
  }
});
