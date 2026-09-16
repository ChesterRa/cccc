// @vitest-environment happy-dom
import { act } from "react";
import { expect, it } from "vite-plus/test";
import { click, fetchWorkspaceListing, listing, mount, panel } from "./workspaceFilesTestSupport";

it("shows loading until the empty root listing settles", async () => {
  let finish!: (value: unknown) => void;
  fetchWorkspaceListing.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await mount();
  expect(panel().querySelector('[role="status"]')?.textContent).toContain("Loading…");
  expect(panel().textContent).not.toContain("No files to display");
  await act(async () => {
    finish(listing("", []));
  });
  expect(panel().querySelector('[role="status"]')?.textContent).toContain("No files to display");
  expect(panel().textContent).not.toContain("Loading…");
});

it("offers to reveal ignored files and replaces the empty state with results", async () => {
  localStorage.setItem("cccc-workspace-show-ignored", "false");
  fetchWorkspaceListing.mockImplementation(
    async (_group: string, path: string, options: { showIgnored: boolean }) =>
      listing(
        path,
        options.showIgnored
          ? [{ name: "notes.txt", path: "notes.txt", is_dir: false, ignored: true }]
          : [],
      ),
  );
  await mount();
  const status = panel().querySelector('[role="status"]')!;
  expect(status.textContent).toContain("Files may be hidden by Git ignore rules");
  await click(status.querySelector("button")!);
  expect(fetchWorkspaceListing.mock.calls.at(-1)?.[2]).toEqual({
    showIgnored: true,
    scopeKey: "scope-a",
    scopeUrl: "/repo",
  });
  expect(panel().querySelector('[role="status"]')).toBeNull();
  expect(panel().querySelector('[role="treeitem"]')?.textContent).toContain("notes.txt");
});

it("suggests adding files and refreshing when the unfiltered directory is empty", async () => {
  fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
    listing(path, []),
  );
  await mount();
  expect(panel().querySelector('[role="status"]')?.textContent).toContain(
    "Add files to this workspace, then refresh.",
  );
  expect(panel().querySelector('[role="status"] button')).toBeNull();
});

it("keeps loading errors distinct from an empty directory", async () => {
  fetchWorkspaceListing.mockResolvedValue({ ok: false, error: { message: "Permission denied" } });
  await mount();
  expect(panel().textContent).toContain("Permission denied");
  expect(panel().textContent).not.toContain("No files to display");
});

it("keeps the missing-workspace hint without showing a loading or empty state", async () => {
  await mount({ scopeKey: "", scopeUrl: "" });
  expect(panel().textContent).toContain("Attach a workspace");
  expect(panel().querySelector('[role="status"]')).toBeNull();
  expect(fetchWorkspaceListing).not.toHaveBeenCalled();
});
