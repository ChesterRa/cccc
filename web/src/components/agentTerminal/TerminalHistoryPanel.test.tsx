// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

const api = vi.hoisted(() => ({ fetchTerminalHistory: vi.fn() }));

vi.mock("../../services/api", () => ({ fetchTerminalHistory: api.fetchTerminalHistory }));

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

import { TerminalHistoryPanel } from "./TerminalHistoryPanel";

function page(text: string, startCursor: number, hasMore = true, expired = false) {
  return {
    ok: true as const,
    result: {
      text,
      hint: "",
      start_cursor: startCursor,
      end_cursor: startCursor + 100,
      has_more: hasMore,
      cursor_expired: expired,
    },
  };
}

async function render() {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <TerminalHistoryPanel
        groupId="g_alpha"
        actorId="claude-1"
        actorTitle="Claude"
        isDark
        onClose={vi.fn()}
      />,
    );
  });
  return { host, root };
}

describe("TerminalHistoryPanel", () => {
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    api.fetchTerminalHistory.mockReset();
  });

  it("loads the newest page with ANSI stripped and shows it", async () => {
    api.fetchTerminalHistory.mockResolvedValueOnce(page("newest output", 900));
    const { host, root } = await render();

    expect(api.fetchTerminalHistory).toHaveBeenCalledWith(
      "g_alpha",
      "claude-1",
      expect.objectContaining({ before: null, stripAnsi: true }),
    );
    expect(host.querySelector("pre")?.textContent).toBe("newest output");

    await act(async () => root.unmount());
    host.remove();
  });

  it("prepends older text when the reader scrolls to the top", async () => {
    api.fetchTerminalHistory
      .mockResolvedValueOnce(page("newer", 900))
      .mockResolvedValueOnce(page("older\nnewer", 400));
    const { host, root } = await render();

    const scroller = host.querySelector<HTMLElement>(".overflow-auto")!;
    scroller.scrollTop = 0;
    await act(async () => {
      scroller.dispatchEvent(new Event("scroll", { bubbles: true }));
    });

    expect(api.fetchTerminalHistory).toHaveBeenCalledTimes(2);
    // Second request pages backwards from the first page's start cursor.
    expect(api.fetchTerminalHistory.mock.calls[1][2]).toEqual(
      expect.objectContaining({ before: 900, renderBefore: 1000 }),
    );
    // Older text is prepended, not appended.
    expect(host.querySelector("pre")?.textContent).toBe("older\nnewer");

    await act(async () => root.unmount());
    host.remove();
  });

  it("stops paging once the server reports the start of the backlog", async () => {
    api.fetchTerminalHistory.mockResolvedValueOnce(page("all there is", 0, false));
    const { host, root } = await render();

    const scroller = host.querySelector<HTMLElement>(".overflow-auto")!;
    scroller.scrollTop = 0;
    await act(async () => {
      scroller.dispatchEvent(new Event("scroll", { bubbles: true }));
      scroller.dispatchEvent(new Event("scroll", { bubbles: true }));
    });

    expect(api.fetchTerminalHistory).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("historyStart");

    await act(async () => root.unmount());
    host.remove();
  });

  it("loads a short page only once and lets the reader explicitly request older history", async () => {
    // Simulate a laid-out scroller that is taller than its content: nothing
    // to scroll, so onScroll alone could never fetch the next page.
    const clientHeight = vi
      .spyOn(HTMLElement.prototype, "clientHeight", "get")
      .mockReturnValue(600);
    const scrollHeight = vi
      .spyOn(HTMLElement.prototype, "scrollHeight", "get")
      .mockReturnValue(300);
    api.fetchTerminalHistory
      .mockResolvedValueOnce(page("newer", 900))
      .mockResolvedValueOnce(page("older\nnewer", 400, false));
    const { host, root } = await render();

    expect(api.fetchTerminalHistory).toHaveBeenCalledTimes(1);
    const scroller = host.querySelector(".overflow-auto")!;
    await act(async () => scroller.dispatchEvent(new Event("scroll")));
    expect(api.fetchTerminalHistory).toHaveBeenCalledTimes(1);
    const loadOlder = [...host.querySelectorAll("button")].find(
      (button) => button.textContent === "historyLoadOlder",
    )!;
    await act(async () => loadOlder.click());
    expect(api.fetchTerminalHistory).toHaveBeenCalledTimes(2);
    expect(host.querySelector("pre")?.textContent).toBe("older\nnewer");
    expect(host.textContent).toContain("historyStart");

    clientHeight.mockRestore();
    scrollHeight.mockRestore();
    await act(async () => root.unmount());
    host.remove();
  });

  it("surfaces a retry when the request fails", async () => {
    api.fetchTerminalHistory.mockResolvedValueOnce({
      ok: false as const,
      error: { code: "boom", message: "backlog unavailable" },
    });
    const { host, root } = await render();

    expect(host.textContent).toContain("backlog unavailable");
    const retry = [...host.querySelectorAll("button")].find(
      (button) => button.textContent === "historyRetry",
    );
    expect(retry).toBeDefined();

    api.fetchTerminalHistory.mockResolvedValueOnce(page("recovered", 500));
    await act(async () => retry?.click());
    expect(host.querySelector("pre")?.textContent).toBe("recovered");

    await act(async () => root.unmount());
    host.remove();
  });

  it("warns when the backlog rotated past the requested cursor", async () => {
    api.fetchTerminalHistory.mockResolvedValueOnce(page("tail only", 10, true, true));
    const { host, root } = await render();

    expect(host.textContent).toContain("historyTruncated");

    await act(async () => root.unmount());
    host.remove();
  });
});
