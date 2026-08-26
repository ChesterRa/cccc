// @vitest-environment happy-dom

import { beforeEach, describe, expect, it } from "vite-plus/test";

import { clearAuthToken, setAuthToken } from "../services/api/base";
import { buildMessageAttachmentLinks, safeAttachmentFilename } from "./messageAttachmentLinks";

describe("message attachment links", () => {
  beforeEach(() => clearAuthToken());

  it("keeps preview inline and marks only the download URL as an attachment", () => {
    setAuthToken("dev-token");
    const links = buildMessageAttachmentLinks({
      groupId: "g demo",
      path: "state/blobs/content-hash",
      title: "分析 报告.txt",
      fallbackLabel: "file",
    });

    const preview = new URL(links.previewHref, "https://cccc.example");
    const download = new URL(links.downloadHref, "https://cccc.example");
    expect(preview.pathname).toBe("/api/v1/groups/g%20demo/blobs/content-hash");
    expect(preview.searchParams.get("filename")).toBe("分析 报告.txt");
    expect(preview.searchParams.get("download")).toBeNull();
    expect(preview.searchParams.get("token")).toBeNull();
    expect(download.searchParams.get("filename")).toBe("分析 报告.txt");
    expect(download.searchParams.get("download")).toBe("true");
    expect(download.searchParams.get("token")).toBeNull();
    expect(links.downloadName).toBe("分析 报告.txt");
  });

  it("keeps optimistic blob previews local", () => {
    const links = buildMessageAttachmentLinks({
      groupId: "g1",
      path: "",
      title: "draft.png",
      localPreviewUrl: "blob:preview",
      fallbackLabel: "image",
    });

    expect(links.previewHref).toBe("blob:preview");
    expect(links.downloadHref).toBe("blob:preview");
    expect(links.downloadName).toBe("draft.png");
  });

  it("removes path and header control characters from suggested names", () => {
    expect(safeAttachmentFilename('../folder\\report\r\n"final".txt')).toBe("report_final_.txt");
    expect(safeAttachmentFilename("..", "file")).toBe("file");
  });
});
