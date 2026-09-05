// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { RuntimeInspectorModal } from "../modals/RuntimeInspectorModal";

const api = vi.hoisted(() => ({ fetchTerminalHistory: vi.fn() }));
vi.mock("../../services/api", () => ({ fetchTerminalHistory: api.fetchTerminalHistory }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
import { TerminalHistoryPanel } from "./TerminalHistoryPanel";

describe("terminal history failure and keyboard boundaries", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.useFakeTimers();
    api.fetchTerminalHistory.mockReset();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });
  const tick = () =>
    act(async () => {
      await vi.advanceTimersByTimeAsync(100);
    });

  it("pauses short-page auto-loading and scrolling after an error until explicit retry", async () => {
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(600);
    vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(300);
    api.fetchTerminalHistory.mockImplementation(
      () =>
        new Promise((resolve) => {
          setTimeout(
            () => resolve({ ok: false, error: { code: "io_error", message: "offline" } }),
            50,
          );
        }),
    );
    await act(async () =>
      root.render(
        <TerminalHistoryPanel
          groupId="g"
          actorId="a"
          actorTitle="Actor"
          isDark={false}
          onClose={vi.fn()}
        />,
      ),
    );
    await tick();
    await tick();
    const scroller = host.querySelector(".overflow-auto")!;
    await act(async () => scroller.dispatchEvent(new Event("scroll", { bubbles: true })));
    expect(api.fetchTerminalHistory).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("offline");
    api.fetchTerminalHistory.mockResolvedValue({
      ok: true,
      result: { text: "recovered", start_cursor: 0, end_cursor: 9, has_more: false },
    });
    const retry = [...host.querySelectorAll("button")].find(
      (button) => button.textContent === "historyRetry",
    )!;
    await act(async () => retry.click());
    expect(api.fetchTerminalHistory).toHaveBeenCalledTimes(2);
    expect(host.querySelector("pre")?.textContent).toBe("recovered");
  });

  it("traps focus, closes only history on Escape and restores its opener", async () => {
    api.fetchTerminalHistory.mockResolvedValue({
      ok: true,
      result: { text: "history", start_cursor: 0, end_cursor: 7, has_more: false },
    });
    const outerClose = vi.fn();
    function Nested() {
      const [open, setOpen] = useState(false);
      return (
        <RuntimeInspectorModal
          isOpen
          isDark={false}
          titleId="runtime-title"
          closeAriaLabel="Close runtime"
          onClose={outerClose}
        >
          <button id="history-opener" onClick={() => setOpen(true)}>
            Open history
          </button>
          {open && (
            <TerminalHistoryPanel
              groupId="g"
              actorId="a"
              actorTitle="Actor"
              isDark={false}
              onClose={() => setOpen(false)}
            />
          )}
        </RuntimeInspectorModal>
      );
    }
    await act(async () => root.render(<Nested />));
    await tick();
    const opener = host.querySelector<HTMLButtonElement>("#history-opener")!;
    opener.focus();
    await act(async () => opener.click());
    await tick();
    const history = host.querySelector('[aria-labelledby="terminal-history-title"]')!;
    expect(history.contains(document.activeElement)).toBe(true);
    const tab = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    await act(async () => document.dispatchEvent(tab));
    expect(tab.defaultPrevented).toBe(true);
    expect(history.contains(document.activeElement)).toBe(true);
    await act(async () =>
      document.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      ),
    );
    await tick();
    expect(host.querySelector('[aria-labelledby="terminal-history-title"]')).toBeNull();
    expect(outerClose).not.toHaveBeenCalled();
    expect(document.activeElement).toBe(opener);
  });
});
