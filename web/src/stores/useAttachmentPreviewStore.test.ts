import { beforeEach, describe, expect, it } from "vitest";
import {
  type AttachmentPreviewItem,
  attachmentSequence,
  attachmentSetKey,
  useAttachmentPreviewStore,
} from "./useAttachmentPreviewStore";
import { useResourcePanelStore } from "./useResourcePanelStore";

const item = (index: number, media: "image" | "pdf" = "image"): AttachmentPreviewItem => ({
  resource: {
    kind: "attachment",
    messageKey: "m1",
    eventId: "m1",
    index,
    blobGroupId: "g1",
    blobPath: `state/blobs/${index}`,
    media,
    isSvg: false,
  },
  title: `file-${index}`,
});
const items = [item(0), item(2, "pdf"), item(4)];
const preview = () => useAttachmentPreviewStore.getState();
const key = attachmentSetKey("g1", "m1");

describe("attachment preview store", () => {
  beforeEach(() => {
    useAttachmentPreviewStore.setState({ sets: {}, viewing: null });
    useResourcePanelStore.setState({ groups: {}, shownGroupId: "" });
  });

  it("steps through the message's attachments and stops at either end", () => {
    preview().view("g1", "m1", items, 2);
    expect(preview().viewing).toMatchObject({ index: 2, stepped: false });
    preview().step(1);
    expect(preview().viewing).toMatchObject({ index: 4, stepped: true });
    preview().step(1);
    expect(preview().viewing?.index).toBe(4);
    preview().step(-1);
    preview().step(-1);
    preview().step(-1);
    expect(preview().viewing?.index).toBe(0);
  });

  it("reports no sequence for a lone attachment", () => {
    expect(attachmentSequence([item(3)], 3)).toBeNull();
    expect(attachmentSequence(items, 7)).toBeNull();
    expect(attachmentSequence(items, 2)).toMatchObject({ position: 1, count: 3 });
  });

  it("keeps a message's attachments only while the preview or a panel tab shows them", () => {
    preview().view("g1", "m1", items, 0);
    useResourcePanelStore.getState().open("g1", item(2, "pdf").resource, "file-2");
    preview().close();
    // The panel tab still shows the message.
    expect(preview().sets[key]).toHaveLength(3);
    useResourcePanelStore.getState().closeTabs("g1", ["attachment:m1:2"]);
    expect(preview().sets[key]).toBeUndefined();

    preview().view("g1", "m1", items, 0);
    preview().close();
    expect(preview().sets).toEqual({});
  });

  it("keeps the same list when nothing about it changed", () => {
    preview().view("g1", "m1", items, 0);
    const before = preview().sets;
    preview().remember(
      "g1",
      "m1",
      items.map((entry) => ({ ...entry })),
    );
    expect(preview().sets).toBe(before);
    const stored = items.map((entry) => ({
      ...entry,
      resource: { ...entry.resource, blobPath: `${entry.resource.blobPath}-stored` },
    }));
    preview().remember("g1", "m1", stored);
    expect(preview().sets[key][0].resource.blobPath).toBe("state/blobs/0-stored");
  });
});
