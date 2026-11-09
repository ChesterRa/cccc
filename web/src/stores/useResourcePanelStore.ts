import { create } from "zustand";
import {
  resourceKey,
  type AttachmentUpdate,
  type GroupPanel,
  type ResourceDescriptor,
  type ResourceDock,
  type ResourceSplit,
  type ResourceTab,
  type WorkspaceResource,
} from "./resourcePanelModel";
import { readResourcePanels, saveResourcePanels } from "./resourcePanelPersistence";

export { resourceKey } from "./resourcePanelModel";
export type {
  AttachmentResource,
  AttachmentUpdate,
  ClosedEntry,
  PresentationResource,
  ResourceDescriptor,
  ResourceDock,
  ResourceTab,
  WorkspaceResource,
} from "./resourcePanelModel";

const DOCK_KEY = "cccc-resource-panel-dock";
const WIDTH_KEY = "cccc-resource-panel-width";
const CLOSED_LIMIT = 20;
const EMPTY: GroupPanel = {
  tabs: [],
  activeKey: "",
  open: false,
  maximized: false,
  closed: [],
  split: null,
};
let closedId = 0;

function readDock(): ResourceDock {
  try {
    return localStorage.getItem(DOCK_KEY) === "left" ? "left" : "right";
  } catch {
    return "right";
  }
}

function readWidth(): number | null {
  try {
    const value = Number(localStorage.getItem(WIDTH_KEY));
    return Number.isFinite(value) && value > 0 ? Math.round(value) : null;
  } catch {
    return null;
  }
}

type ResourcePanelState = {
  dock: ResourceDock;
  /** Docked right, the panel's own column width; until the user drags it, half the row. */
  width: number | null;
  /** Each browser tab restores resource identities and layout per Group across refreshes. */
  groups: Record<string, GroupPanel>;
  /**
   * Bumped by every choice the user makes in the panel. A slow open that finishes after a newer
   * choice adds its tab without taking the selection back.
   */
  generation: number;
  /**
   * Adds a tab if it is missing, leaving the selection and visibility as they are. Closing a tab
   * whose file is still loading retires that request, so a late response cannot reopen it.
   */
  ensure: (groupId: string, resource: ResourceDescriptor, title: string) => void;
  open: (groupId: string, resource: ResourceDescriptor, title: string) => string;
  activate: (groupId: string, key: string) => void;
  /**
   * Closes the given tabs as one step, so "close all" is restored as a whole. Returns the
   * recently closed entry it made, or null when none of the tabs was open.
   */
  closeTabs: (groupId: string, keys: string[]) => number | null;
  /** Reopens a closed entry from its source; discarded edits do not come back with it. */
  restore: (groupId: string, entryId: number) => void;
  /**
   * Points an already open or recently closed attachment at its confirmed message and server
   * address. It never adds a tab, selects one or shows the panel.
   */
  updateAttachment: (
    groupId: string,
    messageKey: string,
    index: number,
    update: AttachmentUpdate,
  ) => void;
  /** Names a tab after what it now shows, such as a slot's new card; the selection stays. */
  retitle: (groupId: string, key: string, title: string) => void;
  /**
   * Follows a rename or move in a workspace: open and recently closed tabs at `from`, or under it
   * when it is a folder, move to `to`; the selection stays on the same file.
   */
  moveWorkspacePath: (
    groupId: string,
    scopeKey: string,
    scopeUrl: string,
    from: string,
    to: string,
  ) => void;
  setSplit: (
    groupId: string,
    split: ResourceSplit | null | ((current: ResourceSplit | null) => ResourceSplit | null),
  ) => void;
  setOpen: (groupId: string, open: boolean) => void;
  setMaximized: (groupId: string, maximized: boolean) => void;
  setDock: (dock: ResourceDock) => void;
  setWidth: (width: number) => void;
};

