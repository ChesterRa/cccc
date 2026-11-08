import { beforeEach, expect, it } from "vite-plus/test";
import { useModalStore } from "./useModalStore";

beforeEach(() => {
  useModalStore.setState({
    presentationViewer: null,
    presentationAttention: {},
    visiblePresentationResources: new Map(),
  });
});

it("keeps a quoted resource's identity when moving its window beside the chat", () => {
  const focusRef = { kind: "presentation_ref" as const, slot_id: "slot-1", title: "second" };
  const store = useModalStore.getState();
  store.setPresentationViewer({
    groupId: "g",
    slotId: "slot-1",
    surface: "modal",
    focusRef,
    focusEventId: "event-1",
    refKey: "event-1:1",
    reuseExistingSession: true,
  });
  const viewer = useModalStore.getState().presentationViewer;
  expect(viewer?.refKey).toBe("event-1:1");
  expect(viewer?.focusRef).toEqual(focusRef);
  if (!viewer) throw new Error("expected a presentation window");
  store.setPresentationViewer({ ...viewer, surface: "split" });
  expect(useModalStore.getState().presentationViewer).toMatchObject({
    surface: "split",
    focusRef,
    focusEventId: "event-1",
    refKey: "event-1:1",
  });
  // Selecting a different slot starts a different viewer without a quoted reference.
  store.setPresentationViewer({ groupId: "g", slotId: "slot-2", surface: "modal" });
  expect(useModalStore.getState().presentationViewer?.refKey).toBeUndefined();
});

it("suppresses attention only for a visible owner of the same Group and slot", () => {
  const first = Symbol("first panel");
  const second = Symbol("second panel");
  const store = useModalStore.getState();
  store.markPresentationSlotAttention("g", "slot-1");
  store.markPresentationSlotAttention("other", "slot-1");
  store.setVisiblePresentationResource(first, { groupId: "g", slotId: "slot-1" });
  expect(useModalStore.getState().presentationAttention).toEqual({ other: { "slot-1": true } });
  store.markPresentationSlotAttention("g", "slot-1");
  store.markPresentationSlotAttention("g", "slot-2");
  expect(useModalStore.getState().presentationAttention.g).toEqual({ "slot-2": true });

  // One surface unmounting must not make the other visible surface count as hidden.
  store.setVisiblePresentationResource(second, { groupId: "g", slotId: "slot-1" });
  store.setVisiblePresentationResource(first, null);
  store.markPresentationSlotAttention("g", "slot-1");
  expect(useModalStore.getState().presentationAttention.g).toEqual({ "slot-2": true });
  store.setVisiblePresentationResource(second, null);
  store.markPresentationSlotAttention("g", "slot-1");
  expect(useModalStore.getState().presentationAttention.g).toEqual({
    "slot-1": true,
    "slot-2": true,
  });
});
