// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { fetchVoiceNotifications } from "../../services/api/codexVoice";
import { CodexVoiceMessageSources } from "./CodexVoiceMessageSources";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../services/api/codexVoice", () => ({
  fetchVoiceNotifications: vi.fn(async () => ({
    ok: true,
    result: {
      pending_count: 0,
      unconfirmed_count: 1,
      suppressed_count: 1,
      messages: [
        {
          source: { group_id: "fixture", event_id: "skipped" },
          by: "worker",
          output_status: "suppressed",
          suppression_reason: "viewed",
        },
        {
          source: { group_id: "fixture", event_id: "result" },
          by: "worker",
          output_status: "unconfirmed",
        },
      ],
    },
  })),
}));

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

describe("Voice notification delivery status", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    vi.clearAllMocks();
  });
  it("shows queued delivery separately from receipt uncertainty and preserves source access", async () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    const open = vi.fn();
    await act(async () =>
      root.render(
        <CodexVoiceMessageSources
          active
          onOpenSource={open}
          outputStatus={{ queued: 1, blocked: "backpressure" }}
        />,
      ),
    );
    expect(host.querySelector("summary")?.textContent).toContain("voicePreferences.queued");
    expect(host.querySelector("summary")?.textContent).toContain("voicePreferences.suppressed");
    expect(host.textContent).toContain("voicePreferences.suppressed_viewed");
    expect(host.textContent).toContain("voicePreferences.status_unconfirmed");
    expect(host.textContent).not.toContain("status_undefined");
    expect(host.querySelector("summary")?.textContent).not.toContain(
      "voicePreferences.unconfirmed",
    );
    expect(host.querySelector('[role="status"]')?.textContent).toContain(
      "voicePreferences.wait_backpressure",
    );
    expect(host.querySelector('[role="status"]')?.textContent).toContain(
      "voicePreferences.unconfirmed",
    );
    host.querySelector("button")?.click();
    expect(open).toHaveBeenCalledWith("fixture", "result");
    await act(async () =>
      root.render(<CodexVoiceMessageSources active outputStatus={{ queued: 0, blocked: null }} />),
    );
    expect(host.querySelector("summary")?.textContent).toContain("voicePreferences.unconfirmed");
    expect(host.querySelector('[role="status"]')).toBeNull();
    await act(async () => root.unmount());
  });
});

it("only refreshes visible notification history and preserves it across backgrounding", async () => {
  vi.useFakeTimers();
  const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
  const host = document.createElement("div");
  const root = createRoot(host);
  const request = vi.mocked(fetchVoiceNotifications);
  request.mockClear();
  try {
    await act(async () => root.render(<CodexVoiceMessageSources active />));
    await act(async () => vi.advanceTimersByTimeAsync(6000));
    expect(request).not.toHaveBeenCalled();
    hidden.mockReturnValue(false);
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    expect(request).toHaveBeenCalledTimes(1);
    const content = host.textContent;
    hidden.mockReturnValue(true);
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    await act(async () => vi.advanceTimersByTimeAsync(6000));
    expect(request).toHaveBeenCalledTimes(1);
    expect(host.textContent).toBe(content);
    let finish!: (value: Awaited<ReturnType<typeof fetchVoiceNotifications>>) => void;
    const response = await request.mock.results[0].value;
    request.mockClear().mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    hidden.mockReturnValue(false);
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    await act(async () => vi.advanceTimersByTimeAsync(6000));
    expect(request).toHaveBeenCalledTimes(1);
    await act(async () => finish(response));
    await act(async () => vi.advanceTimersByTimeAsync(2000));
    expect(request).toHaveBeenCalledTimes(2);
    await act(async () => root.render(<CodexVoiceMessageSources active={false} />));
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    expect(request).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => root.unmount());
    vi.useRealTimers();
    vi.restoreAllMocks();
  }
});
