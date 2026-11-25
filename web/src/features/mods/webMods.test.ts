// @vitest-environment happy-dom
import { act } from "react";
import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { WebMod, WebModsListing } from "./webMods";

const apiJson = vi.fn();
vi.mock("../../services/api/base", () => ({
  apiJson: (...args: unknown[]) => apiJson(...args),
  withAuthToken: (url: string) => url,
}));

const { MOD_LINK_ATTR, isModSafeMode, modFileUrl, reconcileModStyles } = await import("./webMods");
const { useWebMods, WEB_MODS_IDLE_POLL_MS, WEB_MODS_POLL_MS } = await import("./useWebMods");

const mod = (id: string, over: Partial<WebMod> = {}): WebMod => ({
  id,
  name: id,
  version: "1",
  api: 1,
  description: "",
  author: "",
  enabled: true,
  style: { path: "style.css", sha256: `${id}0000000000000000000000` },
  script: null,
  error: null,
  ...over,
});
const listing = (mods: WebMod[], revision = "r1"): WebModsListing => ({ api: 1, revision, mods });
const links = () =>
  [...document.head.querySelectorAll<HTMLLinkElement>(`link[${MOD_LINK_ATTR}]`)].map((link) => [
    link.getAttribute(MOD_LINK_ATTR),
    link.getAttribute("href"),
  ]);

beforeEach(() => {
  // Stylesheet links are asserted, not fetched.
  (
    window as unknown as { happyDOM?: { settings: { disableCSSFileLoading: boolean } } }
  ).happyDOM!.settings.disableCSSFileLoading = true;
  document.head.innerHTML = '<link rel="stylesheet" href="/ui/app.css" />';
  apiJson.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

it("loads one stylesheet per enabled, loadable style mod, after the app's own styles", () => {
  reconcileModStyles(
    document,
    listing([
      mod("glass"),
      mod("off", { enabled: false }),
      mod("broken", { error: "mod.json: bad" }),
      mod("script-only", { style: null }),
      mod("wide"),
    ]),
  );
  expect(links().map(([id]) => id)).toEqual(["glass", "wide"]);
  // The app's own stylesheet stays first, so a mod wins at equal specificity.
  expect(document.head.firstElementChild?.getAttribute("href")).toBe("/ui/app.css");
  expect(links()[0][1]).toBe("/api/v1/web_mods/files/glass/style.css?h=glass00000000000");
});

it("swaps an edited style in place and removes mods switched off or gone", () => {
  reconcileModStyles(document, listing([mod("glass"), mod("wide")]));
  const first = document.head.querySelector(`link[${MOD_LINK_ATTR}="glass"]`);
  reconcileModStyles(
    document,
    listing([mod("glass", { style: { path: "style.css", sha256: "edited00000000000000" } })]),
  );
  expect(links()).toEqual([["glass", "/api/v1/web_mods/files/glass/style.css?h=edited0000000000"]]);
  // The same element is reused, so the page never flashes without the style.
  expect(document.head.querySelector(`link[${MOD_LINK_ATTR}="glass"]`)).toBe(first);
  reconcileModStyles(document, listing([mod("glass", { enabled: false })]));
  expect(links()).toEqual([]);
});

it("encodes each part of a mod file path", () => {
  expect(modFileUrl("glass", { path: "assets/a b.css", sha256: "abc" })).toBe(
    "/api/v1/web_mods/files/glass/assets/a%20b.css?h=abc",
  );
});

it("treats only safe=1 as safe mode", () => {
  expect(isModSafeMode("?safe=1")).toBe(true);
  expect(isModSafeMode("?x=1&safe=1")).toBe(true);
  expect(isModSafeMode("")).toBe(false);
  expect(isModSafeMode("?safe=0")).toBe(false);
});

async function mountHook() {
  const host = document.createElement("div");
  const root = createRoot(host);
  function Probe() {
    useWebMods();
    return null;
  }
  await act(async () => root.render(createElement(Probe)));
  return () => act(async () => root.unmount());
}

it("applies mods, then picks up a change on the next poll", async () => {
  vi.useFakeTimers();
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  apiJson.mockResolvedValue({ ok: true, result: listing([mod("glass")], "r1") });
  const unmount = await mountHook();
  expect(links().map(([id]) => id)).toEqual(["glass"]);

  apiJson.mockResolvedValue({
    ok: true,
    result: listing([mod("glass", { enabled: false })], "r2"),
  });
  await act(async () => vi.advanceTimersByTime(WEB_MODS_POLL_MS));
  expect(links()).toEqual([]);
  await unmount();
  const calls = apiJson.mock.calls.length;
  await act(async () => vi.advanceTimersByTime(WEB_MODS_POLL_MS * 3));
  expect(apiJson.mock.calls.length).toBe(calls);
});

it("checks only rarely while no mod is installed", async () => {
  vi.useFakeTimers();
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  apiJson.mockResolvedValue({ ok: true, result: listing([], "none") });
  const unmount = await mountHook();
  expect(apiJson).toHaveBeenCalledTimes(1);
  await act(async () => vi.advanceTimersByTime(WEB_MODS_IDLE_POLL_MS - 1));
  expect(apiJson).toHaveBeenCalledTimes(1);
  // Once a mod is installed, the page switches to quick checks.
  apiJson.mockResolvedValue({ ok: true, result: listing([mod("glass")], "r1") });
  await act(async () => vi.advanceTimersByTime(1));
  expect(apiJson).toHaveBeenCalledTimes(2);
  expect(links().map(([id]) => id)).toEqual(["glass"]);
  await act(async () => vi.advanceTimersByTime(WEB_MODS_POLL_MS));
  expect(apiJson).toHaveBeenCalledTimes(3);
  await unmount();
});

it("loads nothing and asks for nothing in safe mode", async () => {
  window.history.replaceState(null, "", "/?safe=1");
  try {
    const unmount = await mountHook();
    expect(apiJson).not.toHaveBeenCalled();
    expect(links()).toEqual([]);
    await unmount();
  } finally {
    window.history.replaceState(null, "", "/");
  }
});
