// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useFrontWorkspaceLoad, type WorkspaceEditorActivity } from "./useFrontWorkspaceLoad";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const idle: WorkspaceEditorActivity = {
  path: "a.md",
  selectedPath: "a.md",
  loading: false,
  pathLoading: false,
  error: "",
};

let root: Root | null = null;
const load = vi.fn();

function Probe({ front, editor }: { front: string; editor: WorkspaceEditorActivity }) {
  useFrontWorkspaceLoad(front, editor, load);
  return null;
}

function show(front: string, editor: Partial<WorkspaceEditorActivity>) {
  act(() => root!.render(createElement(Probe, { front, editor: { ...idle, ...editor } })));
}

describe("useFrontWorkspaceLoad", () => {
  beforeEach(() => {
    load.mockReset();
    root = createRoot(document.createElement("div"));
  });
  afterEach(() => {
    act(() => root?.unmount());
    root = null;
  });

  it("loads the front tab's file when the editor holds another one", () => {
    show("b.md", {});
    expect(load).toHaveBeenCalledTimes(1);
    expect(load).toHaveBeenCalledWith("b.md");
  });

  it("does nothing when the editor already holds the front tab's file", () => {
    show("a.md", {});
    show("", {});
    expect(load).not.toHaveBeenCalled();
  });

  it("waits for a read of another file instead of cancelling it", () => {
    show("b.md", { selectedPath: "c.md", loading: true });
    expect(load).not.toHaveBeenCalled();
    // The read finished, but its tab was not selected because the user picked b.md meanwhile.
    show("b.md", { path: "c.md", selectedPath: "c.md" });
    expect(load).toHaveBeenCalledTimes(1);
    expect(load).toHaveBeenCalledWith("b.md");
  });

  it("waits for a path from a message or the path box to be looked up", () => {
    show("b.md", { pathLoading: true });
    expect(load).not.toHaveBeenCalled();
  });

  it("does not read again a front file that just failed", () => {
    show("b.md", { selectedPath: "b.md", error: "not found" });
    expect(load).not.toHaveBeenCalled();
  });
});
