import { describe, expect, it } from "vitest";

import { createSseConnectionRegistry } from "./sseConnectionRegistry";

class FakeEventSource {
  closeCount = 0;

  close() {
    this.closeCount += 1;
  }
}

describe("createSseConnectionRegistry", () => {
  it("closes only stale group connections and marks old generations inactive", () => {
    const registry = createSseConnectionRegistry<FakeEventSource>();
    const oldLedger = new FakeEventSource();
    const oldHeadless = new FakeEventSource();
    const nextLedger = new FakeEventSource();

    const oldLedgerToken = registry.set("ledger", "old-group", oldLedger);
    const oldHeadlessToken = registry.set("headless", "old-group", oldHeadless);
    const nextLedgerToken = registry.set("ledger", "new-group", nextLedger);

    registry.closeGroup("old-group");

    expect(oldLedger.closeCount).toBe(1);
    expect(oldHeadless.closeCount).toBe(1);
    expect(nextLedger.closeCount).toBe(0);
    expect(registry.isCurrent(oldLedgerToken)).toBe(false);
    expect(registry.isCurrent(oldHeadlessToken)).toBe(false);
    expect(registry.isCurrent(nextLedgerToken)).toBe(true);
  });
});
