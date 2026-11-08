import { describe, expect, it } from "vitest";
import type { ResourceTab } from "../../stores/useResourcePanelStore";
import {
  KEEP_DOCUMENTS,
  KEEP_IMAGES,
  NO_RECENT_ATTACHMENTS,
  mountedAttachments,
  noteRecentAttachment,
} from "./attachmentMounts";

const attachment = (key: string, media: "image" | "pdf" | "video" | "markdown" | "text") =>
  ({
    kind: "attachment",
    key,
    title: key,
    messageKey: "m1",
    index: 0,
    media,
    url: `/blob/${key}`,
    name: key,
  }) as unknown as ResourceTab;

function view(tabs: ResourceTab[], order: string[]) {
  let recent = NO_RECENT_ATTACHMENTS;
  for (const key of order) recent = noteRecentAttachment(recent, tabs, key);
  return recent;
}

describe("attachment viewers kept mounted", () => {
  it("keeps the document being read while a series of images is looked through", () => {
    const images = Array.from({ length: KEEP_IMAGES + 2 }, (_, index) =>
      attachment(`img${index}`, "image"),
    );
    const tabs = [attachment("doc", "pdf"), ...images];
    const recent = view(tabs, ["doc", ...images.map((tab) => tab.key)]);
    const mounted = mountedAttachments(tabs, images[images.length - 1].key, recent);
    expect(mounted.has("doc")).toBe(true);
    expect(mounted.has("img0")).toBe(false);
    expect(mounted.has("img1")).toBe(false);
    expect([...mounted].filter((key) => key.startsWith("img"))).toHaveLength(KEEP_IMAGES);
  });

  it("unloads the oldest document once a newer one goes past the limit", () => {
    const docs = Array.from({ length: KEEP_DOCUMENTS + 1 }, (_, index) =>
      attachment(`doc${index}`, index % 2 ? "video" : "pdf"),
    );
    const recent = view(
      docs,
      docs.map((tab) => tab.key),
    );
    const mounted = mountedAttachments(docs, docs[docs.length - 1].key, recent);
    expect(mounted.has("doc0")).toBe(false);
    expect([...mounted]).toEqual(docs.slice(1).map((tab) => tab.key));
  });

  it("mounts a selected attachment again and leaves out tabs that were closed", () => {
    const docs = Array.from({ length: KEEP_DOCUMENTS + 1 }, (_, index) =>
      attachment(`doc${index}`, "markdown"),
    );
    const recent = view(
      docs,
      docs.map((tab) => tab.key),
    );
    // The oldest was unloaded; selecting it mounts it again and makes it the most recent.
    expect(mountedAttachments(docs, "doc0", recent).has("doc0")).toBe(true);
    const again = noteRecentAttachment(recent, docs, "doc0");
    expect(again.documents).toEqual(["doc0", "doc4", "doc3", "doc2"]);
    const open = docs.filter((tab) => tab.key !== "doc4");
    expect(mountedAttachments(open, "doc0", again)).toEqual(new Set(["doc0", "doc2", "doc3"]));
  });

  it("does not let a closed tab hold a place", () => {
    const [a, b, c, d, e] = ["a", "b", "c", "d", "e"].map((key) => attachment(key, "pdf"));
    const recent = view([a, b, c, d], ["a", "b", "c", "d"]);
    // d closes and e opens: the four still open stay mounted.
    const open = [a, b, c, e];
    const next = noteRecentAttachment(recent, open, "e");
    expect(next.documents).toEqual(["e", "c", "b", "a"]);
    expect(mountedAttachments(open, "e", next)).toEqual(new Set(["a", "b", "c", "e"]));
  });

  it("ignores tabs that are not attachments", () => {
    const file = { kind: "workspace", key: "w", title: "w" } as unknown as ResourceTab;
    expect(noteRecentAttachment(NO_RECENT_ATTACHMENTS, [file], "w")).toBe(NO_RECENT_ATTACHMENTS);
    expect(mountedAttachments([file], "w", NO_RECENT_ATTACHMENTS).size).toBe(0);
  });
});
