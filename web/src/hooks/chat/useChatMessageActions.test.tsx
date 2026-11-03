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
