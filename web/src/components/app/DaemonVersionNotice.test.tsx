// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { DaemonVersionNotice } from "./DaemonVersionNotice";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

it("offers details and restores focus only when the notice owned it", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  const previous = document.createElement("button");
  document.body.append(previous, host);
  Object.defineProperty(previous, "getClientRects", { value: () => [new DOMRect(0, 0, 10, 10)] });
  const root = createRoot(host);
  const difference = { webVersion: "0.4.45", daemonVersion: "0.4.44" };
  const render = () => <DaemonVersionNotice difference={difference} onDismiss={() => {}} />;
  try {
    await act(async () => root.render(render()));
    expect(host.querySelector('[role="status"]')?.textContent).toBe("daemonVersionMismatch");
    expect(host.querySelector("details summary")?.textContent).toBe("daemonVersionMismatchHelp");
    previous.focus();
    host.querySelector("button")?.focus();
    await act(async () => root.render(null));
    expect(document.activeElement).toBe(previous);

    await act(async () => root.render(render()));
    previous.focus();
    await act(async () => root.render(null));
    expect(document.activeElement).toBe(previous);
  } finally {
    await act(async () => root.unmount());
    previous.remove();
    host.remove();
  }
});
