// @vitest-environment happy-dom

import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vite-plus/test";

import { MessageAttachments } from "./MessageAttachments";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("MessageAttachments download contract", () => {
  it("preserves the original file name in the URL and download attribute", () => {
    const markup = renderToStaticMarkup(
      <MessageAttachments
        attachments={[
          {
            kind: "file",
            path: "state/blobs/content-hash",
            title: "quarterly report.txt",
            mime_type: "text/plain",
          },
        ]}
        blobGroupId="g1"
        isUserMessage={false}
        isDark={false}
        attachmentKeyPrefix="event-1"
        downloadTitle={(name) => `Download ${name}`}
      />,
    );

    expect(markup).toContain('download="quarterly report.txt"');
    expect(markup).toContain("filename=quarterly+report.txt&amp;download=true");
    expect(markup).toContain(">quarterly report.txt</span>");
  });

  it("uses the named preview URL for images without forcing attachment disposition", () => {
    const markup = renderToStaticMarkup(
      <MessageAttachments
        attachments={[
          {
            kind: "image",
            path: "state/blobs/image-hash",
            title: "现场 图片.png",
            mime_type: "image/png",
          },
        ]}
        blobGroupId="g1"
        isUserMessage={false}
        isDark={false}
        attachmentKeyPrefix="event-2"
        downloadTitle={(name) => `Download ${name}`}
      />,
    );

    expect(markup).toContain("filename=%E7%8E%B0%E5%9C%BA+%E5%9B%BE%E7%89%87.png");
    expect(markup).not.toContain("download=true");
  });
});
