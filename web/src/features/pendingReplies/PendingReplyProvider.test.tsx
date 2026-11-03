// @vitest-environment happy-dom
import { act, useContext } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { PendingReplyProvider } from "./PendingReplyProvider";
import { PendingSummaryContext } from "./context";
import { PendingReplyStoreContext, replyKey, type PendingReplyStoreApi } from "./store";
import { useUIStore, getChatSession } from "../../stores/useUIStore";
import { GroupPendingIndicator } from "./PendingIndicator";
import { usePendingReplyIndicators } from "./useVisiblePendingSummary";
import { usePendingGroupLabel } from "./usePendingGroupLabel";
import { useNotificationPromptOffer } from "../pushNotifications/useNotificationPromptOffer";
import { ChatFilterTabs } from "../../pages/chat/ChatFilterTabs";

const fetchSummary = vi.hoisted(() => vi.fn());
vi.mock("../../services/api/pendingReplies", () => ({ fetchPendingReplySummary: fetchSummary }));
vi.mock("../pushNotifications/subscription", () => ({
  PUSH_STATUS_EVENT: "test-push-status",
  pushSupported: () => true,
  readPushSetupStatus: async () => "disabled",
}));

it("hides entry points across groups without removing reply data or drafts", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const original = useUIStore.getState();
  const stored = { ...localStorage };
  useUIStore.setState({
    showPendingPrompts: true,
    showPendingReplyReminders: true,
    chatSessions: {},
  });
  fetchSummary.mockReset().mockResolvedValue({
    ok: true,
    result: {
      groups: [
        { group_id: "a", state: "fresh", pending: 2, revision: "a1" },
        { group_id: "b", state: "unknown", pending: null, revision: "b1" },
      ],
    },
  });
  let replyStore: PendingReplyStoreApi | null = null;
  let rawSummary: unknown;
  function Entries() {
    replyStore = useContext(PendingReplyStoreContext);
    rawSummary = useContext(PendingSummaryContext);
    const filter = useUIStore((state) => getChatSession("a", state.chatSessions).chatFilter);
    const { waitingIndicator, hasPendingReplies } = usePendingReplyIndicators(
      "a",
      filter === "pending_user",
    );
    const notification = useNotificationPromptOffer(hasPendingReplies);
    const label = usePendingGroupLabel("b", "Beta");
    return (
      <>
        <GroupPendingIndicator groupId="a" groupName="Alpha" />
        <GroupPendingIndicator groupId="b" groupName="Beta" collapsed />
        <span data-group-label>{label}</span>
        <span data-notification-offer>{String(notification.visible)}</span>
        <ChatFilterTabs
          filter={filter}
          isSmallScreen={false}
          onChange={(value) => useUIStore.getState().setChatFilter("a", value)}
          waiting={waitingIndicator}
        />
      </>
    );
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () =>
      root.render(
        <PendingReplyProvider enabled groupIds={["a", "b"]}>
          <Entries />
        </PendingReplyProvider>,
      ),
    );
    const store = replyStore!;
    const summary = rawSummary;
    await act(async () => {
      store
        .getState()
        .setDraft(replyKey("a", "request"), {
          selection: { kind: "other" },
          text: "keep my reply",
        });
      useUIStore.getState().setChatFilter("a", "pending_user");
    });
    const data = store.getState().entries;
    expect(host.querySelector("[data-notification-offer]")?.textContent).toBe("true");
    expect(host.querySelectorAll("[data-pending-state]")).toHaveLength(3);
    expect(host.querySelector('[data-pending-state="unknown"]')?.textContent).toBe("?");
    expect(host.querySelector("[data-group-label]")?.textContent).not.toBe("Beta");
    await act(async () => useUIStore.getState().setShowPendingReplyReminders(false));
    expect(host.querySelector("[data-pending-state]")).toBeNull();
    expect(host.querySelector("[data-group-label]")?.textContent).toBe("Beta");
    expect(getChatSession("a", useUIStore.getState().chatSessions).chatFilter).toBe("all");
    expect(host.querySelector('button[aria-pressed="true"]')?.textContent).toMatch(
      /filterAll|All|全部/,
    );
    expect(replyStore).toBe(store);
    expect(host.querySelector("[data-notification-offer]")?.textContent).toBe("true");
    expect(rawSummary).toBe(summary);
    expect(store.getState().entries).toBe(data);
    await act(async () => useUIStore.getState().setShowPendingPrompts(false));
    expect(host.querySelector("[data-notification-offer]")?.textContent).toBe("false");
    await act(async () => useUIStore.getState().setShowPendingReplyReminders(true));
    expect(host.querySelector("[data-pending-state]")).toBeNull();
    expect(host.querySelector("[data-notification-offer]")?.textContent).toBe("false");
    expect(store.getState().entries).toBe(data);
    await act(async () => useUIStore.getState().setShowPendingPrompts(true));
    expect(host.querySelectorAll("[data-pending-state]")).toHaveLength(3);
    expect(host.querySelector("[data-notification-offer]")?.textContent).toBe("true");
    expect(getChatSession("a", useUIStore.getState().chatSessions).chatFilter).toBe("all");
    expect(store.getState().entries).toBe(data);
  } finally {
    await act(async () => root.unmount());
    host.remove();
    useUIStore.setState(original);
    localStorage.clear();
    for (const [key, value] of Object.entries(stored)) localStorage.setItem(key, value);
  }
});

it.each(["permission_denied", "read_only"])(
  "clears drafts on %s and restores access only after an authorized refresh",
  async (denialCode) => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    fetchSummary.mockReset();
    const success = {
      ok: true,
      result: { groups: [{ group_id: "a", state: "fresh", pending: 1, revision: "r1" }] },
    };
    fetchSummary.mockResolvedValueOnce(success);
    let exposed: PendingReplyStoreApi | null = null;
    let refresh: (() => void) | undefined;
    function Reader() {
      exposed = useContext(PendingReplyStoreContext);
      const summary = useContext(PendingSummaryContext);
      if (summary) refresh = summary.refresh;
      return null;
    }
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    try {
      await act(async () =>
        root.render(
          <PendingReplyProvider enabled groupIds={["a"]}>
            <Reader />
          </PendingReplyProvider>,
        ),
      );
      const store = exposed!;
      await act(async () =>
        store
          .getState()
          .setDraft(replyKey("a", "r"), { selection: { kind: "other" }, text: "private" }),
      );
      expect(Object.keys(store.getState().entries)).toHaveLength(1);

      fetchSummary.mockResolvedValueOnce({ ok: false, error: { code: denialCode } });
      await act(async () => refresh!());
      expect(exposed).toBeNull();
      expect(store.getState().entries).toEqual({});

      fetchSummary.mockResolvedValueOnce({ ok: false, error: { code: "NETWORK_ERROR" } });
      await act(async () => refresh!());
      expect(exposed).toBeNull();
      fetchSummary.mockResolvedValueOnce(success);
      await act(async () => refresh!());
      expect(exposed).toBe(store);
      expect(store.getState().entries).toEqual({});
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  },
);
