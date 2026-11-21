// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vite-plus/test";
import {
  type AttachmentPreviewItem,
  attachmentSetKey,
  useAttachmentPreviewStore,
} from "../../stores/useAttachmentPreviewStore";
import type { AttachmentResource } from "../../stores/useResourcePanelStore";
import { loadAttachmentList } from "./loadAttachmentList";

const fetchMessageWindow = vi.fn();
vi.mock("../../services/api", () => ({
  fetchMessageWindow: (...args: unknown[]) => fetchMessageWindow(...args),
}));

// A restored tab: the third attachment of an own message, sent from another Group's blobs.
const tab: AttachmentResource = {
  kind: "attachment",
  messageKey: "client:c1",
  eventId: "ev1",
  index: 2,
  blobGroupId: "g-src",
  blobPath: "state/blobs/c.png",
  sender: "user",
  sentAt: "2026-11-19T10:00:00Z",
  media: "image",
  isSvg: false,
};
const single: AttachmentPreviewItem[] = [{ resource: tab, title: "c.png" }];
const message = (attachments: unknown[]) => ({
  ok: true,
  result: { events: [{ id: "ev1", kind: "chat.message", data: { attachments } }] },
});
const key = attachmentSetKey("g1", "client:c1");

beforeEach(() => {
  fetchMessageWindow.mockReset();
  useAttachmentPreviewStore.setState({ sets: {}, viewing: null });
  useAttachmentPreviewStore.getState().view("g1", "client:c1", single, 2);
});

it("reads only the tab's message and keeps its previewable attachments in order", async () => {
  fetchMessageWindow.mockResolvedValue(
    message([
      { kind: "image", path: "state/blobs/a.png", title: "a.png", mime_type: "image/png" },
      { kind: "file", path: "state/blobs/b.zip", title: "b.zip", mime_type: "application/zip" },
      { kind: "image", path: "state/blobs/c.png", title: "c.png", mime_type: "image/png" },
      { kind: "file", path: "https://example.com/x.pdf", title: "x.pdf" },
      { kind: "file", path: "state/blobs/d.pdf", title: "d.pdf", mime_type: "application/pdf" },
    ]),
  );
  await loadAttachmentList("g1", tab);
  expect(fetchMessageWindow).toHaveBeenCalledWith("g1", "ev1", { before: 0, after: 0 });
  const items = useAttachmentPreviewStore.getState().sets[key];
  expect(items.map((item) => [item.resource.index, item.resource.media, item.title])).toEqual([
    [0, "image", "a.png"],
    [2, "image", "c.png"],
    [4, "pdf", "d.pdf"],
  ]);
  expect(items[0].resource).toMatchObject({
    messageKey: "client:c1",
    eventId: "ev1",
    blobGroupId: "g-src",
    blobPath: "state/blobs/a.png",
    sender: "user",
  });
});

it("keeps nothing once the reader has moved on, or the message gave its own list", async () => {
  const attachments = [
    { kind: "image", path: "state/blobs/a.png", title: "a.png", mime_type: "image/png" },
    { kind: "image", path: "state/blobs/b.png", title: "b.png", mime_type: "image/png" },
    { kind: "image", path: "state/blobs/c.png", title: "c.png", mime_type: "image/png" },
  ];
  fetchMessageWindow.mockResolvedValue(message(attachments));
  const pending = loadAttachmentList("g1", tab);
  useAttachmentPreviewStore.getState().close();
  await pending;
  expect(useAttachmentPreviewStore.getState().sets[key]).toBeUndefined();

  const own = [single[0], { resource: { ...tab, index: 5 }, title: "own.png" }];
  useAttachmentPreviewStore.getState().view("g1", "client:c1", single, 2);
  const late = loadAttachmentList("g1", tab);
  useAttachmentPreviewStore.getState().remember("g1", "client:c1", own);
  await late;
  expect(useAttachmentPreviewStore.getState().sets[key]).toEqual(own);
});

it("leaves a lone attachment, or a failed read, as it is", async () => {
  fetchMessageWindow.mockResolvedValue(
    message([{ kind: "image", path: "state/blobs/c.png", title: "c.png", mime_type: "image/png" }]),
  );
  await loadAttachmentList("g1", tab);
  expect(useAttachmentPreviewStore.getState().sets[key]).toEqual(single);
  fetchMessageWindow.mockRejectedValue(new Error("offline"));
  await loadAttachmentList("g1", tab);
  expect(useAttachmentPreviewStore.getState().sets[key]).toEqual(single);
  // An unconfirmed message has nothing to read yet.
  await loadAttachmentList("g1", { ...tab, eventId: "" });
  expect(fetchMessageWindow).toHaveBeenCalledTimes(2);
});
