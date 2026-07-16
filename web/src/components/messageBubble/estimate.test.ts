import { describe, expect, it } from "vite-plus/test";
import type { LedgerEvent } from "../../types";
import { estimateMessageRowHeight } from "./estimate";

function message(data: Record<string, unknown>): LedgerEvent {
  return {
    id: "evt-1",
    ts: "2026-07-15T00:00:00Z",
    kind: "chat.message",
    group_id: "g-1",
    by: "peer1",
    data,
  };
}

describe("estimateMessageRowHeight insight projection", () => {
  it("reserves height for the perspective label and plain text", () => {
    const bodyOnly = estimateMessageRowHeight(message({ text: "Main body" }));
    const withInsight = estimateMessageRowHeight(
      message({ text: "Main body", insight: "A perspective that wraps onto the next line." }),
    );

    expect(withInsight).toBeGreaterThan(bodyOnly);
  });
});
