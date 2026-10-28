// @vitest-environment happy-dom
import { act, useContext } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { PendingReplyProvider } from "./PendingReplyProvider";
import { PendingSummaryContext } from "./context";
import { PendingReplyStoreContext, replyKey, type PendingReplyStoreApi } from "./store";

const fetchSummary = vi.hoisted(() => vi.fn());
vi.mock("../../services/api/pendingReplies", () => ({ fetchPendingReplySummary: fetchSummary }));

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
