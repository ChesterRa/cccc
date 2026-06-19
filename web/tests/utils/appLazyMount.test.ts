import { describe, expect, it } from "vitest";

import { shouldMountAppModals } from "../../src/utils/appLazyMount";

describe("app lazy mount gates", () => {
  it("keeps AppModals out of the default path when nothing modal-like is active", () => {
    expect(shouldMountAppModals({ modals: { settings: false, search: false } })).toBe(false);
  });

  it("mounts AppModals for modal flags and non-modal modal state", () => {
    expect(shouldMountAppModals({ modals: { settings: true } })).toBe(true);
    expect(shouldMountAppModals({ modals: {}, recipientsEventId: "evt_1" })).toBe(true);
    expect(shouldMountAppModals({ modals: {}, presentationViewer: { slotId: "slot-1" } })).toBe(true);
    expect(shouldMountAppModals({ modals: {}, presentationPin: { slotId: "slot-1" } })).toBe(true);
    expect(shouldMountAppModals({ modals: {}, editingActor: { id: "peer-1" } })).toBe(true);
  });
});
