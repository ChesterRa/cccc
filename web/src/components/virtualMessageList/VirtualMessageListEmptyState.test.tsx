// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { VirtualMessageListEmptyState } from "./VirtualMessageListEmptyState";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("VirtualMessageListEmptyState", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("offers one explicit history request when older filtered results may exist", async () => {
    const loadMore = vi.fn();
    await act(async () =>
      root.render(
        <VirtualMessageListEmptyState
          isLoadingHistory={false}
          hasMoreHistory
          isFilteredEmpty
          onLoadMore={loadMore}
        />,
      ),
    );

    const button = host.querySelector("button");
    expect(host.textContent).toContain("noResults");
    expect(host.textContent).not.toContain("emptyStateQuickNoteTitle");
    expect(button?.textContent).toBe("loadOlderResults");
    button?.click();
    expect(loadMore).toHaveBeenCalledOnce();
  });

  it("shows progress instead of another request while history is loading", async () => {
    await act(async () =>
      root.render(
        <VirtualMessageListEmptyState
          isLoadingHistory
          hasMoreHistory
          isFilteredEmpty={false}
          onLoadMore={vi.fn()}
        />,
      ),
    );

    expect(host.textContent).toContain("loadingHistory");
    expect(host.querySelector("button")).toBeNull();
  });
});
