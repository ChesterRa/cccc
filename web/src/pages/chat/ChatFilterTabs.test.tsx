// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { pendingFilterIndicator } from "../../features/pendingReplies/indicator";
import type { Summary } from "../../features/pendingReplies/usePendingSummary";
import type { ChatFilter } from "../../stores/useUIStore";
import { ChatFilterTabs } from "./ChatFilterTabs";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

function Fixture({
  summary,
  enabled = true,
  pinned = { count: null },
}: {
  summary: Summary | undefined;
  enabled?: boolean;
  pinned?: { count: number | null } | null;
}) {
  const [filter, setFilter] = useState<ChatFilter>("all");
  return (
    <ChatFilterTabs
      filter={filter}
      onChange={setFilter}
      waiting={enabled ? pendingFilterIndicator(summary, filter === "pending_user") : null}
      pinned={pinned}
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
      "filterPinned",
      "filterUser",
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

it("offers Pinned after All with its count, and not where pins are unavailable", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = (pinned: { count: number | null } | null) =>
    act(async () =>
      root.render(<Fixture summary={{ state: "fresh", pending: 2 }} pinned={pinned} />),
    );
  const labels = () => [...host.querySelectorAll("button")].map((button) => button.textContent);
  try {
    await render({ count: 3 });
    expect(labels()).toEqual(["filterWaitingForYou2", "filterAll", "filterPinned3", "filterUser"]);
    await render({ count: 0 });
    expect(labels()[2]).toBe("filterPinned");
    await render(null);
    expect(labels()).toEqual(["filterWaitingForYou2", "filterAll", "filterUser"]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("fades an edge that has more tabs past it", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Fixture summary={{ state: "fresh", pending: 2 }} />));
    const strip = host.firstElementChild as HTMLDivElement;
    expect(strip.dataset.moreAfter).toBeUndefined();
    // A phone shows 200 of the strip's 320 pixels.
    Object.defineProperty(strip, "clientWidth", { configurable: true, value: 200 });
    Object.defineProperty(strip, "scrollWidth", { configurable: true, value: 320 });
    const scrollTo = async (left: number) => {
      strip.scrollLeft = left;
      await act(async () => strip.dispatchEvent(new Event("scroll")));
    };
    await scrollTo(0);
    expect([strip.dataset.moreBefore, strip.dataset.moreAfter]).toEqual([undefined, "true"]);
    await scrollTo(60);
    expect([strip.dataset.moreBefore, strip.dataset.moreAfter]).toEqual(["true", "true"]);
    await scrollTo(120);
    expect([strip.dataset.moreBefore, strip.dataset.moreAfter]).toEqual(["true", undefined]);
    // Two pixels of a tab's padding past the edge hide nothing worth a fade.
    Object.defineProperty(strip, "scrollWidth", { configurable: true, value: 202 });
    await scrollTo(0);
    expect([strip.dataset.moreBefore, strip.dataset.moreAfter]).toEqual([undefined, undefined]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("brings a tab clear of the fade on keyboard focus, but not on a pointer's press", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const matches = Element.prototype.matches;
  let keyboard = false;
  // happy-dom treats :focus-visible as :focus; the browser tells a key from a press.
  vi.spyOn(Element.prototype, "matches").mockImplementation(function (
    this: Element,
    selector: string,
  ) {
    return selector === ":focus-visible" ? keyboard : matches.call(this, selector);
  });
  try {
    await act(async () => root.render(<Fixture summary={{ state: "fresh", pending: 2 }} />));
    const tabs = host.firstElementChild as HTMLDivElement;
    Object.defineProperty(tabs, "clientWidth", { configurable: true, value: 200 });
    tabs.getBoundingClientRect = () => ({ left: 0, right: 200 }) as DOMRect;
    // The last tab sits partly under the right fade.
    const last = Array.from(tabs.querySelectorAll<HTMLButtonElement>("button")).at(-1)!;
    last.getBoundingClientRect = () => ({ left: 170, right: 230 }) as DOMRect;

    // Pressed with a pointer, the tab stays under it until the click completes.
    tabs.scrollLeft = 0;
    await act(async () => last.focus());
    expect(tabs.scrollLeft).toBe(0);
    await act(async () => last.blur());

    keyboard = true;
    await act(async () => last.focus());
    expect(tabs.scrollLeft).toBeGreaterThan(0);
  } finally {
    vi.restoreAllMocks();
    await act(async () => root.unmount());
    host.remove();
  }
});

it("names Waiting for you briefly on a phone, in full for screen readers", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const render = (isSmallScreen: boolean) =>
    act(async () =>
      root.render(
        <ChatFilterTabs
          filter="all"
          onChange={() => {}}
          waiting={pendingFilterIndicator({ state: "fresh", pending: 1 }, false)}
          pinned={null}
          isSmallScreen={isSmallScreen}
        />,
      ),
    );
  try {
    await render(true);
    const waiting = host.querySelector("button")!;
    expect(waiting.querySelector('[aria-hidden="true"]')?.textContent).toBe(
      "filterWaitingForYouShort",
    );
    expect(waiting.querySelector(".sr-only")?.textContent).toBe("filterWaitingForYou");
    await render(false);
    expect(host.querySelector("button")!.textContent).toBe("filterWaitingForYou1");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
