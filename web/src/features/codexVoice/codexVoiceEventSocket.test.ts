import { afterEach, expect, it, vi } from "vitest";
import { CodexVoiceEventSocket } from "./codexVoiceEventSocket";
import type { CodexVoiceCallInfo } from "../../services/api";

vi.mock("../../services/api", () => ({ getCodexVoiceWebSocketUrl: () => "ws://fixture" }));

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it("preserves a server failure on close and records only safe close metadata", async () => {
  vi.useFakeTimers();
  vi.stubGlobal("window", globalThis);
  vi.stubGlobal("document", { visibilityState: "hidden" });
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const socket = {
    onmessage: null as ((event: { data: string }) => void) | null,
    onclose: null as ((event: Partial<CloseEvent>) => void) | null,
    send: vi.fn(),
    readyState: 1,
  };
  vi.stubGlobal(
    "WebSocket",
    class {
      static OPEN = 1;
      constructor() {
        return socket;
      }
    },
  );
  const failed = vi.fn();
  const client = new CodexVoiceEventSocket(
    { generation: "fixture-generation" } as CodexVoiceCallInfo,
    vi.fn(),
    failed,
    () => false,
  );
  const ready = client.connect();
  const message = (value: unknown) => socket.onmessage?.({ data: JSON.stringify(value) });
  message({ type: "ready" });
  await ready;
  message({ type: "error", code: "analyst_disconnected" });
  socket.onclose?.({ code: 1006, reason: "private close reason", wasClean: false });
  expect(failed).toHaveBeenCalledExactlyOnceWith("analyst_disconnected");
  expect(warn).toHaveBeenCalledWith(
    "Codex Voice control connection ended",
    expect.objectContaining({
      generation: "fixture-generation",
      close_code: 1006,
      was_clean: false,
    }),
  );
  expect(JSON.stringify(warn.mock.calls)).not.toContain("private close reason");
  await vi.advanceTimersByTimeAsync(30_000);
  expect(socket.send).not.toHaveBeenCalled();
});
