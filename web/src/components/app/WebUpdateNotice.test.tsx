// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { WebUpdateNotice } from "./WebUpdateNotice";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

it("only reloads after an explicit confirmation and lets the user defer", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const confirm = vi.fn().mockReturnValue(false);
  vi.stubGlobal("confirm", confirm);
  const reload = vi.fn();
  const dismiss = vi.fn();
  const host = document.createElement("div");
  const previous = document.createElement("button");
  Object.defineProperty(previous, "getClientRects", { value: () => [new DOMRect(0, 0, 10, 10)] });
  document.body.append(previous, host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<WebUpdateNotice onDismiss={dismiss} onReload={reload} />));
    const buttons = host.querySelectorAll("button");
    expect(host.querySelector('[role="status"]')?.textContent).toBe("webUpdateAvailable");
    expect(reload).not.toHaveBeenCalled();
    await act(async () => buttons[0].click());
    expect(confirm).toHaveBeenCalledWith("webUpdateReloadConfirm");
    expect(reload).not.toHaveBeenCalled();
    confirm.mockReturnValue(true);
    await act(async () => buttons[0].click());
    expect(reload).toHaveBeenCalledOnce();
    previous.focus();
    buttons[1].focus();
    await act(async () => {
      buttons[1].click();
      root.render(null);
    });
    expect(dismiss).toHaveBeenCalledOnce();
    expect(document.activeElement).toBe(previous);
  } finally {
    await act(async () => root.unmount());
    previous.remove();
    host.remove();
    vi.unstubAllGlobals();
  }
});
