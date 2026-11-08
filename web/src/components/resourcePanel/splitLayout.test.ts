import { describe, expect, it } from "vitest";
import type { ResourceTab } from "../../stores/useResourcePanelStore";
import {
  followTabs,
  navigationSide,
  resumeSplit,
  splitBoxes,
  splitPartner,
  startSplit,
} from "./splitLayout";

const tab = (key: string): ResourceTab => ({
  kind: "workspace",
  scopeKey: "s",
  scopeUrl: "/p",
  path: key,
  key,
  title: key,
});
const tabs = ["a", "b", "c", "d"].map(tab);

describe("starting the side-by-side view", () => {
  it("pairs the selected tab with the one read before it", () => {
    expect(splitPartner(tabs, "c", "a")).toBe("a");
  });

  it("falls back to the next tab, or the previous one for the last tab", () => {
    expect(splitPartner(tabs, "b", "")).toBe("c");
    expect(splitPartner(tabs, "d", "d")).toBe("c");
    // A previous tab that has since closed is not offered.
    expect(splitPartner(tabs, "a", "gone")).toBe("b");
  });

  it("shows a lone tab on both sides", () => {
    expect(splitPartner([tab("a")], "a", "gone")).toBe("a");
    expect(startSplit([tab("a")], "a", "a")).toEqual({
      left: "a",
      right: "a",
      focus: "left",
      ratio: 0.5,
    });
  });

  it("keeps the tab strip's order and focuses the selected tab", () => {
    expect(startSplit(tabs, "c", "a")).toEqual({
      left: "a",
      right: "c",
      focus: "right",
      ratio: 0.5,
    });
    expect(startSplit(tabs, "a", "c")).toEqual({
      left: "a",
      right: "c",
      focus: "left",
      ratio: 0.5,
    });
  });
});

describe("keeping the split in step with the tabs", () => {
  const split = { left: "a", right: "c", focus: "right" as const, ratio: 0.4 };

  it("moves the focus to the side that holds the selected tab", () => {
    expect(followTabs(split, tabs, "a")).toEqual({ ...split, focus: "left" });
    expect(followTabs(split, tabs, "c")).toBe(split);
  });

  it("opens a tab selected elsewhere on the focused side", () => {
    expect(followTabs(split, tabs, "d")).toEqual({ ...split, right: "d" });
    expect(followTabs({ ...split, focus: "left" }, tabs, "b")).toEqual({
      ...split,
      focus: "left",
      left: "b",
    });
  });

  it("gives a side whose tab closed another tab, never the other side's", () => {
    const left = tabs.filter((item) => item.key !== "a");
    expect(followTabs(split, left, "c")).toEqual({ ...split, left: "b" });
    const right = tabs.filter((item) => item.key !== "c");
    // The store selected "d" after closing "c": it opens on the focused side.
    expect(followTabs(split, right, "d")).toEqual({ ...split, right: "d" });
  });

  it("shows the last open tab on both sides, and ends when none is open", () => {
    expect(followTabs(split, [tab("a")], "a")).toEqual({ ...split, right: "a", focus: "left" });
    expect(followTabs(split, [], "")).toBeNull();
  });

  it("keeps the focused side when both sides show the selected tab", () => {
    const same = { left: "c", right: "c", focus: "right" as const, ratio: 0.5 };
    expect(followTabs(same, tabs, "c")).toBe(same);
    // Another tab picked elsewhere replaces only the focused side's copy.
    expect(followTabs(same, tabs, "a")).toEqual({ ...same, right: "a" });
  });

  it("gives both sides another tab when the tab they both showed closes", () => {
    const same = { left: "c", right: "c", focus: "left" as const, ratio: 0.5 };
    const rest = tabs.filter((item) => item.key !== "c");
    // The store selected "d" after closing "c": it opens on the focused side.
    expect(followTabs(same, rest, "d")).toEqual({ ...same, left: "d", right: "a" });
  });
});

describe("following a link in a side", () => {
  const split = { left: "a", right: "c", focus: "left" as const, ratio: 0.5 };

  it("moves the side it was followed from, where a new file opens", () => {
    expect(navigationSide(split, "a", "left")).toBe("left");
    expect(navigationSide(split, null, "left")).toBe("left");
    expect(navigationSide(split, "d", "right")).toBe("right");
  });

  it("moves the other side when that side already shows the file", () => {
    expect(navigationSide(split, "c", "left")).toBe("right");
  });

  it("moves only the side it was followed from when both sides show the file", () => {
    const same = { ...split, right: "a" };
    expect(navigationSide(same, "a", "right")).toBe("right");
    expect(navigationSide(same, "a", "left")).toBe("left");
  });
});

describe("placing the two sides", () => {
  it("puts the sides next to each other, each below its header", () => {
    expect(splitBoxes("left", 0.4, false)).toEqual({
      header: { top: 0, left: "0.000%", right: "60.000%", height: 32 },
      content: { top: 32, left: "0.000%", right: "60.000%", bottom: 0 },
    });
    expect(splitBoxes("right", 0.4, false).content).toEqual({
      top: 32,
      left: "40.000%",
      right: "0.000%",
      bottom: 0,
    });
  });

  it("stacks them on a phone, below headers as tall as a touch key", () => {
    expect(splitBoxes("right", 0.5, true, 44)).toEqual({
      header: { top: "50.000%", left: 0, right: 0, height: 44 },
      content: { top: "calc(50.000% + 44px)", left: 0, right: 0, bottom: "0.000%" },
    });
  });
});

describe("coming back to the side-by-side view", () => {
  const last = { left: "a", right: "c", focus: "left" as const, ratio: 0.3 };

  it("brings back the last pair and its divider when the selected tab is one of them", () => {
    expect(resumeSplit(last, tabs, "c")).toEqual({ ...last, focus: "right" });
  });

  it("starts afresh for another tab, or when one of the pair closed", () => {
    expect(resumeSplit(last, tabs, "b")).toBeNull();
    expect(
      resumeSplit(
        last,
        tabs.filter((item) => item.key !== "c"),
        "a",
      ),
    ).toBeNull();
    expect(resumeSplit(null, tabs, "a")).toBeNull();
  });
});
