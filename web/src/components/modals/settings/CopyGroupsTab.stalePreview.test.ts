import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "CopyGroupsTab.tsx"), "utf8");

describe("CopyGroupsTab stale preview handling", () => {
  it("guards preview responses by request id and cleans up stale staged uploads", () => {
    expect(source).toContain("previewRequestSeqRef");
    expect(source).toContain("requestId !== previewRequestSeqRef.current");
    expect(source).toContain("cleanupStagedUploadId(nextUploadId)");
    expect(source).toContain("copyPreview.file === copyFile");
  });
});
