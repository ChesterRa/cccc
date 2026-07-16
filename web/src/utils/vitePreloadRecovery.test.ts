import { describe, expect, it } from "vite-plus/test";

import { claimVitePreloadReload, isDynamicImportError } from "./vitePreloadRecovery";

function memoryStorage(): Storage {
  const values = new Map<string, string>();
  return {
    get length() {
      return values.size;
    },
    clear: () => values.clear(),
    getItem: (key) => values.get(key) ?? null,
    key: (index) => [...values.keys()][index] ?? null,
    removeItem: (key) => values.delete(key),
    setItem: (key, value) => values.set(key, value),
  };
}

describe("claimVitePreloadReload", () => {
  it("allows one reload per cooldown window", () => {
    const storage = memoryStorage();

    expect(claimVitePreloadReload(storage, 1_000)).toBe(true);
    expect(claimVitePreloadReload(storage, 2_000)).toBe(false);
    expect(claimVitePreloadReload(storage, 16_000)).toBe(true);
  });

  it("does not reload when session storage cannot persist the cooldown", () => {
    const storage = {
      getItem: () => {
        throw new Error("storage disabled");
      },
    } as unknown as Storage;

    expect(claimVitePreloadReload(storage, 1_000)).toBe(false);
  });

  it("does not reload when the cooldown marker cannot be written", () => {
    const storage = {
      getItem: () => null,
      setItem: () => {
        throw new Error("storage disabled");
      },
    } as unknown as Storage;

    expect(claimVitePreloadReload(storage, 1_000)).toBe(false);
  });
});

describe("isDynamicImportError", () => {
  it.each([
    "Failed to fetch dynamically imported module: http://localhost:5555/ui/chunk.js",
    "error loading dynamically imported module: /ui/chunk.js",
    "Importing a module script failed.",
  ])("recognizes a browser module-load failure: %s", (message) => {
    expect(isDynamicImportError(new TypeError(message))).toBe(true);
  });

  it("ignores unrelated runtime errors", () => {
    expect(isDynamicImportError(new TypeError("Cannot read properties of undefined"))).toBe(false);
  });
});
