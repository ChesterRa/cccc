import { expect, it } from "vite-plus/test";
import { attachmentDocumentKind, isVideoAttachment } from "./messageAttachments";

it("recognizes videos by type or original name, never images or other files", () => {
  const hash = "state/blobs/0123456789abcdef";
  expect(isVideoAttachment({ kind: "file", path: hash, title: "clip.MP4", mime_type: "" })).toBe(
    true,
  );
  expect(
    isVideoAttachment({ kind: "file", path: hash, title: "clip", mime_type: "video/webm" }),
  ).toBe(true);
  expect(isVideoAttachment({ kind: "file", path: hash, title: "take.mov" })).toBe(true);
  expect(
    isVideoAttachment({ kind: "image", path: hash, title: "poster.mp4", mime_type: "image/png" }),
  ).toBe(false);
  expect(
    isVideoAttachment({ kind: "file", path: hash, title: "notes.txt", mime_type: "text/plain" }),
  ).toBe(false);
});

it("opens documents in the page and leaves media and unknown files to their own paths", () => {
  const file = (title: string, mime_type = "") => ({
    kind: "file",
    path: "state/blobs/0123456789abcdef",
    title,
    mime_type,
  });
  expect(attachmentDocumentKind(file("Notes.MD"))).toBe("markdown");
  expect(attachmentDocumentKind(file("notes", "text/markdown; charset=utf-8"))).toBe("markdown");
  expect(attachmentDocumentKind(file("report.pdf"))).toBe("pdf");
  expect(attachmentDocumentKind(file("report", "application/pdf"))).toBe("pdf");
  expect(attachmentDocumentKind(file("run.log"))).toBe("text");
  expect(attachmentDocumentKind(file("data.json", "application/json"))).toBe("text");
  expect(attachmentDocumentKind(file("voice.mp3"))).toBeNull();
  expect(attachmentDocumentKind(file("diagram.svg"))).toBeNull();
  expect(attachmentDocumentKind(file("page.html", "text/html"))).toBeNull();
  expect(attachmentDocumentKind(file("archive.zip"))).toBeNull();
});
