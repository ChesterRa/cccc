import { expect, it } from "vite-plus/test";
import { isVideoAttachment } from "./messageAttachments";

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
