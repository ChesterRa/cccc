// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vite-plus/test";
import { apiForm, apiFrameControl, apiJson } from "../../services/api/base";
import {
  beginFrameAccess,
  endFrameAccess,
  frameAccessPaused,
  resumeFrameAccess,
} from "./frameAccess";

afterEach(() => {
  endFrameAccess();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it("blocks new application requests after the proof deadline, while admission can recover", async () => {
  vi.useFakeTimers();
  const fetch = vi
    .fn()
    .mockImplementation(async () => new Response(JSON.stringify({ ok: true, result: {} })));
  vi.stubGlobal("fetch", fetch);
  beginFrameAccess(new Date(Date.now() + 1000).toISOString());
  await apiJson("/api/v1/groups");
  expect(fetch).toHaveBeenCalledTimes(1);
  vi.setSystemTime(Date.now() + 1001);
  const write = await apiJson("/api/v1/groups/g/settings", { method: "PUT", body: "{}" });
  const upload = await apiForm("/api/v1/groups/g/files", new FormData());
  expect(write.ok).toBe(false);
  expect(upload.ok).toBe(false);
  expect(frameAccessPaused()).toBe(true);
  expect(document.body.inert).toBe(true);
  expect(fetch).toHaveBeenCalledTimes(1);
  expect((await apiFrameControl("/api/v1/connect/frame/resume", { method: "POST" })).ok).toBe(true);
  expect(fetch).toHaveBeenCalledTimes(2);
  resumeFrameAccess(new Date(Date.now() + 1000).toISOString());
  expect(document.body.inert).toBe(false);
  expect((await apiJson("/api/v1/groups")).ok).toBe(true);
  expect(fetch).toHaveBeenCalledTimes(3);
});
