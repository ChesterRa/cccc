// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { ComposerRecipientsRow } from "./ComposerRecipientsRow";
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
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
