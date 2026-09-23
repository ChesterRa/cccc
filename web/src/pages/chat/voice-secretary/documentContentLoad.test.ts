import { describe, expect, it } from "vite-plus/test";

import {
  createDocumentContentLoadTracker,
  documentContentLoadingMatches,
  documentNeedsContentLoad,
} from "./documentContentLoad";

describe("Voice Secretary document content loading", () => {
  it("detects metadata-only documents that still need content", () => {
    expect(
      documentNeedsContentLoad({
        document_id: "d1",
        title: "Doc",
        status: "active",
        content_chars: 12,
      }),
    ).toBe(true);
    expect(
      documentNeedsContentLoad({
        document_id: "d1",
        title: "Doc",
        status: "active",
        content: "body",
        content_chars: 12,
      }),
    ).toBe(false);
    expect(
      documentNeedsContentLoad({
        document_id: "d1",
        title: "Doc",
        status: "active",
        content_chars: 0,
      }),
    ).toBe(false);
  });

  it("lets only the newest overlapping load clear the indicator", () => {
    const tracker = createDocumentContentLoadTracker();
    const first = tracker.begin();
    expect(tracker.end(first)).toBe(true);

    const poll = tracker.begin();
    const stopRefresh = tracker.begin();
    // The superseded load must not clear what the newer load owns...
    expect(tracker.end(poll)).toBe(false);
    // ...and the newer load clears regardless of which one finished first.
    expect(tracker.end(stopRefresh)).toBe(true);

    const orphan = tracker.begin();
    tracker.reset();
    expect(tracker.end(orphan)).toBe(false);
  });

  it("matches loading path to the active document path", () => {
    expect(documentContentLoadingMatches("docs/a.md", "docs/a.md")).toBe(true);
    expect(documentContentLoadingMatches("docs/a.md", "docs/b.md")).toBe(false);
    expect(documentContentLoadingMatches("", "docs/a.md")).toBe(false);
  });
});
