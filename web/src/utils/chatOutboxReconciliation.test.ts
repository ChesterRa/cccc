import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { getOutboxEntry, useChatOutboxStore } from "../stores/chatOutboxStore";
import type { LedgerEvent } from "../types";
import {
  completeCanonicalOutboxReconciliation,
  reconcileCanonicalOutboxEvent,
} from "./chatOutboxReconciliation";

describe("canonical outbox reconciliation", () => {
  afterEach(() => {
    useChatOutboxStore.getState().clearAll();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("replaces pending state at once and releases its local image preview a minute later", () => {
    vi.useFakeTimers();
    const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    const optimistic: LedgerEvent = {
      id: "client-1",
      kind: "chat.message",
      by: "user",
      data: {
        client_id: "client-1",
        text: "image",
        attachments: [{ path: "", local_preview_url: "blob:preview", mime_type: "image/png" }],
      },
    };
    useChatOutboxStore.getState().enqueue("g1", "client-1", optimistic);
    const canonical: LedgerEvent = {
      id: "event-1",
      kind: "chat.message",
      by: "user",
      data: {
        client_id: "client-1",
        text: "image",
        attachments: [{ path: "state/blobs/hash", mime_type: "image/png" }],
      },
    };

    const reconciliation = reconcileCanonicalOutboxEvent(canonical, "g1");
    completeCanonicalOutboxReconciliation("g1", reconciliation);

    // The confirmed message previews its stored copy; the local one is not carried over, and it
    // stays valid long enough for a raster image on screen to switch without a blank frame.
    expect(reconciliation.event).toBe(canonical);
    expect(getOutboxEntry("g1", "client-1")).toBeNull();
    expect(revoke).not.toHaveBeenCalled();
    vi.advanceTimersByTime(60_000);
    expect(revoke).toHaveBeenCalledWith("blob:preview");
  });
});
