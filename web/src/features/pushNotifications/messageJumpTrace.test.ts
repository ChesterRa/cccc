// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import {
  beginMessageJumpNavigation,
  cancelMessageJumpTrace,
  recordMessageJumpFailure,
  startMessageJumpTrace,
  startMessageWindowRequest,
} from "./messageJumpTrace";

const group = "private-group";
const event = "private-event";
let warn: ReturnType<typeof vi.spyOn>;

function start() {
  startMessageJumpTrace(group, event, "worker", {
    workerToPageMs: 30,
    isCurrentGroup: () => true,
    isWaitingForUser: () => false,
  });
  beginMessageJumpNavigation(group, event);
}

function makeVisible() {
  const log = document.createElement("div");
  log.setAttribute("role", "log");
  log.getBoundingClientRect = () => ({ left: 0, right: 300, top: 0, bottom: 300 }) as DOMRect;
  const row = document.createElement("div");
  row.dataset.messageRow = "true";
  row.dataset.messageId = event;
  row.getBoundingClientRect = () => ({ left: 0, right: 100, top: 20, bottom: 80 }) as DOMRect;
  log.append(row);
  document.body.append(log);
}

beforeEach(() => {
  vi.useFakeTimers();
  warn = vi.spyOn(console, "warn").mockImplementation(() => {});
});

afterEach(() => {
  cancelMessageJumpTrace();
  document.body.replaceChildren();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it("does not log a fast jump whose target is in the message viewport", async () => {
  start();
  startMessageWindowRequest(group, event)!("resolved");
  makeVisible();
  await vi.advanceTimersByTimeAsync(1500);
  expect(warn).not.toHaveBeenCalled();
});

it("reports failure without group, event, URL, untrusted error text or message contents", () => {
  start();
  const finish = startMessageWindowRequest(group, event)!;
  recordMessageJumpFailure(group, event, 503, "private error text");
  finish("failed");
  expect(warn).toHaveBeenCalledOnce();
  const [label, body] = warn.mock.calls[0];
  expect(label).toBe("[CCCC message jump]");
  expect(JSON.parse(String(body))).toMatchObject({
    request: "failed",
    http_status: 503,
    error_kind: "http",
    target_at_sample: "not_rendered",
  });
  for (const value of [group, event, "private error text", "/"])
    expect(String(body)).not.toContain(value);
});

it("samples a slow pending request and samples again after it returns", async () => {
  start();
  const finish = startMessageWindowRequest(group, event)!;
  await vi.advanceTimersByTimeAsync(1500);
  expect(JSON.parse(String(warn.mock.calls[0][1]))).toMatchObject({
    phase: "initial",
    request: "pending",
    http_status: null,
    target_at_sample: "not_rendered",
  });
  await vi.advanceTimersByTimeAsync(100);
  finish("resolved");
  makeVisible();
  await vi.advanceTimersByTimeAsync(300);
  expect(warn).toHaveBeenCalledTimes(2);
  expect(JSON.parse(String(warn.mock.calls[1][1]))).toMatchObject({
    phase: "after_response",
    request: "resolved",
    http_status: null,
    target_at_sample: "in_viewport",
  });
});

it("keeps the trace when the Group takes longer than the first sample to open", async () => {
  start();
  await vi.advanceTimersByTimeAsync(1500);
  expect(JSON.parse(String(warn.mock.calls[0][1]))).toMatchObject({
    phase: "initial",
    request: "not_started",
    target_at_sample: "not_rendered",
  });
  await vi.advanceTimersByTimeAsync(600);
  beginMessageJumpNavigation(group, event);
  startMessageWindowRequest(group, event)!("resolved");
  makeVisible();
  await vi.advanceTimersByTimeAsync(300);
  expect(warn).toHaveBeenCalledTimes(2);
  expect(JSON.parse(String(warn.mock.calls[1][1]))).toMatchObject({
    phase: "after_response",
    request: "resolved",
    target_at_sample: "in_viewport",
  });
});

it("labels a hidden tab instead of claiming its target is missing", async () => {
  vi.spyOn(document, "hidden", "get").mockReturnValue(true);
  start();
  startMessageWindowRequest(group, event)!("resolved");
  await vi.advanceTimersByTimeAsync(1500);
  expect(JSON.parse(String(warn.mock.calls[0][1]))).toMatchObject({
    target_at_sample: "page_hidden",
  });
});

it("does not count a zero-area row as visible", async () => {
  start();
  startMessageWindowRequest(group, event)!("resolved");
  makeVisible();
  const row = document.querySelector<HTMLElement>('[data-message-row="true"]')!;
  row.getBoundingClientRect = () => ({ left: 50, right: 50, top: 20, bottom: 80 }) as DOMRect;
  await vi.advanceTimersByTimeAsync(1500);
  expect(JSON.parse(String(warn.mock.calls[0][1]))).toMatchObject({
    target_at_sample: "outside_viewport",
  });
});

it("does not report an old request after a newer target replaces it", () => {
  start();
  const old = startMessageWindowRequest(group, event)!;
  startMessageJumpTrace(group, "new-event", "worker", {
    isCurrentGroup: () => true,
    isWaitingForUser: () => false,
  });
  old("failed");
  expect(warn).not.toHaveBeenCalled();
});
