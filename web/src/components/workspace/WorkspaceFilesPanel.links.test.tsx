// @vitest-environment happy-dom
import { act } from "react";
import { expect, it, vi } from "vite-plus/test";
import {
  click,
  fetchWorkspaceFile,
  fetchWorkspaceListing,
  listing,
  mount,
  panel,
  rowByName,
} from "./workspaceFilesTestSupport";

it("explains unavailable links, prevents opening/downloading them and keeps path copying available", async () => {
  fetchWorkspaceListing.mockResolvedValue(
    listing("", [
      {
        name: "broken.json",
        path: "broken.json",
        is_dir: false,
        is_symlink: true,
        unavailable: "missing",
      },
      {
        name: "external",
        path: "external",
        is_dir: false,
        is_symlink: true,
        unavailable: "outside_scope",
      },
    ]),
  );
  const pin = vi.fn();
  const attach = vi.fn();
  const copy = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
  await mount({ onPinPath: pin, onAttachPath: attach });
  const row = rowByName("broken.json");
  expect(row.textContent).toContain("Link target not found");
  expect(rowByName("external").textContent).toContain("Link target is outside this workspace");
  await click(row);
  for (const key of ["Enter", " ", "ArrowRight"]) {
    await act(async () => row.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true })));
  }
  expect(fetchWorkspaceFile).not.toHaveBeenCalled();
  await click(row.querySelector("button")!);
  const actions = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')];
  for (const label of ["Download", "Attach as context", "Pin to a Presentation slot"]) {
    const action = actions.find((item) => item.textContent === label)!;
    expect(action.tagName).toBe("BUTTON");
    expect(action.hasAttribute("disabled")).toBe(true);
    expect(action.hasAttribute("href")).toBe(false);
  }
  await click(actions.find((item) => item.textContent === "Copy relative path")!);
  expect(copy).toHaveBeenCalledWith("broken.json");
  expect(pin).not.toHaveBeenCalled();
  expect(attach).not.toHaveBeenCalled();
  copy.mockRestore();
});

it("restores a repaired link's normal actions after directory refresh", async () => {
  fetchWorkspaceListing
    .mockResolvedValueOnce(
      listing("", [
        {
          name: "link.txt",
          path: "link.txt",
          is_dir: false,
          is_symlink: true,
          unavailable: "missing",
        },
      ]),
    )
    .mockResolvedValue(
      listing("", [{ name: "link.txt", path: "link.txt", is_dir: false, is_symlink: true }]),
    );
  await mount();
  expect(rowByName("link.txt").textContent).toContain("Link target not found");
  await click(panel().querySelector('[title="Refresh directory"]')!);
  expect(rowByName("link.txt").textContent).not.toContain("Link target not found");
  await click(rowByName("link.txt").querySelector("button")!);
  const download = [...document.querySelectorAll<HTMLAnchorElement>('a[role="menuitem"]')].find(
    (item) => item.textContent === "Download",
  )!;
  expect(download.getAttribute("download")).toBe("link.txt");
  expect(download.getAttribute("href")).toContain("path=link.txt");
});