export const useResourcePanelStore = create<ResourcePanelState>((set, get) => {
  const update = (groupId: string, patch: (panel: GroupPanel) => GroupPanel, choice = true) =>
    set((state) => {
      const previous = state.groups[groupId] ?? EMPTY;
      const panel = patch(previous);
      if (!choice && panel === previous) return state;
      return {
        generation: choice ? state.generation + 1 : state.generation,
        groups: { ...state.groups, [groupId]: panel },
      };
    });
  return {
    dock: readDock(),
    width: readWidth(),
    groups: readResourcePanels(),
    generation: 0,
    ensure: (groupId, resource, title) => {
      const key = resourceKey(resource);
      update(
        groupId,
        (panel) => {
          if (panel.tabs.some((tab) => tab.key === key)) return panel;
          return {
            ...panel,
            tabs: [...panel.tabs, { ...resource, key, title }],
            activeKey: panel.activeKey || key,
          };
        },
        false,
      );
    },
    open: (groupId, resource, title) => {
      const key = resourceKey(resource);
      update(groupId, (panel) => ({
        ...panel,
        // Opening something already open selects it; it never adds a second tab.
        tabs: panel.tabs.some((tab) => tab.key === key)
          ? panel.tabs
          : [...panel.tabs, { ...resource, key, title }],
        activeKey: key,
        open: true,
      }));
      return key;
    },
    activate: (groupId, key) =>
      update(groupId, (panel) =>
        panel.tabs.some((tab) => tab.key === key)
          ? { ...panel, activeKey: key, open: true }
          : panel,
      ),
    closeTabs: (groupId, keys) => {
      const current = get().groups[groupId] ?? EMPTY;
      if (!current.tabs.some((tab) => keys.includes(tab.key))) return null;
      const id = ++closedId;
      update(groupId, (panel) => {
        const closing = panel.tabs.filter((tab) => keys.includes(tab.key));
        if (!closing.length) return panel;
        const tabs = panel.tabs.filter((tab) => !keys.includes(tab.key));
        let activeKey = panel.activeKey;
        if (keys.includes(activeKey)) {
          // Like a browser, the selection moves to the nearest tab on the right, else the left.
          const index = panel.tabs.findIndex((tab) => tab.key === activeKey);
          const right = panel.tabs.slice(index + 1).find((tab) => !keys.includes(tab.key));
          const left = panel.tabs
            .slice(0, index)
            .reverse()
            .find((tab) => !keys.includes(tab.key));
          activeKey = (right ?? left)?.key ?? "";
        }
        return {
          tabs,
          activeKey,
          open: panel.open && tabs.length > 0,
          maximized: panel.maximized && tabs.length > 0,
          closed: [{ id, tabs: closing }, ...panel.closed].slice(0, CLOSED_LIMIT),
          split: tabs.length ? panel.split : null,
        };
      });
      return id;
    },
    restore: (groupId, entryId) =>
      update(groupId, (panel) => {
        const entry = panel.closed.find((item) => item.id === entryId);
        if (!entry) return panel;
        const present = new Set(panel.tabs.map((tab) => tab.key));
        const back = entry.tabs.filter((tab) => !present.has(tab.key));
        return {
          ...panel,
          tabs: [...panel.tabs, ...back],
          activeKey: entry.tabs[entry.tabs.length - 1]?.key ?? panel.activeKey,
          open: true,
          closed: panel.closed.filter((item) => item.id !== entryId),
        };
      }),
    updateAttachment: (groupId, messageKey, index, change) =>
      update(
        groupId,
        (panel) => {
          const apply = (tab: ResourceTab): ResourceTab =>
            tab.kind === "attachment" && tab.messageKey === messageKey && tab.index === index
              ? { ...tab, ...change }
              : tab;
          return {
            ...panel,
            tabs: panel.tabs.map(apply),
            closed: panel.closed.map((entry) => ({ ...entry, tabs: entry.tabs.map(apply) })),
          };
        },
        false,
      ),
    retitle: (groupId, key, title) => {
      const tab = get().groups[groupId]?.tabs.find((item) => item.key === key);
      if (!tab || tab.title === title) return;
      update(
        groupId,
        (panel) => ({
          ...panel,
          tabs: panel.tabs.map((item) => (item.key === key ? { ...item, title } : item)),
        }),
        false,
      );
    },
    moveWorkspacePath: (groupId, scopeKey, scopeUrl, from, to) => {
      const moved = (tab: ResourceTab): ResourceTab => {
        if (tab.kind !== "workspace" || tab.scopeKey !== scopeKey || tab.scopeUrl !== scopeUrl)
          return tab;
        if (tab.path !== from && !tab.path.startsWith(`${from}/`)) return tab;
        const path = to + tab.path.slice(from.length);
        const resource: WorkspaceResource = { kind: "workspace", scopeKey, scopeUrl, path };
        return { ...resource, key: resourceKey(resource), title: path.split("/").pop() || path };
      };
      // A tab left open for a file deleted elsewhere can already sit at the destination; one
      // file keeps one tab.
      const unique = (list: ResourceTab[]) => {
        const seen = new Set<string>();
        return list.filter((tab) => !seen.has(tab.key) && !!seen.add(tab.key));
      };
      update(
        groupId,
        (panel) => {
          const mapped = panel.tabs.map(moved);
          const active = panel.tabs.findIndex((tab) => tab.key === panel.activeKey);
          return {
            ...panel,
            tabs: unique(mapped),
            activeKey: active >= 0 ? mapped[active].key : panel.activeKey,
            closed: panel.closed.map((entry) => ({
              ...entry,
              tabs: unique(entry.tabs.map(moved)),
            })),
          };
        },
        false,
      );
    },
    setSplit: (groupId, change) =>
      update(
        groupId,
        (panel) => {
          const split = typeof change === "function" ? change(panel.split) : change;
          return split === panel.split ? panel : { ...panel, split };
        },
        false,
      ),
    setOpen: (groupId, open) =>
      update(groupId, (panel) => {
        const next = open && panel.tabs.length > 0;
        return { ...panel, open: next, maximized: next && panel.maximized };
      }),
    setMaximized: (groupId, maximized) =>
      update(groupId, (panel) => ({
        ...panel,
        maximized: maximized && panel.tabs.length > 0,
        open: panel.open || (maximized && panel.tabs.length > 0),
      })),
    setDock: (dock) => {
      try {
        localStorage.setItem(DOCK_KEY, dock);
      } catch {
        // The panel still moves when the browser refuses to store the preference.
      }
      set({ dock });
    },
    setWidth: (width) => {
      try {
        localStorage.setItem(WIDTH_KEY, String(Math.round(width)));
      } catch {
        // The width still applies for this page when the browser refuses to store it.
      }
      set({ width: Math.round(width) });
    },
  };
});

export function useGroupResourcePanel(groupId: string): GroupPanel {
  return useResourcePanelStore((state) => state.groups[groupId] ?? EMPTY);
}

// Persist references rather than document contents or temporary authorization URLs.
useResourcePanelStore.subscribe((state, previous) => {
  if (state.groups !== previous.groups) saveResourcePanels(state.groups);
});
