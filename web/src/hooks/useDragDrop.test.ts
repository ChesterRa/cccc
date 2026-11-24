import { describe, expect, it } from "vite-plus/test";
import i18n, { i18nReady } from "../i18n";
import { formatAttachmentsTooLargeError, partitionAttachments } from "./useDragDrop";

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

describe("formatAttachmentsTooLargeError", () => {
  it("localizes oversized attachment errors in en, zh, and ja", async () => {
    await i18nReady;
    const original = i18n.language;
    const twoFiles = [sizedFile("big.zip", 101 * 1024 * 1024), sizedFile("", 102 * 1024 * 1024)];
    const fiveFiles = [
      sizedFile("a.bin", 101 * 1024 * 1024),
      sizedFile("b.bin", 101 * 1024 * 1024),
      sizedFile("c.bin", 101 * 1024 * 1024),
      sizedFile("d.bin", 101 * 1024 * 1024),
      sizedFile("e.bin", 101 * 1024 * 1024),
    ];

    try {
      await i18n.changeLanguage("en");
      expect(formatAttachmentsTooLargeError(twoFiles)).toBe(
        "Attachments exceed 100 MiB: big.zip, file",
      );
      expect(formatAttachmentsTooLargeError(fiveFiles)).toBe(
        "Attachments exceed 100 MiB: a.bin, b.bin, c.bin (+2 more)",
      );

      await i18n.changeLanguage("zh");
      expect(formatAttachmentsTooLargeError(twoFiles)).toBe("附件超出 100 MiB 上限：big.zip, 文件");
      expect(formatAttachmentsTooLargeError(fiveFiles)).toBe(
        "附件超出 100 MiB 上限：a.bin, b.bin, c.bin（另有 2 个）",
      );

      await i18n.changeLanguage("ja");
      expect(formatAttachmentsTooLargeError(twoFiles)).toBe(
        "添付ファイルが 100 MiB の上限を超えています：big.zip, ファイル",
      );
      expect(formatAttachmentsTooLargeError(fiveFiles)).toBe(
        "添付ファイルが 100 MiB の上限を超えています：a.bin, b.bin, c.bin（ほか 2 件）",
      );
    } finally {
      await i18n.changeLanguage(original);
    }
  });
});
