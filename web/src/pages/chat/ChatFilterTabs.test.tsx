// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { pendingFilterIndicator } from "../../features/pendingReplies/indicator";
import type { Summary } from "../../features/pendingReplies/usePendingSummary";
import type { ChatFilter } from "../../stores/useUIStore";
import { ChatFilterTabs } from "./ChatFilterTabs";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

function Fixture({ summary, enabled = true }: { summary: Summary | undefined; enabled?: boolean }) {
  const [filter, setFilter] = useState<ChatFilter>("all");
  return (
    <ChatFilterTabs
      filter={filter}
      onChange={setFilter}
      waiting={enabled ? pendingFilterIndicator(summary, filter === "pending_user") : null}
      isSmallScreen={false}
    />
  );
}

it("shows waiting first when needed and retains the active view after the last answer", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = (summary: Summary | undefined, enabled = true) =>
    act(async () => root.render(<Fixture summary={summary} enabled={enabled} />));
  const buttons = () => [...host.querySelectorAll<HTMLButtonElement>("button")];
  try {
    await render({ state: "fresh", pending: 0 });
    expect(buttons().map((button) => button.textContent)).toEqual([
      "filterAll",
      "filterUser",
      "filterMail",
      "filterNeedReply",
    ]);
    await render({ state: "fresh", pending: 2 });
    expect(buttons()[0].textContent).toBe("filterWaitingForYou2");
    await act(async () => buttons()[0].click());
    await render({ state: "fresh", pending: 0 });
    expect(buttons()[0].textContent).toBe("filterWaitingForYou0");
    expect(buttons()[0].getAttribute("aria-pressed")).toBe("true");
    await act(async () => buttons()[1].click());
    expect(buttons()[0].textContent).toBe("filterAll");
    await render({ state: "stale", pending: 2 });
    expect(buttons()[0].textContent).toBe("filterWaitingForYou~2");
    await render(undefined);
    expect(buttons()[0].textContent).toBe("filterWaitingForYou?");
    await render(undefined, false);
    expect(buttons()[0].textContent).toBe("filterAll");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
