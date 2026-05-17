import { describe, expect, it } from "vitest";

import { documentNeedsContentLoad, documentContentLoadingMatches } from "./documentContentLoad";

describe("Voice Secretary document content loading", () => {
  it("detects metadata-only documents that still need content", () => {
    expect(documentNeedsContentLoad({ document_id: "d1", title: "Doc", status: "active", content_chars: 12 })).toBe(true);
    expect(documentNeedsContentLoad({ document_id: "d1", title: "Doc", status: "active", content: "body", content_chars: 12 })).toBe(false);
    expect(documentNeedsContentLoad({ document_id: "d1", title: "Doc", status: "active", content_chars: 0 })).toBe(false);
  });

  it("matches loading path to the active document path", () => {
    expect(documentContentLoadingMatches("docs/a.md", "docs/a.md")).toBe(true);
    expect(documentContentLoadingMatches("docs/a.md", "docs/b.md")).toBe(false);
    expect(documentContentLoadingMatches("", "docs/a.md")).toBe(false);
  });
});
