// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { CreateGroupDirectoryBrowser } from "./CreateGroupDirectoryBrowser";

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

describe("CreateGroupDirectoryBrowser", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const onCreateDirectory = vi.fn(async () => true);

  beforeEach(async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    await act(async () => {
      root.render(
        <CreateGroupDirectoryBrowser
          dirItems={[]}
          currentDir="/projects"
          parentDir="/"
          driveLocations={[]}
          creatingDirectory={false}
          onSelect={vi.fn()}
          onFetch={vi.fn()}
          onCreateDirectory={onCreateDirectory}
        />,
      );
    });
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.clearAllMocks();
  });

  it("creates a folder inline and closes the form after success", async () => {
    const newFolderButton = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "createGroup.newFolder",
    );
    await act(async () => newFolderButton?.click());

    const input = container.querySelector<HTMLInputElement>(
      'input[aria-label="createGroup.folderName"]',
    );
    expect(input).toBeTruthy();
    await act(async () => {
      if (!input) return;
      const valueSetter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
      valueSetter?.call(input, "demo");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const createButton = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "createGroup.createFolder",
    );
    await act(async () => createButton?.click());

    expect(onCreateDirectory).toHaveBeenCalledWith("/projects", "demo");
    expect(container.querySelector('input[aria-label="createGroup.folderName"]')).toBeNull();
  });

  it("hides stale directory actions after a listing error", async () => {
    await act(async () => {
      root.render(
        <CreateGroupDirectoryBrowser
          dirItems={[{ name: "stale", path: "/projects/stale", is_dir: true }]}
          currentDir="/projects"
          parentDir="/"
          driveLocations={[]}
          error="directory unavailable"
          creatingDirectory={false}
          onSelect={vi.fn()}
          onFetch={vi.fn()}
          onCreateDirectory={onCreateDirectory}
        />,
      );
    });

    expect(container.querySelector('[role="alert"]')?.textContent).toBe("directory unavailable");
    expect(container.textContent).not.toContain("createGroup.newFolder");
    expect(container.textContent).not.toContain("stale");
  });

  it("discards an unfinished folder name when the parent changes", async () => {
    const newFolderButton = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "createGroup.newFolder",
    );
    await act(async () => newFolderButton?.click());

    const input = container.querySelector<HTMLInputElement>(
      'input[aria-label="createGroup.folderName"]',
    );
    await act(async () => {
      if (!input) return;
      const valueSetter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
      valueSetter?.call(input, "wrong-parent");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });

    await act(async () => {
      root.render(
        <CreateGroupDirectoryBrowser
          dirItems={[]}
          currentDir="/other-projects"
          parentDir="/"
          driveLocations={[]}
          creatingDirectory={false}
          onSelect={vi.fn()}
          onFetch={vi.fn()}
          onCreateDirectory={onCreateDirectory}
        />,
      );
    });

    expect(container.querySelector('input[aria-label="createGroup.folderName"]')).toBeNull();
    expect(onCreateDirectory).not.toHaveBeenCalled();
  });
});
