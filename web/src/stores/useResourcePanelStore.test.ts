import { beforeEach, describe, expect, it } from "vitest";
import { resourceKey, useResourcePanelStore } from "./useResourcePanelStore";

const image = (messageKey: string, index = 0) =>
  ({
    kind: "attachment",
    messageKey,
    eventId: "",
    index,
    blobGroupId: "g1",
    blobPath: `state/blobs/${messageKey}-${index}`,
    media: "image",
    isSvg: false,
  }) as const;
const file = (path: string, scopeUrl = "/repo") =>
  ({ kind: "workspace", scopeKey: "s1", scopeUrl, path }) as const;
const state = () => useResourcePanelStore.getState();

describe("resource panel store", () => {
  beforeEach(() => {
    useResourcePanelStore.setState({ groups: {}, dock: "right", generation: 0 });
  });

  it("selects an open resource instead of adding a second tab", () => {
    state().open("g1", file("docs/a.md"), "a.md");
    state().open("g1", image("m1"), "shot.png");
    state().open("g1", file("docs/a.md"), "a.md");
    const panel = state().groups.g1;
    expect(panel.tabs.map((tab) => tab.key)).toEqual([
      resourceKey(file("docs/a.md")),
      resourceKey(image("m1")),
    ]);
    expect(panel.activeKey).toBe(resourceKey(file("docs/a.md")));
    expect(panel.open).toBe(true);
  });

  it("keeps the same image sent in two messages, and one path under two roots, apart", () => {
    state().open("g1", image("m1"), "shot.png");
    state().open("g1", image("m2"), "shot.png");
    state().open("g1", file("a.md", "/repo"), "a.md");
    state().open("g1", file("a.md", "/other"), "a.md");
    expect(state().groups.g1.tabs).toHaveLength(4);
  });

  it("selects the right neighbour when the selected tab closes, like a browser", () => {
    state().open("g1", image("m1"), "1");
    state().open("g1", image("m2"), "2");
    state().open("g1", image("m3"), "3");
    state().activate("g1", resourceKey(image("m2")));
    state().closeTabs("g1", [resourceKey(image("m2"))]);
    expect(state().groups.g1.activeKey).toBe(resourceKey(image("m3")));
    state().closeTabs("g1", [resourceKey(image("m3"))]);
    expect(state().groups.g1.activeKey).toBe(resourceKey(image("m1")));
  });

  it("hides the panel without closing tabs, and disappears with the last tab", () => {
    state().open("g1", image("m1"), "1");
    state().setOpen("g1", false);
    expect(state().groups.g1).toMatchObject({ open: false });
    expect(state().groups.g1.tabs).toHaveLength(1);
    state().setOpen("g1", true);
    state().closeTabs("g1", [resourceKey(image("m1"))]);
    expect(state().groups.g1).toMatchObject({ open: false, tabs: [] });
    state().setOpen("g1", true);
    expect(state().groups.g1.open).toBe(false);
  });

  it("restores a close-all as one entry, newest first", () => {
    state().open("g1", image("m1"), "1");
    state().open("g1", image("m2"), "2");
    state().open("g1", image("m3"), "3");
    state().closeTabs("g1", [resourceKey(image("m3"))]);
    state().closeTabs(
      "g1",
      state().groups.g1.tabs.map((tab) => tab.key),
    );
    const [all, single] = state().groups.g1.closed;
    expect(all.tabs.map((tab) => tab.title)).toEqual(["1", "2"]);
    expect(single.tabs.map((tab) => tab.title)).toEqual(["3"]);
    state().restore("g1", all.id);
    expect(state().groups.g1).toMatchObject({ open: true });
    expect(state().groups.g1.tabs.map((tab) => tab.title)).toEqual(["1", "2"]);
    expect(state().groups.g1.closed.map((entry) => entry.id)).toEqual([single.id]);
  });

  it("lets a late open add its tab without taking the selection or showing the panel", () => {
    state().open("g1", image("m1"), "1");
    state().open("g1", image("m2"), "2");
    state().setOpen("g1", false);
    const generation = state().generation;
    state().ensure("g1", file("slow.md"), "slow.md");
    expect(state().groups.g1.tabs.map((tab) => tab.title)).toEqual(["1", "2", "slow.md"]);
    expect(state().groups.g1).toMatchObject({ activeKey: resourceKey(image("m2")), open: false });
    expect(state().generation).toBe(generation);
  });

  it("updates a confirmed attachment in place without selecting or showing it", () => {
    state().open("g1", image("client:c1"), "1");
    state().open("g1", image("m2"), "2");
    state().setOpen("g1", false);
    state().updateAttachment("g1", "client:c1", 0, {
      eventId: "ev1",
      blobPath: "state/blobs/real",
      localPreviewUrl: undefined,
    });
    const panel = state().groups.g1;
    expect(panel.tabs[0]).toMatchObject({ eventId: "ev1", blobPath: "state/blobs/real" });
    expect(panel.activeKey).toBe(resourceKey(image("m2")));
    expect(panel.open).toBe(false);
  });

  it("keeps each Group's tabs apart", () => {
    state().open("g1", image("m1"), "1");
    state().open("g2", file("README.md"), "README.md");
    const { groups } = state();
    expect(groups.g1.tabs.map((tab) => tab.title)).toEqual(["1"]);
    expect(groups.g2.tabs.map((tab) => tab.title)).toEqual(["README.md"]);
  });
  it("follows a renamed file or folder, keeping the selection and what was closed", () => {
    state().open("g1", file("docs/a.md"), "a.md");
    state().open("g1", file("docs/sub/b.md"), "b.md");
    state().open("g1", file("docs/a.md", "/other"), "a.md");
    state().open("g1", file("notes.md"), "notes.md");
    state().closeTabs("g1", [resourceKey(file("notes.md"))]);
    state().activate("g1", resourceKey(file("docs/sub/b.md")));
    state().moveWorkspacePath("g1", "s1", "/repo", "docs", "guide");
    state().moveWorkspacePath("g1", "s1", "/repo", "notes.md", "notes/todo.md");
    const panel = state().groups.g1;
    expect(panel.tabs.map((tab) => [tab.key, tab.title])).toEqual([
      [resourceKey(file("guide/a.md")), "a.md"],
      [resourceKey(file("guide/sub/b.md")), "b.md"],
      // The same path under another root is another file.
      [resourceKey(file("docs/a.md", "/other")), "a.md"],
    ]);
    expect(panel.activeKey).toBe(resourceKey(file("guide/sub/b.md")));
    expect(panel.closed[0].tabs.map((tab) => [tab.key, tab.title])).toEqual([
      [resourceKey(file("notes/todo.md")), "todo.md"],
    ]);
  });

  it("keeps one tab when a file moves onto a tab left open for a deleted file", () => {
    state().open("g1", file("docs/old.md"), "old.md");
    state().open("g1", file("docs/new.md"), "new.md");
    state().activate("g1", resourceKey(file("docs/new.md")));
    state().moveWorkspacePath("g1", "s1", "/repo", "docs/new.md", "docs/old.md");
    const panel = state().groups.g1;
    expect(panel.tabs.map((tab) => tab.key)).toEqual([resourceKey(file("docs/old.md"))]);
    expect(panel.activeKey).toBe(resourceKey(file("docs/old.md")));
  });

  it("renames a tab without selecting it or showing the panel", () => {
    state().open("g1", file("a.md"), "a.md");
    state().setOpen("g1", false);
    const slot = { kind: "presentation", slotId: "slot-1", slotIndex: 1, refKey: "live" } as const;
    state().ensure("g1", slot, "Old card");
    const before = state().generation;
    state().retitle("g1", resourceKey(slot), "New card");
    const panel = state().groups.g1;
    expect(panel.tabs.find((tab) => tab.key === resourceKey(slot))?.title).toBe("New card");
    expect(panel.activeKey).toBe(resourceKey(file("a.md")));
    expect(panel.open).toBe(false);
    expect(state().generation).toBe(before);
  });
});
