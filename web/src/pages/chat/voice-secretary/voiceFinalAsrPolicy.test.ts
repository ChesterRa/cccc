import { describe, expect, it } from "vite-plus/test";

import { documentFinalAsrDisposition } from "./voiceFinalAsrPolicy";

describe("documentFinalAsrDisposition", () => {
  it("keeps complete live text when segmented final ASR is partial", () => {
    expect(
      documentFinalAsrDisposition({
        partial: true,
        transcript_persistence: "skipped_partial",
        transcript_persisted: false,
      }),
    ).toBe("preserve_live");
  });

  it("retries failed persistence but restores a committed final revision", () => {
    expect(
      documentFinalAsrDisposition({
        transcript_persistence: "failed",
        transcript_persisted: false,
      }),
    ).toBe("retry_persistence");
    expect(
      documentFinalAsrDisposition({
        transcript_persistence: "persisted",
        transcript_persisted: true,
      }),
    ).toBe("restore_persisted");
    expect(documentFinalAsrDisposition({})).toBe("retry_persistence");
  });
});
