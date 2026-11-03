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

  it.each(["shot.png", "clip.mp4"])("uses the stored blob once %s is confirmed", (title) => {
    // The local URL is revoked about a minute after confirmation, and a message can be shown
    // again much later, so an image previewed from it would turn into the failure notice.
    const links = buildMessageAttachmentLinks({
      groupId: "g1",
      path: "state/blobs/stored-hash",
      title,
      localPreviewUrl: "blob:preview",
      fallbackLabel: "file",
    });
    expect(links.previewHref).toContain("/api/v1/groups/g1/blobs/stored-hash?");
    expect(links.previewHref).not.toContain("download=true");
    expect(links.downloadHref).toContain("/api/v1/groups/g1/blobs/stored-hash?");
    expect(links.downloadHref).toContain("download=true");
  });

  it("removes path and header control characters from suggested names", () => {
    expect(safeAttachmentFilename('../folder\\report\r\n"final".txt')).toBe("report_final_.txt");
    expect(safeAttachmentFilename("..", "file")).toBe("file");
  });
});
