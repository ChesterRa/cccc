import { create } from "zustand";
import type { PresentationMessageRef } from "../types";

/** Where the one resource panel sits: over the message area, or in the right-hand column. */
export type ResourceDock = "left" | "right";

export type WorkspaceResource = {
  kind: "workspace";
  scopeKey: string;
  /** The scope's root as well: a scope key bound to another root is another workspace. */
  scopeUrl: string;
  /** The server-resolved workspace path, so a symlink and its target share one tab. */
  path: string;
};

export type AttachmentResource = {
  kind: "attachment";
  /**
   * The message's stable key: for the user's own messages their client id, which stays the key
   * after the ledger confirms the message, and the event id for everyone else's. With `index`
   * it names one attachment, so the same blob sent in two messages stays two tabs.
   */
  messageKey: string;
  /** The confirmed ledger event, empty until known; returning to the message uses it. */
  eventId: string;
  index: number;
  /** The Group that owns the blob, which can differ from the Group whose panel shows it. */
  blobGroupId: string;
  /**
   * Where the blob lives. Addresses are built from this when shown, so a temporary proof that
   * an address can carry never enters the store or the recently closed list.
   */
  blobPath: string;
  /** The sender's local preview, only until the ledger confirms the message. */
  localPreviewUrl?: string;
  /** Who sent it and when, shown to tell two attachments of the same name apart. */
  sender?: string;
  sentAt?: string;
  media: "image" | "video" | "markdown" | "text" | "pdf";
  isSvg: boolean;
};

/**
 * A Presentation slot, either itself ("live") or as one message quoted it. Closing such a tab only
 * closes this person's view; the shared slot is cleared only by its own explicit action.
 */
export type PresentationResource = {
  kind: "presentation";
  slotId: string;
  /** The slot's position as people see it, 1 to 4. */
  slotIndex: number;
  /** "live" for the slot itself, `${eventId}:${refIndex}` for a quoted reference. */
  refKey: string;
  focusEventId?: string;
  focusRef?: PresentationMessageRef | null;
};

export type ResourceDescriptor = WorkspaceResource | AttachmentResource | PresentationResource;
export type ResourceTab = ResourceDescriptor & { key: string; title: string };
export type AttachmentUpdate = Partial<
  Pick<AttachmentResource, "eventId" | "blobGroupId" | "blobPath" | "localPreviewUrl" | "sentAt">
>;

/** What one close took away: one tab, or every tab of a "close others" or "close all". */
export type ClosedEntry = { id: number; tabs: ResourceTab[] };

type GroupPanel = {
  tabs: ResourceTab[];
  activeKey: string;
  open: boolean;
  /** Covering the messages and the right column together, until restored or collapsed. */
  maximized: boolean;
  closed: ClosedEntry[];
};

const DOCK_KEY = "cccc-resource-panel-dock";
const WIDTH_KEY = "cccc-resource-panel-width";
const CLOSED_LIMIT = 20;
const EMPTY: GroupPanel = { tabs: [], activeKey: "", open: false, maximized: false, closed: [] };
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

export function resourceKey(resource: ResourceDescriptor): string {
  if (resource.kind === "workspace")
    return `workspace:${resource.scopeKey}:${resource.scopeUrl}:${resource.path}`;
  if (resource.kind === "presentation") return `presentation:${resource.slotId}:${resource.refKey}`;
  return `attachment:${resource.messageKey}:${resource.index}`;
}

type ResourcePanelState = {
  dock: ResourceDock;
  /** Docked right, the panel's own column width; until the user drags it, half the row. */
  width: number | null;
  /** Open tabs live per Group in this page only; a refresh starts empty. */
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
  setOpen: (groupId: string, open: boolean) => void;
  setMaximized: (groupId: string, maximized: boolean) => void;
  setDock: (dock: ResourceDock) => void;
  setWidth: (width: number) => void;
};

export const useResourcePanelStore = create<ResourcePanelState>((set, get) => {
  const update = (groupId: string, patch: (panel: GroupPanel) => GroupPanel, choice = true) =>
    set((state) => ({
      generation: choice ? state.generation + 1 : state.generation,
      groups: { ...state.groups, [groupId]: patch(state.groups[groupId] ?? EMPTY) },
    }));
  return {
    dock: readDock(),
    width: readWidth(),
    groups: {},
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
