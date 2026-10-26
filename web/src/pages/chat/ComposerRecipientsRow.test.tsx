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
