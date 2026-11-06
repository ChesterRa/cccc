import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { getAutomaticTheme, subscribeToAutomaticTheme } from "../../src/utils/theme";

function stubSystemScheme(dark: boolean) {
  const listeners = new Set<() => void>();
  const scheme = {
    matches: dark,
    addEventListener: (_type: string, listener: () => void) => listeners.add(listener),
    removeEventListener: (_type: string, listener: () => void) => listeners.delete(listener),
  };
  vi.stubGlobal("window", { matchMedia: () => scheme });
  return {
    change(nextDark: boolean) {
      scheme.matches = nextDark;
      listeners.forEach((listener) => listener());
    },
  };
}

describe("automatic theme", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("follows the system's appearance setting", () => {
    stubSystemScheme(true);
    expect(getAutomaticTheme()).toBe("dark");
    stubSystemScheme(false);
    expect(getAutomaticTheme()).toBe("light");
  });

  it("reports a system change until it is unsubscribed", () => {
    const system = stubSystemScheme(false);
    const changed = vi.fn();
    const unsubscribe = subscribeToAutomaticTheme(changed);

    system.change(true);
    expect(changed).toHaveBeenCalledTimes(1);
    expect(getAutomaticTheme()).toBe("dark");

    unsubscribe();
    system.change(false);
    expect(changed).toHaveBeenCalledTimes(1);
  });
});
