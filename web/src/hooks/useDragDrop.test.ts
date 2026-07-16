import { describe, expect, it } from "vite-plus/test";
import { partitionAttachments } from "./useDragDrop";

function sizedFile(name: string, size: number): File {
  return { name, size } as File;
}

describe("partitionAttachments", () => {
  it("accepts up to 100 MiB total", () => {
    const first = sizedFile("first.bin", 60 * 1024 * 1024);
    const second = sizedFile("second.bin", 40 * 1024 * 1024);
    expect(partitionAttachments([first, second])).toEqual({
      accepted: [first, second],
      rejected: [],
    });
  });

  it("includes files already attached to the composer", () => {
    const next = sizedFile("next.bin", 30 * 1024 * 1024);
    expect(partitionAttachments([next], 80 * 1024 * 1024)).toEqual({
      accepted: [],
      rejected: [next],
    });
  });
});
