// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { GroupStatusIndicator } from "./GroupStatusIndicator";
import { getGroupStatus, type GroupStatus } from "../../utils/groupStatus";
import { setUiStylePrefs } from "../../utils/uiStyle";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  setUiStylePrefs({ style: "lcd" });
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  setUiStylePrefs({ style: "classic" });
});

async function render(status: GroupStatus, connectionStatus?: "connecting" | "disconnected") {
  await act(async () =>
    root.render(<GroupStatusIndicator status={status} connectionStatus={connectionStatus} />),
  );
  return host.querySelector<HTMLElement>(".lcd-group-symbol")!;
}

// Without colour each state needs its own shape, drawn by CSS from the key.
it("names the shape after the Group state", async () => {
  for (const status of [
    getGroupStatus(true, "active"),
    getGroupStatus(true, "paused"),
    getGroupStatus(true, "idle"),
    getGroupStatus(false),
    getGroupStatus(false, "stopped"),
  ]) {
    const symbol = await render(status);
    expect(symbol.dataset.lcdShape).toBe(status.key);
    expect(symbol.textContent).toBe("");
  }
});

it("shows the connection state instead of a run state it cannot vouch for", async () => {
  const connecting = await render(getGroupStatus(true, "active"), "connecting");
  expect(connecting.dataset.lcdShape).toBeUndefined();
  expect(connecting.textContent).toBe("…");
  const lost = await render(getGroupStatus(true, "active"), "disconnected");
  expect(lost.textContent).toBe("×");
});
