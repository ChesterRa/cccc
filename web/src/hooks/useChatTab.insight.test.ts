import { describe, expect, it } from "vite-plus/test";
import type { LedgerEvent } from "../types";
import { mergeVisibleChatMessages } from "./useChatTab";

function chatEvent(id: string, data: Record<string, unknown>, streaming = false): LedgerEvent {
  return {
    id,
    ts: "2026-07-15T00:00:00Z",
    kind: "chat.message",
    group_id: "g-1",
    by: "peer1",
    data,
    _streaming: streaming,
  };
}

describe("final chat.message Insight reconciliation", () => {
  it("keeps the canonical Insight while suppressing the matching stream row", () => {
    const stream = chatEvent(
      "stream:s-1",
      { text: "Final body", stream_id: "s-1", pending_event_id: "pending-1" },
      true,
    );
    const canonical = chatEvent("evt-1", {
      text: "Final body",
      insight: "The final frame still deserves independent review.",
      stream_id: "s-1",
      pending_event_id: "pending-1",
    });

    const merged = mergeVisibleChatMessages([canonical], [stream], [], {
      map: new Map<string, number>(),
      next: 0,
    });

    expect(merged).toHaveLength(1);
    expect(merged[0]).toBe(canonical);
    const mergedData = merged[0]?.data as { insight?: string } | undefined;
    expect(mergedData?.insight).toBe("The final frame still deserves independent review.");
  });
});
