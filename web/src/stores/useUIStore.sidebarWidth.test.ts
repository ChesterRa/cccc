// @vitest-environment happy-dom
import { afterEach, expect, it } from "vite-plus/test";

import { SIDEBAR_DEFAULT_WIDTH, SIDEBAR_MAX_WIDTH, loadSidebarWidth } from "./useUIStore";

const KEY = "cccc-sidebar-width";

afterEach(() => localStorage.clear());

it("opens a sidebar nobody resized at the narrower default", () => {
  expect(loadSidebarWidth()).toBe(SIDEBAR_DEFAULT_WIDTH);
  // 248 was the old default and the old minimum, so it was never picked by dragging.
  localStorage.setItem(KEY, "248");
  expect(loadSidebarWidth()).toBe(SIDEBAR_DEFAULT_WIDTH);
  expect(SIDEBAR_DEFAULT_WIDTH).toBeLessThan(248);
});

it("keeps a width the user chose by widening the sidebar", () => {
  localStorage.setItem(KEY, "300");
  expect(loadSidebarWidth()).toBe(300);
  localStorage.setItem(KEY, "9999");
  expect(loadSidebarWidth()).toBe(SIDEBAR_MAX_WIDTH);
});
