// @vitest-environment happy-dom
import { afterEach, expect, it } from "vite-plus/test";

import {
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  loadSidebarWidth,
} from "./useUIStore";

const KEY = "cccc-sidebar-width";

afterEach(() => localStorage.clear());

it("opens at the 248 px default when no width is saved", () => {
  expect(SIDEBAR_DEFAULT_WIDTH).toBe(248);
  expect(SIDEBAR_MIN_WIDTH).toBe(200);
  expect(loadSidebarWidth()).toBe(SIDEBAR_DEFAULT_WIDTH);
  localStorage.setItem(KEY, "");
  expect(loadSidebarWidth()).toBe(SIDEBAR_DEFAULT_WIDTH);
  localStorage.setItem(KEY, "not-a-number");
  expect(loadSidebarWidth()).toBe(SIDEBAR_DEFAULT_WIDTH);
});

it("keeps saved narrower and wider widths within bounds", () => {
  localStorage.setItem(KEY, "248");
  expect(loadSidebarWidth()).toBe(248);
  localStorage.setItem(KEY, "200");
  expect(loadSidebarWidth()).toBe(200);
  localStorage.setItem(KEY, "220");
  expect(loadSidebarWidth()).toBe(220);
  localStorage.setItem(KEY, "300");
  expect(loadSidebarWidth()).toBe(300);
  localStorage.setItem(KEY, "100");
  expect(loadSidebarWidth()).toBe(SIDEBAR_MIN_WIDTH);
  localStorage.setItem(KEY, "9999");
  expect(loadSidebarWidth()).toBe(SIDEBAR_MAX_WIDTH);
});
