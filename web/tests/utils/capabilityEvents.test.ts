import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  CAPABILITY_CHANGED_STORAGE_KEY,
  publishCapabilityChanged,
  subscribeCapabilityChanged,
} from "../../src/utils/capabilityEvents";

function makeStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
    clear: () => values.clear(),
  };
}

function makeStorageEvent(key: string, newValue: string | null, storageArea: Storage) {
  const event = new Event("storage") as StorageEvent;
  Object.defineProperties(event, {
    key: { value: key },
    newValue: { value: newValue },
    storageArea: { value: storageArea },
  });
  return event;
}

beforeEach(() => {
  const target = new EventTarget();
  const storage = makeStorage() as unknown as Storage;
  vi.stubGlobal("CustomEvent", class TestCustomEvent extends Event {
    detail: unknown;

    constructor(type: string, init?: CustomEventInit) {
      super(type);
      this.detail = init?.detail;
    }
  });
  vi.stubGlobal("window", {
    localStorage: storage,
    addEventListener: target.addEventListener.bind(target),
    removeEventListener: target.removeEventListener.bind(target),
    dispatchEvent: target.dispatchEvent.bind(target),
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("capabilityEvents", () => {
  it("notifies the current window when capabilities change", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeCapabilityChanged("g1", listener);

    publishCapabilityChanged("g1");

    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  it("notifies other windows through storage events", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeCapabilityChanged("g1", listener);

    window.dispatchEvent(makeStorageEvent(
      CAPABILITY_CHANGED_STORAGE_KEY,
      JSON.stringify({ group_id: "g1", nonce: "n1" }),
      window.localStorage,
    ));

    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  it("ignores capability events for other groups", () => {
    const listener = vi.fn();
    const unsubscribe = subscribeCapabilityChanged("g1", listener);

    publishCapabilityChanged("g2");
    window.dispatchEvent(makeStorageEvent(
      CAPABILITY_CHANGED_STORAGE_KEY,
      JSON.stringify({ group_id: "g2", nonce: "n2" }),
      window.localStorage,
    ));

    expect(listener).not.toHaveBeenCalled();
    unsubscribe();
  });
});
