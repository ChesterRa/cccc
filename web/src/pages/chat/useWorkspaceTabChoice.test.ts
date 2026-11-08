// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vite-plus/test";
import { resourceKey, useResourcePanelStore } from "../../stores/useResourcePanelStore";
import { useWorkspaceTabChoice } from "./useWorkspaceTabChoice";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const file = (path: string) =>
  ({ kind: "workspace", scopeKey: "s1", scopeUrl: "/repo", path }) as const;
const key = (path: string) => resourceKey(file(path));
const panel = () => useResourcePanelStore.getState().groups.g1;

type Choice = ReturnType<typeof useWorkspaceTabChoice>;
let root: Root | null = null;
let container: HTMLDivElement | null = null;
let current: Choice | null = null;

function Probe({ editorPath }: { editorPath: string }) {
  current = useWorkspaceTabChoice("g1", "s1", "/repo", editorPath);
  return null;
}

/** Shows `editorPath` as the file the editor holds, as ChatTab passes it. */
function editorHolds(editorPath: string) {
  act(() => root!.render(createElement(Probe, { editorPath })));
  return current!;
}

describe("useWorkspaceTabChoice", () => {
  beforeEach(() => {
    useResourcePanelStore.setState({ groups: {}, dock: "right", generation: 0 });
    container = document.createElement("div");
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => root?.unmount());
    root = null;
    container = null;
    current = null;
  });

  it("selects the file the user asked for once it arrives", () => {
    const tabs = editorHolds("a.md");
    const choice = tabs.choose();
    editorHolds("b.md");
    act(() => tabs.settle("b.md", choice));
    expect(panel().activeKey).toBe(key("b.md"));
    expect(panel().tabs.map((tab) => tab.key)).toEqual([key("a.md"), key("b.md")]);
  });

  it("gives a file the editor holds without a request a tab, not the selection", () => {
    const tabs = editorHolds("a.md");
    act(() => tabs.settle("a.md", tabs.choose()));
    editorHolds("b.md");
    expect(panel().activeKey).toBe(key("a.md"));
    expect(panel().tabs.map((tab) => tab.key)).toEqual([key("a.md"), key("b.md")]);
  });

  it("does not let a file that arrives on its own use another request's choice", () => {
    // The user has a.md open and asks for c.md; while c.md is read, the tab in front loads
    // b.md again. b.md arriving must not take the choice meant for c.md, or the tabs would keep
    // taking the selection from each other.
    const tabs = editorHolds("a.md");
    act(() => tabs.settle("a.md", tabs.choose()));
    const choice = tabs.choose();
    editorHolds("b.md");
    expect(panel().activeKey).toBe(key("a.md"));
    editorHolds("c.md");
    act(() => tabs.settle("c.md", choice));
    expect(panel().activeKey).toBe(key("c.md"));
    editorHolds("b.md");
    expect(panel().activeKey).toBe(key("c.md"));
  });

  it("lets only the latest of several quick requests select its file", () => {
    const tabs = editorHolds("");
    const first = tabs.choose();
    const second = tabs.choose();
    act(() => tabs.settle("second.md", second));
    act(() => tabs.settle("first.md", first));
    expect(panel().activeKey).toBe(key("second.md"));
    expect(panel().tabs.map((tab) => tab.key)).toEqual([key("second.md"), key("first.md")]);
  });

  it("gives the front to the path a tab in front now leads to", () => {
    // Loading the tab in front found its file at another path, as after it became a symlink.
    const tabs = editorHolds("a.md");
    act(() => tabs.settle("a.md", tabs.choose()));
    // A request that failed leaves its choice behind; that must not keep the front where it was.
    tabs.choose();
    const since = useResourcePanelStore.getState().generation;
    editorHolds("b.md");
    act(() => tabs.follow("b.md", since));
    expect(panel().activeKey).toBe(key("b.md"));
  });

  it("does not follow such a path once something else was picked meanwhile", () => {
    const tabs = editorHolds("a.md");
    act(() => tabs.settle("a.md", tabs.choose()));
    const since = useResourcePanelStore.getState().generation;
    act(() => {
      useResourcePanelStore.getState().open("g1", file("picked.md"), "picked.md");
    });
    act(() => tabs.follow("b.md", since));
    expect(panel().activeKey).toBe(key("picked.md"));
    expect(panel().tabs.map((tab) => tab.key)).toContain(key("b.md"));
  });

  it("keeps what the user picked in the panel while an earlier request was still loading", () => {
    const tabs = editorHolds("");
    const choice = tabs.choose();
    act(() => {
      useResourcePanelStore.getState().open("g1", file("picked.md"), "picked.md");
    });
    act(() => tabs.settle("slow.md", choice));
    expect(panel().activeKey).toBe(key("picked.md"));
    expect(panel().tabs.map((tab) => tab.key)).toEqual([key("picked.md"), key("slow.md")]);
  });
});
