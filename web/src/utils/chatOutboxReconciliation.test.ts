import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { getOutboxEntry, useChatOutboxStore } from "../stores/chatOutboxStore";
import { resourceKey, useResourcePanelStore } from "../stores/useResourcePanelStore";
import type { LedgerEvent } from "../types";
import {
  completeCanonicalOutboxReconciliation,
  reconcileCanonicalOutboxEvent,
} from "./chatOutboxReconciliation";

describe("canonical outbox reconciliation", () => {
  afterEach(() => {
    useChatOutboxStore.getState().clearAll();
    useResourcePanelStore.setState({ groups: {} });
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("replaces pending state at once and releases its local image preview a minute later", () => {
    vi.useFakeTimers();
    const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    const optimistic: LedgerEvent = {
      id: "client-1",
      kind: "chat.message",
      by: "user",
      data: {
        client_id: "client-1",
        text: "image",
        attachments: [{ path: "", local_preview_url: "blob:preview", mime_type: "image/png" }],
      },
    };
    useChatOutboxStore.getState().enqueue("g1", "client-1", optimistic);
    const canonical: LedgerEvent = {
      id: "event-1",
      kind: "chat.message",
      by: "user",
      data: {
        client_id: "client-1",
        text: "image",
        attachments: [{ path: "state/blobs/hash", mime_type: "image/png" }],
      },
    };

    const reconciliation = reconcileCanonicalOutboxEvent(canonical, "g1");
    completeCanonicalOutboxReconciliation("g1", reconciliation);

    // The confirmed message previews its stored copy; the local one is not carried over, and it
    // stays valid long enough for a raster image on screen to switch without a blank frame.
    expect(reconciliation.event).toBe(canonical);
    expect(getOutboxEntry("g1", "client-1")).toBeNull();
    expect(revoke).not.toHaveBeenCalled();
    vi.advanceTimersByTime(60_000);
    expect(revoke).toHaveBeenCalledWith("blob:preview");
  });

  it("updates open and recently closed resources before their optimistic URLs are revoked", () => {
    vi.useFakeTimers();
    const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    const attachment = (index: number) =>
      ({
        kind: "attachment",
        messageKey: "client:client-1",
        eventId: "",
        index,
        blobGroupId: "g1",
        blobPath: "",
        localPreviewUrl: `blob:preview-${index}`,
        media: "image",
        isSvg: false,
      }) as const;
    const panel = useResourcePanelStore.getState();
    panel.open("g1", attachment(1), "one.png");
    panel.open("g1", attachment(3), "two.png");
    panel.closeTabs("g1", [resourceKey(attachment(3))]);
    panel.setOpen("g1", false);
    const generation = useResourcePanelStore.getState().generation;
    const optimistic = {
      id: "client-1",
      kind: "chat.message",
      by: "user",
      data: {
        client_id: "client-1",
        attachments: [
          { path: "not-a-stored-blob" },
          { path: "", local_preview_url: "blob:preview-1" },
          null,
          { path: "", local_preview_url: "blob:preview-3" },
        ],
      },
    } as unknown as LedgerEvent;
    useChatOutboxStore.getState().enqueue("g1", "client-1", optimistic);
    const canonical = {
      id: "event-1",
      group_id: "blob-owner",
      kind: "chat.message",
      by: "user",
      data: {
        client_id: "client-1",
        attachments: [
          { path: "not-a-stored-blob" },
          { path: "state/blobs/one" },
          null,
          { path: "state/blobs/two" },
        ],
      },
    } as unknown as LedgerEvent;

    const reconciliation = reconcileCanonicalOutboxEvent(canonical, "g1");
    completeCanonicalOutboxReconciliation("g1", reconciliation);
    const state = useResourcePanelStore.getState();
    expect(state.generation).toBe(generation);
    expect(state.groups.g1.open).toBe(false);
    expect(state.groups.g1.activeKey).toBe(resourceKey(attachment(1)));
    expect(state.groups.g1.tabs).toHaveLength(1);
    expect(state.groups.g1.tabs[0]).toMatchObject({
      eventId: "event-1",
      blobGroupId: "blob-owner",
      blobPath: "state/blobs/one",
      localPreviewUrl: undefined,
    });
    expect(state.groups.g1.closed[0].tabs[0]).toMatchObject({
      eventId: "event-1",
      blobGroupId: "blob-owner",
      blobPath: "state/blobs/two",
      localPreviewUrl: undefined,
    });
    vi.advanceTimersByTime(60_000);
    expect(revoke).toHaveBeenCalledTimes(2);
    expect(JSON.stringify(state.groups.g1)).not.toContain("blob:preview");
  });
});
