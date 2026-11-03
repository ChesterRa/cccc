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

  it("plays a video inline from the preview URL and keeps a download link", () => {
    const markup = renderToStaticMarkup(
      <MessageAttachments
        attachments={[
          // A Web upload keeps kind "file"; the name and type identify the video.
          { kind: "file", path: "state/blobs/video-hash", title: "补帧-30帧.mp4", mime_type: "" },
          {
            kind: "file",
            path: "state/blobs/notes-hash",
            title: "notes.txt",
            mime_type: "text/plain",
          },
        ]}
        blobGroupId="g1"
        isUserMessage={false}
        isDark={false}
        attachmentKeyPrefix="event-3"
        downloadTitle={(name) => `Download ${name}`}
      />,
    );

    // A video is a first-frame thumbnail that loads once visible (see VideoPreview tests).
    const thumbnail = /<video[^>]*>/.exec(markup)?.[0] ?? "";
    expect(thumbnail).toContain('preload="none"');
    expect(thumbnail).not.toContain("src=");
    expect(thumbnail).not.toContain("controls");
    expect(markup).toContain('aria-label="playVideo"');
    expect(markup).toContain('download="补帧-30帧.mp4"');
    expect(markup.match(/aria-label="playVideo"/g)).toHaveLength(1);
    expect(markup).toContain(">notes.txt</span>");
  });

  it("downloads a confirmed upload from the stored blob, not its expiring local copy", () => {
    const markup = renderToStaticMarkup(
      <MessageAttachments
        attachments={[
          {
            kind: "file",
            path: "state/blobs/stored-hash",
            title: "clip.mp4",
            mime_type: "video/mp4",
            local_preview_url: "blob:revoked-soon",
          },
        ]}
        blobGroupId="g1"
        isUserMessage
        isDark={false}
        attachmentKeyPrefix="event-4"
        downloadTitle={(name) => `Download ${name}`}
      />,
    );

    expect(markup).not.toContain("blob:revoked-soon");
    expect(markup).toContain(
      "/api/v1/groups/g1/blobs/stored-hash?filename=clip.mp4&amp;download=true",
    );
  });

  it("previews a confirmed image you sent from its stored blob, so it shows again later", () => {
    // Shown again after its local copy is revoked (a filter change, a long list), an image
    // previewed from that copy would fail until the page was reloaded.
    const markup = renderToStaticMarkup(
      <MessageAttachments
        attachments={[
          {
            kind: "image",
            path: "state/blobs/stored-hash",
            title: "shot.png",
            mime_type: "image/png",
            local_preview_url: "blob:revoked-soon",
          },
        ]}
        blobGroupId="g1"
        isUserMessage
        isDark={false}
        attachmentKeyPrefix="event-5"
        downloadTitle={(name) => `Download ${name}`}
      />,
    );

    expect(markup).not.toContain("blob:revoked-soon");
    expect(markup).toContain('src="/api/v1/groups/g1/blobs/stored-hash?filename=shot.png"');
  });
});

describe("MessageAttachments image layout", () => {
  it.each([
    { isUserMessage: false, count: 1, reserveWidth: true },
    { isUserMessage: true, count: 1, reserveWidth: false },
    { isUserMessage: false, count: 2, reserveWidth: false },
  ])(
    "reserves agent single-image preview width: user=$isUserMessage, count=$count",
    ({ isUserMessage, count, reserveWidth }) => {
      const markup = renderToStaticMarkup(
        <MessageAttachments
          attachments={Array.from({ length: count }, (_, index) => ({
            kind: "image" as const,
            path: `state/blobs/image-${index}`,
            title: `image-${index}.png`,
            mime_type: "image/png",
          }))}
          blobGroupId="g1"
          isUserMessage={isUserMessage}
          isDark={false}
          attachmentKeyPrefix="event-image"
          downloadTitle={(name) => `Download ${name}`}
        />,
      );
      const host = document.createElement("div");
      host.innerHTML = markup;
      const imageContainer = host.querySelector("[data-image-frame]")?.parentElement?.parentElement;
      expect(imageContainer).not.toBeNull();
      expect(imageContainer?.classList.contains("max-w-full")).toBe(true);
      expect(imageContainer?.classList.contains("w-[min(30rem,82vw)]")).toBe(reserveWidth);
    },
  );
});

describe("MessageAttachments video layout", () => {
  it.each([
    { isUserMessage: false, reserveWidth: true },
    { isUserMessage: true, reserveWidth: false },
  ])(
    "reserves agent video preview width: user=$isUserMessage",
    ({ isUserMessage, reserveWidth }) => {
      const markup = renderToStaticMarkup(
        <MessageAttachments
          attachments={[
            {
              kind: "file",
              path: "state/blobs/video-hash",
              title: "video.mp4",
              mime_type: "video/mp4",
            },
          ]}
          blobGroupId="g1"
          isUserMessage={isUserMessage}
          isDark={false}
          attachmentKeyPrefix="event-video"
          downloadTitle={(name) => `Download ${name}`}
        />,
      );
      const host = document.createElement("div");
      host.innerHTML = markup;
      const videoContainer = host.querySelector("figure")?.parentElement;
      expect(videoContainer).not.toBeNull();
      expect(videoContainer?.classList.contains("max-w-full")).toBe(true);
      expect(videoContainer?.classList.contains("w-[min(30rem,82vw)]")).toBe(reserveWidth);
    },
  );
});
