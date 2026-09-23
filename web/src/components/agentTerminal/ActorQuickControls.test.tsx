// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

import { ActorQuickControls } from "./ActorQuickControls";

async function render(running: boolean, onRemove: () => void) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <ActorQuickControls
        actorTitle="Claude"
        running={running}
        busy={false}
        readOnly={false}
        hasTerminal
        writable
        connected
        connectionFailed={false}
        canStartNewSession={false}
        unreadCount={0}
        onInterrupt={vi.fn()}
        onLaunch={vi.fn()}
        onReconnect={vi.fn()}
        onTakeover={vi.fn()}
        onHistory={vi.fn()}
        onNewSession={vi.fn()}
        onRestart={vi.fn()}
        onStop={vi.fn()}
        onEdit={vi.fn()}
        onInbox={vi.fn()}
        onRemove={onRemove}
      />,
    );
  });
  const trigger = host.querySelector<HTMLButtonElement>('button[aria-label="actorControls"]');
  if (!trigger) throw new Error("menu trigger missing");
  await act(async () => {
    trigger.dispatchEvent(new MouseEvent("pointerdown", { bubbles: true }));
    trigger.click();
  });
  const removeButton = Array.from(document.querySelectorAll("button")).find(
    (button) => button.textContent === "removeAgent",
  );
  if (!removeButton) throw new Error("remove entry missing");
  return { root, host, removeButton };
}

describe("ActorQuickControls", () => {
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    document.body.innerHTML = "";
  });

  it("allows removing a running Actor and invokes the removal callback", async () => {
    const onRemove = vi.fn();
    const { root, host, removeButton } = await render(true, onRemove);
    expect(removeButton.disabled).toBe(false);
    await act(async () => {
      removeButton.click();
    });
    expect(onRemove).toHaveBeenCalledTimes(1);
    await act(async () => root.unmount());
    host.remove();
  });
});
