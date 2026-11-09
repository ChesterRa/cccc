import { afterEach, expect, it, vi } from "vite-plus/test";
import {
  resourceKey,
  type GroupPanel,
  type ResourceDescriptor,
  type ResourceTab,
} from "./resourcePanelModel";
import {
  readResourcePanels,
  saveResourcePanels,
  serializeResourcePanels,
  RESOURCE_PANELS_KEY,
} from "./resourcePanelPersistence";
import { useResourcePanelStore } from "./useResourcePanelStore";

const file = (path: string): ResourceTab => ({
  kind: "workspace",
  scopeKey: "s1",
  scopeUrl: "/repo",
  path,
  title: path,
  key: `workspace:s1:/repo:${path}`,
});
const tab = (resource: ResourceDescriptor): ResourceTab => ({
  ...resource,
  key: resourceKey(resource),
  title: "fixture",
});
const panel = (tabs: ResourceTab[], patch: Partial<GroupPanel> = {}): GroupPanel => ({
  tabs,
  activeKey: tabs[0]?.key ?? "",
  open: true,
  maximized: true,
  closed: [],
  split: null,
  ...patch,
});
const storage = (raw: string | null) => ({ getItem: () => raw });
const read = (groups: Record<string, GroupPanel>) =>
  readResourcePanels(storage(serializeResourcePanels(groups)));
afterEach(() => vi.unstubAllGlobals());

it("round-trips each Group's order, active tab, visibility and layout without an undo history", () => {
  const a = file("a.md"),
    b = file("b.md");
  const result = read({
    g1: panel([a, b], {
      activeKey: b.key,
      closed: [{ id: 1, tabs: [a] }],
      split: { left: a.key, right: b.key, focus: "right", ratio: 0.35 },
    }),
    g2: panel([b], { open: false, maximized: false }),
  });
  expect(result.g1).toEqual(
    panel([a, b], {
      activeKey: b.key,
      split: { left: a.key, right: b.key, focus: "right", ratio: 0.35 },
    }),
  );
  expect(result.g2).toEqual(panel([b], { open: false, maximized: false }));
});

it("keeps the right side focused when both sides show the same resource", () => {
  const a = file("a.md");
  expect(
    read({ g: panel([a], { split: { left: a.key, right: a.key, focus: "right", ratio: 0.6 } }) }).g
      .split,
  ).toEqual({ left: a.key, right: a.key, focus: "right", ratio: 0.6 });
});

it("writes an allowlist, including only confirmed attachment and quote identities", () => {
  const quote = tab({
    kind: "presentation",
    slotId: "slot-1",
    slotIndex: 1,
    refKey: "event:0",
    focusEventId: "event",
    focusRef: {
      kind: "presentation_ref",
      slot_id: "slot-1",
      href: "https://invalid.test/?secret=fixture-only-secret",
      excerpt: "fixture-only-excerpt",
      locator: { extra: "fixture-only-locator" },
    },
  });
  const confirmed = tab({
    kind: "attachment",
    messageKey: "client",
    eventId: "confirmed",
    index: 0,
    blobGroupId: "other-group",
    blobPath: "state/blobs/fixture",
    media: "pdf",
    isSvg: false,
    localPreviewUrl: "blob:fixture-only-local",
  });
  const pending = tab({
    kind: "attachment",
    messageKey: "pending",
    eventId: "",
    index: 0,
    blobGroupId: "g",
    blobPath: "state/blobs/pending",
    media: "image",
    isSvg: false,
  });
  const raw = serializeResourcePanels({ g: panel([quote, confirmed, pending]) });
  for (const forbidden of [
    "focusRef",
    "href",
    "excerpt",
    "locator",
    "localPreviewUrl",
    "fixture-only",
    "pending",
  ])
    expect(raw).not.toContain(forbidden);
  expect(readResourcePanels(storage(raw)).g.tabs).toMatchObject([
    { kind: "presentation", focusEventId: "event", refKey: "event:0" },
    { kind: "attachment", eventId: "confirmed", blobGroupId: "other-group" },
  ]);
});

it("repairs selection and split after omitting an unconfirmed attachment", () => {
  const a = file("a.md");
  const pending = tab({
    kind: "attachment",
    messageKey: "pending",
    eventId: "",
    index: 0,
    blobGroupId: "g",
    blobPath: "",
    media: "image",
    isSvg: false,
  });
  const result = read({
    g: panel([pending, a], {
      activeKey: pending.key,
      split: { left: pending.key, right: a.key, focus: "left", ratio: 0.4 },
    }),
  }).g;
  expect(result.tabs).toEqual([a]);
  expect(result.activeKey).toBe(a.key);
  expect(result.split).toBeNull();
  expect(read({ g: panel([pending]) }).g).toMatchObject({
    tabs: [],
    activeKey: "",
    open: false,
    maximized: false,
  });
});

it("recomputes keys, removes duplicates and clamps a restored divider", () => {
  const a = { ...file("a.md"), key: "untrusted-key" };
  const canonical = "workspace:s1:/repo:a.md";
  const raw = JSON.stringify({
    v: 1,
    groups: {
      g: panel([a, a], {
        activeKey: canonical,
        split: { left: canonical, right: canonical, focus: "right", ratio: 9 },
      }),
    },
  });
  const result = readResourcePanels(storage(raw)).g;
  expect(result.tabs).toEqual([file("a.md")]);
  expect(result.split).toEqual({ left: canonical, right: canonical, focus: "right", ratio: 0.8 });
});

it.each([null, "broken JSON", '{"v":2,"groups":{}}', '{"v":1,"groups":[]}'])(
  "ignores an unreadable or unsupported saved session: %s",
  (raw) => {
    expect(readResourcePanels(storage(raw))).toEqual({});
  },
);

it("does not let malformed tabs or quote indices into the restored state", () => {
  const invalid = {
    kind: "presentation",
    slotId: "slot-1",
    slotIndex: 1,
    refKey: "event:NaN",
    focusEventId: "event",
    title: "bad quote",
  };
  const raw = JSON.stringify({
    v: 1,
    groups: {
      g: {
        ...panel([file("a.md")]),
        tabs: [null, invalid, { kind: "workspace", title: "missing path" }, file("a.md")],
      },
    },
  });
  expect(readResourcePanels(storage(raw)).g.tabs).toEqual([file("a.md")]);
});

it("keeps the panel usable when reads or writes are denied", () => {
  expect(
    readResourcePanels({
      getItem: () => {
        throw new Error("denied");
      },
    }),
  ).toEqual({});
  expect(() =>
    saveResourcePanels(
      { g: panel([file("a.md")]) },
      {
        setItem: () => {
          throw new Error("full");
        },
      },
    ),
  ).not.toThrow();
});

it("persists actual store changes without turning layout restoration into a choice", () => {
  const values = new Map<string, string>();
  vi.stubGlobal("sessionStorage", {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
  });
  useResourcePanelStore.setState({ groups: {}, generation: 0 });
  const store = useResourcePanelStore.getState();
  const a = file("a.md");
  store.open("g", a, a.title);
  const generation = useResourcePanelStore.getState().generation;
  store.setSplit("g", { left: a.key, right: a.key, focus: "right", ratio: 0.4 });
  expect(useResourcePanelStore.getState().generation).toBe(generation);
  expect(values.has(RESOURCE_PANELS_KEY)).toBe(true);
  expect(readResourcePanels().g.split?.focus).toBe("right");
});
