// @vitest-environment happy-dom
import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { fullyVisibleMessage, useVoiceViewedMessages } from "./useVoiceViewedMessages";
import { markVoiceMessagesViewed } from "../../services/api/codexVoice";
vi.mock("../../services/api/codexVoice", () => ({
  markVoiceMessagesViewed: vi.fn(async () => ({ ok: true, result: { observed: 1 } })),
}));
(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;
afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it("requires a focused, unoccluded, expanded full message", () => {
  const row = document.createElement("div");
  document.body.append(row);
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  vi.spyOn(row, "getBoundingClientRect").mockReturnValue({
    top: 5,
    left: 5,
    bottom: 80,
    right: 200,
    width: 195,
    height: 75,
  } as DOMRect);
  Object.defineProperty(document, "elementFromPoint", {
    configurable: true,
    value: vi.fn(() => row),
  });
  expect(fullyVisibleMessage(row)).toBe(true);
  row.innerHTML = "<details><summary>Collapsed</summary>Hidden text</details>";
  expect(fullyVisibleMessage(row)).toBe(false);
  row.querySelector("details")!.open = true;
  expect(fullyVisibleMessage(row)).toBe(true);
  vi.spyOn(document, "elementFromPoint").mockReturnValue(document.body);
  expect(fullyVisibleMessage(row)).toBe(false);
  vi.spyOn(document, "hasFocus").mockReturnValue(false);
  expect(fullyVisibleMessage(row)).toBe(false);
});

it("posts an exact source only after dwell, never for a disabled observer", async () => {
  vi.useFakeTimers();
  vi.mocked(markVoiceMessagesViewed).mockClear();
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  function Fixture({ enabled }: { enabled: boolean }) {
    const ref = useRef<HTMLDivElement>(null);
    useVoiceViewedMessages(ref, "group-a", enabled);
    return (
      <div ref={ref}>
        <div data-message-id="event-a" data-voice-viewable="true">
          Message
        </div>
      </div>
    );
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<Fixture enabled={false} />));
  const row = host.querySelector<HTMLElement>("[data-message-id]")!;
  vi.spyOn(row, "getBoundingClientRect").mockReturnValue({
    top: 5,
    left: 5,
    bottom: 80,
    right: 200,
    width: 195,
    height: 75,
  } as DOMRect);
  Object.defineProperty(document, "elementFromPoint", { configurable: true, value: () => row });
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(markVoiceMessagesViewed).not.toHaveBeenCalled();
  await act(async () => root.render(<Fixture enabled />));
  await act(async () => vi.advanceTimersByTimeAsync(1500));
  expect(markVoiceMessagesViewed).not.toHaveBeenCalled();
  await act(async () => vi.advanceTimersByTimeAsync(500));
  expect(markVoiceMessagesViewed).toHaveBeenCalledExactlyOnceWith([
    { group_id: "group-a", event_id: "event-a" },
  ]);
  await act(async () => vi.advanceTimersByTimeAsync(5000));
  expect(markVoiceMessagesViewed).toHaveBeenCalledTimes(1);
  await act(async () => root.unmount());
});
