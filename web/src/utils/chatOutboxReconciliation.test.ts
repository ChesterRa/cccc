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

  it("replaces pending state immediately while preserving its local image preview", () => {
    vi.useFakeTimers();
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
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

    expect(reconciliation.event.data?.attachments?.[0]?.local_preview_url).toBe("blob:preview");
    expect(getOutboxEntry("g1", "client-1")).toBeNull();
  });
});
