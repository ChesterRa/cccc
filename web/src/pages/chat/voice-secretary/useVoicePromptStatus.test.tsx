// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useVoicePromptStatus } from "./useVoicePromptStatus";

const api = vi.hoisted(() => ({ fetch: vi.fn() }));
vi.mock("../../../services/api", () => ({ fetchVoiceAssistantStatus: api.fetch }));

describe("Prompt request observation", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const onStatus = vi.fn();
  const onError = vi.fn();
  function Probe({ groupId = "A", requestId = "request-a", enabled = true }) {
    useVoicePromptStatus({ groupId, requestId, enabled, onStatus, onError });
    return null;
  }
  const render = (props = {}) => act(async () => root.render(<Probe {...props} />));
  const advance = (ms: number) =>
    act(async () => {
      await vi.advanceTimersByTimeAsync(ms);
    });
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.useFakeTimers();
    vi.clearAllMocks();
    api.fetch.mockResolvedValue({ ok: true, result: { secretary_tasks: [] } });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.useRealTimers();
  });
  it("continues observing a queued request beyond three minutes and delivers its later result", async () => {
    await render();
    await advance(240_000);
    const draft = { request_id: "request-a", status: "pending", draft_text: "refined" };
    api.fetch.mockResolvedValue({ ok: true, result: { prompt_draft: draft } });
    await advance(2_000);
    expect(onStatus).toHaveBeenLastCalledWith("A", "request-a", { prompt_draft: draft });
    expect(api.fetch.mock.lastCall).toEqual(["A", { promptRequestId: "request-a" }]);
  });
  it("rejects a delayed response from a replaced request", async () => {
    let resolve!: (value: unknown) => void;
    api.fetch.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    await render();
    await render({ groupId: "B", requestId: "request-b" });
    onStatus.mockClear();
    await act(async () =>
      resolve({ ok: true, result: { prompt_draft: { request_id: "request-a" } } }),
    );
    expect(onStatus).not.toHaveBeenCalled();
    await advance(2000);
    expect(onStatus).toHaveBeenLastCalledWith("B", "request-b", { secretary_tasks: [] });
  });
  it("does not overlap slow reads or apply their result after observation is disabled", async () => {
    let resolve!: (value: unknown) => void;
    api.fetch.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    await render();
    await advance(240_000);
    expect(api.fetch).toHaveBeenCalledTimes(1);
    await render({ enabled: false });
    await act(async () => resolve({ ok: true, result: {} }));
    expect(onStatus).not.toHaveBeenCalled();
  });
  it("surfaces a failed read and resumes read-only observation", async () => {
    api.fetch.mockRejectedValueOnce(new Error("Connection lost"));
    await render();
    expect(onError).toHaveBeenCalledWith("A", "request-a", "Connection lost");
    await advance(2000);
    expect(onStatus).toHaveBeenCalledWith("A", "request-a", { secretary_tasks: [] });
  });
});
