// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { ComposerFileTray } from "./ComposerFileTray";

describe("ComposerFileTray", () => {
  const host = document.createElement("div");
  let root: Root;
  const file = (name: string) => new File(["fixture"], name, { type: "text/plain" });
  const render = (files: File[], onRemove = vi.fn()) =>
    act(async () =>
      root.render(
        <ComposerFileTray
          groupId="g1"
          files={files}
          onRemove={onRemove}
          removeLabel={(item) => `Remove ${item.name}`}
        />,
      ),
    );
  const names = () =>
    [...host.querySelectorAll("[data-composer-file-preview]")].map((item) =>
      item.querySelector("[title]")?.getAttribute("title"),
    );

  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("takes no room at all while nothing is attached", async () => {
    await render([]);

    expect(host.innerHTML).toBe("");
  });

  it("shows each waiting file once, in the order it was attached", async () => {
    await render([file("notes.md"), file("plan.txt"), file("notes.md")]);

    expect(names()).toEqual(["notes.md", "plan.txt", "notes.md"]);
  });

  it("removes the file whose own button was pressed, even among files of the same name", async () => {
    const onRemove = vi.fn();
    await render([file("notes.md"), file("plan.txt"), file("notes.md")], onRemove);
    const buttons = [...host.querySelectorAll<HTMLButtonElement>('button[aria-label^="Remove "]')];
    expect(buttons.map((button) => button.getAttribute("aria-label"))).toEqual([
      "Remove notes.md",
      "Remove plan.txt",
      "Remove notes.md",
    ]);

    await act(async () => buttons[2].click());
    await act(async () => buttons[1].click());

    expect(onRemove.mock.calls).toEqual([[2], [1]]);
  });
});
