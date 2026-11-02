// @vitest-environment happy-dom
import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { ComposerRecipientsRow } from "./ComposerRecipientsRow";
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

it("offers Chet only to an administrator, while allowing a stale selection to be cleared", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const toggle = vi.fn();
  const render = async (allowChet: boolean, toTokens: string[]) =>
    act(async () =>
      root.render(
        <ComposerRecipientsRow
          isDark={false}
          isSmallScreen={false}
          selectedGroupId="g"
          busy=""
          actors={[]}
          allowChet={allowChet}
          toTokens={toTokens}
          onToggleRecipient={toggle}
          onClearRecipients={vi.fn()}
        />,
      ),
    );
  const chet = () =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.textContent === "@chet",
    );
  try {
    await render(false, []);
    expect(chet()).toBeUndefined();
    await render(true, []);
    expect(chet()?.getAttribute("aria-pressed")).toBe("false");
    await act(async () => chet()?.click());
    expect(toggle).toHaveBeenCalledWith("@chet");
    await render(false, ["@chet"]);
    expect(chet()?.getAttribute("aria-pressed")).toBe("true");
    await act(async () => chet()?.click());
    expect(toggle).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("labels recipients accessibly without a visible caption and retains selection", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host),
    toggle = vi.fn(),
    clear = vi.fn();
  try {
    await act(async () =>
      root.render(
        <ComposerRecipientsRow
          isDark={false}
          isSmallScreen={false}
          selectedGroupId="g"
          busy=""
          actors={[]}
          toTokens={["@foreman"]}
          onToggleRecipient={toggle}
          onClearRecipients={clear}
        />,
      ),
    );
    const group = host.querySelector('[role="group"][aria-label="to"]')!;
    expect(group.textContent).not.toContain("to");
    const selected = host.querySelector<HTMLButtonElement>('button[aria-pressed="true"]')!;
    expect(selected.textContent).toBe("@foreman");
    await act(async () => selected.click());
    expect(toggle).toHaveBeenCalledWith("@foreman");
    await act(async () =>
      host.querySelector<HTMLButtonElement>('button[aria-label="clearRecipients"]')!.click(),
    );
    expect(clear).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it.each([true, false])(
  "dismisses recipient details without relying on a blur (%s)",
  async (isSmallScreen) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    const host = document.createElement("div");
    const outside = document.createElement("button");
    document.body.append(host, outside);
    const root = createRoot(host);
    try {
      await act(async () =>
        root.render(
          <ComposerRecipientsRow
            isDark={false}
            isSmallScreen={isSmallScreen}
            selectedGroupId="g"
            busy=""
            actors={[{ id: "agent", title: "Agent", runtime: "codex" }]}
            toTokens={["agent"]}
            onToggleRecipient={vi.fn()}
            onClearRecipients={vi.fn()}
          />,
        ),
      );
      const trigger = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
        (button) => button.textContent === "Agent",
      )!;
      const details = () =>
        document.querySelector('[role="dialog"][aria-label="recipientDetails"]');
      await act(async () => trigger.focus());
      const copy = details()!.querySelector<HTMLButtonElement>("button")!;
      await act(async () => copy.dispatchEvent(new Event("pointerdown", { bubbles: true })));
      expect(details()).not.toBeNull();
      await act(async () => copy.focus());
      await act(async () =>
        document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })),
      );
      expect(details()).toBeNull();
      expect(document.activeElement).toBe(trigger);
      await act(async () => {
        outside.focus();
        trigger.focus();
      });
      expect(details()).not.toBeNull();
      // A touch outside need not move focus away from the recipient button.
      await act(async () => outside.dispatchEvent(new Event("pointerdown", { bubbles: true })));
      expect(details()).toBeNull();
      expect(document.activeElement).toBe(trigger);
    } finally {
      await act(async () => root.unmount());
      host.remove();
      outside.remove();
    }
  },
);

it("keeps a newly selected recipient visible when the clear button reduces the row", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  function Harness() {
    const [tokens, setTokens] = useState<string[]>([]);
    return (
      <ComposerRecipientsRow
        isDark={false}
        isSmallScreen={false}
        selectedGroupId="g"
        busy=""
        actors={[{ id: "agent", title: "Agent", runtime: "codex" }]}
        toTokens={tokens}
        onToggleRecipient={(id) =>
          setTokens((previous) =>
            previous.includes(id) ? previous.filter((item) => item !== id) : [...previous, id],
          )
        }
        onClearRecipients={() => setTokens([])}
      />
    );
  }
  try {
    await act(async () => root.render(<Harness />));
    const scrollPort = host.querySelector<HTMLElement>(".overflow-x-auto")!;
    const selected = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
      (button) => button.textContent === "Agent",
    )!;
    const inside = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
      (button) => button.textContent === "@foreman",
    )!;
    Object.defineProperty(scrollPort, "scrollWidth", { configurable: true, value: 240 });
    Object.defineProperty(scrollPort, "clientWidth", {
      configurable: true,
      get: () => (host.querySelector('[aria-label="clearRecipients"]') ? 100 : 136),
    });
    vi.spyOn(scrollPort, "getBoundingClientRect").mockImplementation(
      () => new DOMRect(0, 0, scrollPort.clientWidth, 32),
    );
    vi.spyOn(selected, "getBoundingClientRect").mockImplementation(
      () => new DOMRect(96 - scrollPort.scrollLeft, 0, 40, 32),
    );
    vi.spyOn(inside, "getBoundingClientRect").mockReturnValue(new DOMRect(10, 0, 40, 32));
    await act(async () => selected.click());
    expect(selected.getAttribute("aria-pressed")).toBe("true");
    expect(scrollPort.scrollLeft).toBeGreaterThan(0);
    expect(selected.getBoundingClientRect().right).toBeLessThanOrEqual(
      scrollPort.getBoundingClientRect().right,
    );
    const afterReveal = scrollPort.scrollLeft;
    await act(async () => inside.click());
    expect(inside.getAttribute("aria-pressed")).toBe("true");
    expect(scrollPort.scrollLeft).toBe(afterReveal);

    // A name wider than the port cannot fit completely. Do not scroll its
    // already-visible beginning away just to reveal its end.
    const wide = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
      (button) => button.textContent === "@all",
    )!;
    vi.spyOn(wide, "getBoundingClientRect").mockReturnValue(new DOMRect(10, 0, 150, 32));
    await act(async () => wide.click());
    expect(wide.getAttribute("aria-pressed")).toBe("true");
    expect(scrollPort.scrollLeft).toBe(afterReveal);

    // When a name nearly fills the port, an oversized fade margin would hide
    // its beginning after revealing the end.
    const nearWidth = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
      (button) => button.textContent === "@peers",
    )!;
    const beforeNearWidth = scrollPort.scrollLeft;
    vi.spyOn(nearWidth, "getBoundingClientRect").mockImplementation(
      () => new DOMRect(40 - (scrollPort.scrollLeft - beforeNearWidth), 0, 90, 32),
    );
    await act(async () => nearWidth.click());
    expect(nearWidth.getBoundingClientRect().left).toBeGreaterThanOrEqual(
      scrollPort.getBoundingClientRect().left,
    );
    expect(nearWidth.getBoundingClientRect().right).toBeLessThanOrEqual(
      scrollPort.getBoundingClientRect().right,
    );
  } finally {
    await act(async () => root.unmount());
    host.remove();
    vi.restoreAllMocks();
  }
});
