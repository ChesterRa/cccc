// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { ConnectSidebar } from "./ConnectSidebar";
import type { ConnectWorkbench } from "./useConnectWorkbench";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

it("opens a remote Group's connections through its own instance navigation", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host),
    select = vi.fn(),
    onSelected = vi.fn();
  const instance = {
    instance_id: "remote",
    device_id: "remote-device",
    public_origin: "https://remote.test",
    display_name: "Remote",
  };
  const workbench = {
    instances: [instance],
    collapsedInstances: [],
    selected: null,
    select,
    listings: {
      remote: {
        deviceId: instance.device_id,
        origin: instance.public_origin,
        checkedAt: Date.now(),
        groups: [{ group_id: "other-group", title: "Remote Group", running: true }],
      },
    },
  } as unknown as ConnectWorkbench;
  try {
    await act(async () =>
      root.render(
        <ConnectSidebar workbench={workbench} collapsed={false} onSelected={onSelected} />,
      ),
    );
    const trigger = host.querySelector<HTMLButtonElement>('button[aria-haspopup="menu"]')!;
    await act(async () => trigger.click());
    expect(select).not.toHaveBeenCalled();
    const action = document.querySelector<HTMLButtonElement>('[role="menuitem"]')!;
    expect(action.textContent).toBe("groupConnections.title");
    await act(async () => action.click());
    expect(select).toHaveBeenCalledExactlyOnceWith("remote", "other-group", "connections");
    expect(onSelected).toHaveBeenCalledOnce();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
