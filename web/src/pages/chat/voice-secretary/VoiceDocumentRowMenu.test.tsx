// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { TFunction } from "i18next";
import { VoiceDocumentRowMenu } from "./VoiceDocumentRowMenu";

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const t = ((_key: string, options: { defaultValue: string }) => options.defaultValue) as TFunction;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

it("routes right-click actions to the clicked document and closes the menu", async () => {
  const actions = [vi.fn(), vi.fn(), vi.fn()];
  await act(async () =>
    root.render(
      <VoiceDocumentRowMenu
        title="文档 B"
        disabled={false}
        t={t}
        onSelect={actions[0]}
        onArchive={actions[1]}
        onDelete={actions[2]}
      >
        <button>文档 B</button>
      </VoiceDocumentRowMenu>,
    ),
  );
  for (const [index, label] of ["Select", "Archive", "Delete"].entries()) {
    await act(async () => {
      host
        .querySelector("button")!
        .dispatchEvent(
          new MouseEvent("contextmenu", {
            bubbles: true,
            cancelable: true,
            clientX: 40,
            clientY: 50,
          }),
        );
    });
    const item = [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find(
      (item) => item.textContent === label,
    )!;
    expect(item).toBeTruthy();
    await act(async () => item.click());
    expect(actions[index]).toHaveBeenCalledTimes(1);
    expect(document.querySelector('[role="menu"]')).toBeNull();
  }
});

it("supports keyboard opening and disables mutations while busy", async () => {
  const onDelete = vi.fn();
  await act(async () =>
    root.render(
      <VoiceDocumentRowMenu
        title="busy"
        disabled
        t={t}
        onSelect={vi.fn()}
        onArchive={vi.fn()}
        onDelete={onDelete}
      >
        <button>busy</button>
      </VoiceDocumentRowMenu>,
    ),
  );
  await act(async () => {
    host
      .querySelector("button")!
      .dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "F10",
          shiftKey: true,
          bubbles: true,
          cancelable: true,
        }),
      );
  });
  const items = [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
  expect(items).toHaveLength(3);
  expect(items.every((item) => item.disabled)).toBe(true);
  await act(async () => items[2].click());
  expect(onDelete).not.toHaveBeenCalled();
});
