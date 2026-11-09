import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";
import { useTranslation } from "react-i18next";
import {
  ChevronDown,
  ChevronLeft,
  Columns2,
  ExternalLink,
  FileText,
  Image as ImageGlyph,
  Maximize2,
  Minimize2,
  Presentation,
  SquareChevronLeft,
  SquareChevronRight,
} from "lucide-react";
import type { GroupPresentation } from "../../types";
import {
  type ClosedEntry,
  type PresentationResource,
  type ResourceDock,
  type ResourceTab,
  useGroupResourcePanel,
  useResourcePanelStore,
} from "../../stores/useResourcePanelStore";
import { getChatSession, useUIStore } from "../../stores/useUIStore";
import { classNames } from "../../utils/classNames";
import { buildMessageAttachmentLinks } from "../../utils/messageAttachmentLinks";
import { readFrameProof } from "../../features/connect/protocol";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";
import { CloseIcon, DownloadIcon, LcdIcon } from "../Icons";
import { buttonVariants } from "../ui/button-variants";
import { Popover, PopoverAnchor, PopoverContent } from "../ui/popover";
import { WorkspaceEntryMenu } from "../workspace/WorkspaceEntryMenu";
import { WorkspaceViewer } from "../workspace/WorkspaceViewer";
import type { WorkspaceFilesController } from "../workspace/useWorkspaceFiles";
import { ResourceTabsList } from "./ResourceTabsList";
import { SplitDivider, SplitPaneHeader } from "./ResourceSplit";
import {
  followTabs,
  resumeSplit,
  type ResourceSplit,
  type SplitSide,
  navigationSide,
  SPLIT_HEADER,
  splitBoxes,
  splitPartner,
  startSplit,
} from "./splitLayout";
import { AttachmentResourceView } from "./views/AttachmentResourceView";
import { WorkspaceResourceView } from "./views/WorkspaceResourceView";
import { findPresentationSlot } from "../../utils/presentation";
import type { PresentationResourceHandlers } from "./views/PresentationResourceView";
import { useFrontWorkspaceLoad } from "./useFrontWorkspaceLoad";
import { useWorkspaceScrollMemory } from "./useWorkspaceScrollMemory";
import { useMediaQuery } from "../../hooks/useMediaQuery";
import {
  NO_RECENT_ATTACHMENTS,
  mountedAttachments,
  noteRecentAttachment,
} from "./attachmentMounts";

// The Presentation viewer is large and only needed once a slot is open in the panel.
const PresentationResourceView = lazy(() =>
  import("./views/PresentationResourceView").then((module) => ({
    default: module.PresentationResourceView,
  })),
);

/**
 * Every key in the panel's bar has one height; a key with a word or a count grows sideways.
 * Built without the shared button sizes: the larger of two heights wins in the stylesheet, so a
 * smaller one added after a size never applies.
 */
const barKey =
  "inline-flex h-8 min-w-8 shrink-0 items-center justify-center gap-1 rounded-md border border-transparent px-1.5 text-xs font-medium tabular-nums text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)] pointer-coarse:h-10 pointer-coarse:min-w-10";
/**
 * Keeps the panel on top of whichever area it is docked to.
 *
 * The panel is mounted once and never moves in the DOM: moving an iframe reloads it, so a dock
 * change only rewrites this element's box from the slot it follows.
 */
function useFollowSlot(
  panel: RefObject<HTMLDivElement | null>,
  slot: HTMLElement | null,
  container: RefObject<HTMLDivElement | null>,
) {
  useLayoutEffect(() => {
    const node = panel.current;
    const frame = container.current;
    if (!node || !frame || !slot) return;
    const place = () => {
      const outer = frame.getBoundingClientRect();
      const inner = slot.getBoundingClientRect();
      node.style.left = `${inner.left - outer.left}px`;
      node.style.top = `${inner.top - outer.top}px`;
      node.style.width = `${inner.width}px`;
      node.style.height = `${inner.height}px`;
    };
    place();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", place);
      return () => window.removeEventListener("resize", place);
    }
    const observer = new ResizeObserver(place);
    observer.observe(slot);
    observer.observe(frame);
    return () => observer.disconnect();
  }, [panel, slot, container]);
}

/** A workspace tab belongs to one scope key and root; another root is another workspace. */
function sameScope(
  tab: { scopeKey: string; scopeUrl: string },
  files: { scopeKey: string; scopeUrl: string },
) {
  return tab.scopeKey === files.scopeKey && tab.scopeUrl === files.scopeUrl;
}

/** Addresses are built when shown, from where the blob lives; the store keeps no address. */
function attachmentLinks(tab: ResourceTab & { kind: "attachment" }) {
  return buildMessageAttachmentLinks({
    groupId: tab.blobGroupId,
    path: tab.blobPath,
    title: tab.title,
    localPreviewUrl: tab.localPreviewUrl,
    fallbackLabel: tab.media,
  });
}

function TabIcon({ tab }: { tab: ResourceTab }) {
  if (tab.kind === "attachment" && tab.media === "image" && !tab.isSvg)
    return (
      <img
        src={attachmentLinks(tab).previewHref}
        alt=""
        loading="lazy"
        decoding="async"
        className="h-4 w-4 shrink-0 rounded-sm object-cover"
        data-resource-tab-thumbnail
      />
    );
  return (
    <LcdIcon
      icon={
        tab.kind === "workspace"
          ? FileText
          : tab.kind === "presentation"
            ? Presentation
            : ImageGlyph
      }
      glyph={
        tab.kind === "workspace" ? "file-text" : tab.kind === "presentation" ? "monitor" : "image"
      }
      className="h-4 w-4 shrink-0"
      aria-hidden="true"
    />
  );
}

type PendingClose = { keys: string[]; blocked: ResourceTab[] };

/**
 * A tab from a folder that is not the Group's active one. Its unsaved text stays readable here,
 * so switching folders never hides the only copy of someone's edits.
 */
function OtherScopeNotice({ draft }: { draft: string | null }) {
  const { t } = useTranslation("chat");
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2 p-4 text-sm" data-resource-other-scope>
      <p className="text-[var(--color-text-secondary)]">{t("resourcePanel.otherScope")}</p>
      {draft !== null ? (
        <>
          <p className="text-[var(--color-text-secondary)]">{t("resourcePanel.unsaved")}</p>
          <textarea
            readOnly
            value={draft}
            aria-label={t("resourcePanel.unsaved")}
            className="min-h-0 flex-1 resize-none rounded-md border border-[var(--glass-border-subtle)] bg-transparent p-2 font-mono text-xs leading-relaxed"
            data-resource-other-scope-draft
          />
        </>
      ) : null}
    </div>
  );
}

export function ResourcePanel({
  groupId,
  dock,
  layout = "docked",
  visible,
  slot,
  container,
  files,
  scopeReady = true,
  isDark,
  readOnly,
  presentation,
  onLocateMessage,
  onAttachPath,
}: {
  groupId: string;
  dock: ResourceDock;
  /** On a phone the panel is one full-screen surface with a way back, and no sides to choose. */
  layout?: "docked" | "phone";
  /** False while docked over the messages and the terminal view is showing. */
  visible: boolean;
  slot: HTMLElement | null;
  container: RefObject<HTMLDivElement | null>;
  /** openFile and locatePath also select the file's tab once it is read; loadFile does not. */
  files: WorkspaceFilesController & { loadFile: WorkspaceFilesController["openFile"] };
  /** The current Group's scope list has arrived, including an empty list. */
  scopeReady?: boolean;
  isDark: boolean;
  readOnly: boolean;
  /** What a Presentation tab needs from the page: the slots, and the slot actions it offers. */
  presentation?: {
    data: GroupPresentation | null;
    readOnly: boolean;
    handlers: PresentationResourceHandlers;
    onOpenWindow?: (tab: PresentationResource) => void;
  };
  /** Shows a message, bringing the message view forward from whatever covers it. */
  onLocateMessage: (eventId: string) => void;
  onAttachPath: (path: string) => void;
}) {
  const { t, i18n } = useTranslation("chat");
  const panel = useGroupResourcePanel(groupId);
  const tabs = panel.tabs;
  // A slot's own tab is named after the slot's current card, so the strip, All, the tiles and the
  // recently closed list use the name on screen; a message's reference keeps the title it was
  // quoted with.
  const retitle = useResourcePanelStore((state) => state.retitle);
  useEffect(() => {
    for (const tab of panel.tabs) {
      if (tab.kind !== "presentation" || tab.refKey !== "live") continue;
      const title =
        String(findPresentationSlot(presentation?.data, tab.slotId)?.card?.title || "").trim() ||
        t("resourcePanel.fromSlot", { slot: tab.slotIndex });
      if (title !== tab.title) retitle(groupId, tab.key, title);
    }
  }, [groupId, panel.tabs, presentation?.data, retitle, t]);
  // Restores and undo run later than the render that offered them; they use the editor as it is now.
  const filesRef = useRef(files);
  filesRef.current = files;
  const activate = useResourcePanelStore((state) => state.activate);
  const closeTabs = useResourcePanelStore((state) => state.closeTabs);
  const restore = useResourcePanelStore((state) => state.restore);
  const setOpen = useResourcePanelStore((state) => state.setOpen);
  const setDock = useResourcePanelStore((state) => state.setDock);
  const showNotice = useUIStore((state) => state.showNotice);
  const unread = useUIStore((state) => getChatSession(groupId, state.chatSessions).chatUnreadCount);
  const root = useRef<HTMLDivElement>(null);
  const strip = useRef<HTMLDivElement>(null);
  const maximized = panel.maximized;
  const setMaximized = useResourcePanelStore((state) => state.setMaximized);
  // Two tabs side by side, as in an editor's split view; leaving it keeps every tab. The tab read
  // before the selected one is the other side's first choice.
  const split = panel.split;
  const setGroupSplit = useResourcePanelStore((state) => state.setSplit);
  const setSplit = useCallback(
    (change: ResourceSplit | null | ((current: ResourceSplit | null) => ResourceSplit | null)) =>
      setGroupSplit(groupId, change),
    [groupId, setGroupSplit],
  );
  // Leaving and coming back finds the same pair, while both are open.
  const lastSplit = useRef<ResourceSplit | null>(null);
  const leaveSplit = useCallback(() => {
    setSplit((current) => {
      if (current) lastSplit.current = current;
      return null;
    });
  }, [setSplit]);
  // A link followed in a side moves one side only. A link within the file moves the side it
  // was followed from, without the editor. A link to another file goes through the editor and
  // moves the side already showing that file, or else the current side, where the file opens;
  // both are settled when the file arrives. With one file on both sides, the other side keeps
  // its place.
  const [sideNavigation, setSideNavigation] = useState<{
    navigation: typeof files.navigation;
    side: SplitSide | null;
    path: string;
  }>({ navigation: null, side: null, path: "" });
  const [editorNavigation, setEditorNavigation] = useState(files.navigation);
  if (editorNavigation !== files.navigation) {
    setEditorNavigation(files.navigation);
    const target = files.file
      ? tabs.find(
          (tab) =>
            tab.kind === "workspace" &&
            tab.path === files.file?.path &&
            tab.scopeKey === files.file.scope_key &&
            tab.scopeUrl === files.file.scope_url,
        )
      : undefined;
    setSideNavigation({
      navigation: files.navigation,
      side: split ? navigationSide(split, target?.key ?? null, split.focus) : null,
      path: files.file?.path ?? "",
    });
  }
  const previousKey = useRef("");
  const lastActiveKey = useRef(panel.activeKey);
  useEffect(() => {
    if (panel.activeKey === lastActiveKey.current) return;
    previousKey.current = lastActiveKey.current;
    lastActiveKey.current = panel.activeKey;
  }, [panel.activeKey]);
  useEffect(() => {
    setSplit((current) => (current ? followTabs(current, tabs, panel.activeKey) : current));
  }, [panel.activeKey, setSplit, tabs]);
  const splitSide = (key: string): SplitSide | null =>
    split?.left === key ? "left" : split?.right === key ? "right" : null;
  const [listOpen, setListOpen] = useState(false);
  const [menu, setMenu] = useState<{
    tab: ResourceTab;
    x: number;
    y: number;
    anchor: HTMLElement;
  } | null>(null);
  const [pending, setPending] = useState<PendingClose | null>(null);
  const listAnchor = useRef<HTMLButtonElement>(null);
  const phone = layout === "phone";
  // Touch makes every button at least 44px tall; the sides' headers grow to match.
  const touch = useMediaQuery("(pointer: coarse)");
  const splitHeader = touch ? SPLIT_HEADER.touch : SPLIT_HEADER.pointer;
  useFollowSlot(root, phone ? null : maximized ? container.current : slot, container);

  const active = tabs.find((tab) => tab.key === panel.activeKey) ?? null;
  // Only the tab's own file shows under it. While the editor still holds another file, because
  // this one is loading or could not be read, the tab says so instead of showing that file.
  const workspaceShown =
    scopeReady &&
    active?.kind === "workspace" &&
    sameScope(active, files) &&
    files.file?.path === active.path;
  const workspacePending =
    scopeReady && active?.kind === "workspace" && sameScope(active, files) && !workspaceShown
      ? active
      : null;
  const workspaceLoading = (
    <p
      role="status"
      className="p-4 text-sm text-[var(--color-text-secondary)]"
      data-resource-workspace-loading
    >
      {t("common:loading")}
    </p>
  );
  const workspaceKept =
    !!files.file &&
    tabs.some(
      (tab) => tab.kind === "workspace" && sameScope(tab, files) && tab.path === files.file?.path,
    );
  const [presentationPick, setPresentationPick] = useState<Record<string, string>>({});
  useEffect(() => {
    if (active?.kind !== "presentation") return;
    setPresentationPick((current) =>
      current[active.slotId] === active.key ? current : { ...current, [active.slotId]: active.key },
    );
  }, [active]);
  // One viewer per slot. A tab on a side of the split view takes it, so both sides showing the
  // same slot share one viewer and session rather than starting a second.
  const presentationViews = useMemo(() => {
    const sides = split ? [split.left, split.right] : [];
    const onSide = (slotId: string) =>
      sides.find((key) =>
        tabs.some((tab) => tab.key === key && tab.kind === "presentation" && tab.slotId === slotId),
      );
    const bySlot = new Map<string, ResourceTab & PresentationResource>();
    for (const tab of tabs) {
      if (tab.kind !== "presentation") continue;
      const picked =
        active?.kind === "presentation" && active.slotId === tab.slotId
          ? active.key
          : (onSide(tab.slotId) ?? presentationPick[tab.slotId]);
      if (!bySlot.has(tab.slotId) || tab.key === picked) bySlot.set(tab.slotId, tab);
    }
    return [...bySlot.values()];
  }, [active, tabs, presentationPick, split]);
  const body = useRef<HTMLDivElement>(null);
  const forgetScroll = useWorkspaceScrollMemory(
    body,
    !split &&
      active?.kind === "workspace" &&
      sameScope(active, files) &&
      files.file?.path === active.path &&
      !files.fileLoading
      ? active.key
      : "",
    files.navigation,
  );
  const shown = visible && panel.open && !!active && (phone || !!slot || maximized);
  // A late open or a change of folder can leave another file in the shared editor; the tab in
  // front then loads its own file again. That load is not a choice: it must not select a tab.
  useFrontWorkspaceLoad(
    shown && !split && workspacePending ? workspacePending.path : "",
    {
      path: files.file?.path ?? "",
      selectedPath: files.selectedPath,
      loading: files.fileLoading,
      pathLoading: files.pathLoading,
      error: files.fileError,
    },
    files.loadFile,
  );

  // Attachments seen most recently stay mounted; older ones load again when selected.
  const [recentAttachments, setRecentAttachments] = useState(NO_RECENT_ATTACHMENTS);
  useEffect(() => {
    setRecentAttachments((current) => noteRecentAttachment(current, tabs, panel.activeKey));
  }, [panel.activeKey, tabs]);
  const mounted = useMemo(() => {
    const kept = mountedAttachments(tabs, panel.activeKey, recentAttachments);
    // Both sides of the split view show their attachments.
    if (split) for (const key of [split.left, split.right]) kept.add(key);
    return kept;
  }, [panel.activeKey, tabs, recentAttachments, split]);

  const select = useCallback(
    (tab: ResourceTab) => {
      setPending(null);
      activate(groupId, tab.key);
      // Workspace tabs share the one editor; its draft map keeps unsaved text per path. A tab
      // from another scope is never opened against the current one. The split view reads its
      // files on its own, and the editor opens the selected file when it is shown alone again.
      if (tab.kind === "workspace" && sameScope(tab, files) && !split)
        void files.openFile(tab.path);
    },
    [activate, files, groupId, split],
  );

  // A file keeps its unsaved edits even after the Group's active folder changes, so a tab from
  // another folder is still asked about before it closes.
  const editState = useCallback(
    (tab: ResourceTab) =>
      tab.kind === "workspace"
        ? files.getFileEditStatus(tab.path, tab.scopeKey, tab.scopeUrl)
        : { dirty: false, saving: false },
    [files],
  );

  /** Closes tabs as one step. Files with unsaved edits are never closed silently. */
  const close = useCallback(
    (keys: string[], options: { force?: boolean } = {}) => {
      const closing = tabs.filter((tab) => keys.includes(tab.key));
      if (!closing.length) return;
      const blocked = options.force
        ? []
        : closing.filter((tab) => {
            const state = editState(tab);
            return state.dirty || state.saving;
          });
      if (blocked.length) {
        // A single unsaved file is shown first, so saving it is one click away.
        if (closing.length === 1 && blocked[0].key !== panel.activeKey) select(blocked[0]);
        // Saving goes through the editor. Side by side, selecting does not load the editor, so
        // the guard loads the file there itself.
        const only = blocked.length === 1 ? blocked[0] : null;
        if (
          split &&
          only?.kind === "workspace" &&
          sameScope(only, files) &&
          files.file?.path !== only.path
        )
          void files.openFile(only.path);
        setPending({ keys, blocked });
        return;
      }
      setPending(null);
      const nextActive = (() => {
        if (!keys.includes(panel.activeKey)) return null;
        const index = tabs.findIndex((tab) => tab.key === panel.activeKey);
        return (
          tabs.slice(index + 1).find((tab) => !keys.includes(tab.key)) ??
          tabs
            .slice(0, index)
            .reverse()
            .find((tab) => !keys.includes(tab.key)) ??
          null
        );
      })();
      const entryId = closeTabs(groupId, keys);
      forgetScroll(keys);
      // Closing the file on screen, or one still loading, retires the editor's request too, so
      // a late response cannot reopen the tab.
      if (
        closing.some(
          (tab) =>
            tab.kind === "workspace" &&
            sameScope(tab, files) &&
            (files.file?.path === tab.path ||
              (files.fileLoading && files.selectedPath === tab.path)),
        )
      )
        files.closeFile();
      if (nextActive?.kind === "workspace" && sameScope(nextActive, files))
        void files.openFile(nextActive.path);
      if (closing.length > 1 && entryId !== null) {
        showNotice({
          message: t("resourcePanel.closedNotice", { n: closing.length }),
          actionLabel: t("resourcePanel.undo"),
          durationMs: 8000,
          onAction: () => {
            const entry = useResourcePanelStore
              .getState()
              .groups[groupId]?.closed.find((item) => item.id === entryId);
            if (entry) restoreEntryRef.current(entry);
          },
        });
      }
    },
    [
      closeTabs,
      editState,
      files,
      forgetScroll,
      groupId,
      panel.activeKey,
      split,
      tabs,
      select,
      showNotice,
      t,
    ],
  );

  const restoreEntry = useCallback(
    (entry: ClosedEntry) => {
      restore(groupId, entry.id);
      const last = entry.tabs[entry.tabs.length - 1];
      const current = filesRef.current;
      if (last?.kind === "workspace" && sameScope(last, current)) void current.openFile(last.path);
    },
    [groupId, restore],
  );
  const restoreEntryRef = useRef(restoreEntry);
  restoreEntryRef.current = restoreEntry;

  const keysExcept = (tab: ResourceTab) =>
    tabs.filter((item) => item.key !== tab.key).map((item) => item.key);
  const keysRightOf = (tab: ResourceTab) =>
    tabs.slice(tabs.findIndex((item) => item.key === tab.key) + 1).map((item) => item.key);
  const allKeys = () => tabs.map((tab) => tab.key);

  const collapse = useCallback(() => {
    leaveSplit();
    setOpen(groupId, false);
  }, [groupId, leaveSplit, setOpen]);

  const toggleSplit = () => {
    if (split) leaveSplit();
    else
      setSplit(
        resumeSplit(lastSplit.current, tabs, panel.activeKey) ??
          startSplit(
            tabs,
            panel.activeKey,
            splitPartner(tabs, panel.activeKey, previousKey.current),
          ),
      );
  };
  // Pointing into a side, or moving focus into it, makes it the current side and its tab the
  // selected one.
  const focusSide = (side: SplitSide) => {
    if (!split || split.focus === side) return;
    if (split[side] === panel.activeKey)
      setSplit((current) => current && { ...current, focus: side });
    else activate(groupId, split[side]);
  };

  // Keep the selected tab in view when it is opened or picked; a manual scroll is left alone.
  useEffect(() => {
    strip.current
      ?.querySelector<HTMLElement>('[aria-selected="true"]')
      ?.parentElement?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, [panel.activeKey, shown, dock, maximized]);

  // A vertical wheel scrolls the tab strip sideways, as in browsers' crowded tab strips.
  useEffect(() => {
    const node = strip.current;
    if (!node) return;
    const onWheel = (event: WheelEvent) => {
      if (Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
      node.scrollLeft += event.deltaY;
      event.preventDefault();
    };
    node.addEventListener("wheel", onWheel, { passive: false });
    return () => node.removeEventListener("wheel", onWheel);
  }, []);

  // A file opened elsewhere while this one waited to be saved retires the question.
  useEffect(() => {
    if (pending && !pending.keys.some((key) => tabs.some((tab) => tab.key === key)))
      setPending(null);
  }, [tabs, pending]);

  const workspaceView = {
    ...files,
    // A diff picked in Changes keeps its own view over the messages; a file tab shows the file.
    mode: "files" as const,
    // The viewer's own close button closes this tab.
    closeFile: () => {
      if (active) close([active.key]);
    },
  };

  // Chinese and Japanese list names with an ideographic comma.
  const names = (tabs: ResourceTab[]) =>
    tabs
      .map((tab) => tab.title)
      .join(/^(zh|ja)/.test(i18n.resolvedLanguage || i18n.language) ? "、" : ", ");

  const savePending = async () => {
    const tab = pending?.blocked[0];
    if (!tab || tab.kind !== "workspace" || !sameScope(tab, files) || files.file?.path !== tab.path)
      return;
    const saved = await files.saveFile(files.draft);
    if (saved) close(pending.keys);
  };
  const discardPending = () => {
    if (!pending) return;
    const kept = pending.blocked.filter(
      (tab) =>
        tab.kind === "workspace" && !files.discardFileDraft(tab.path, tab.scopeKey, tab.scopeUrl),
    );
    if (kept.length) {
      setPending({ ...pending, blocked: kept });
      return;
    }
    close(pending.keys, { force: true });
  };
  // Judged again on the click: a tab edited while the question was showing stays open.
  const closeClean = () => {
    if (!pending) return;
    const clean = pending.keys.filter((key) => {
      const tab = tabs.find((item) => item.key === key);
      if (!tab) return false;
      const state = editState(tab);
      return !state.dirty && !state.saving;
    });
    setPending(null);
    if (clean.length) close(clean, { force: true });
  };

  // Over the message list, or maximized over everything, the panel keeps the messages from view.
  const unreadShown = !phone && (dock === "left" || maximized) && unread > 0;
  const collapseLabel = unreadShown
    ? `${t("resourcePanel.hide")} · ${t("workView.unread", { count: unread })}`
    : t("resourcePanel.hide");

  const toolbar =
    active?.kind === "attachment" ? (
      <div className="flex h-9 pointer-coarse:h-12 shrink-0 items-center gap-1 border-b border-[var(--glass-border-subtle)] px-2 text-xs text-[var(--color-text-tertiary)]">
        <span className="min-w-0 flex-1 truncate" title={active.title}>
          {active.sender ? `${active.title} · ${active.sender}` : active.title}
        </span>
        {active.eventId ? (
          <button
            type="button"
            className={`${buttonVariants({ variant: "ghost", size: "sm" })} h-7 px-2 text-xs`}
            onClick={() => onLocateMessage(active.eventId)}
            data-resource-locate
            data-lcd-key
          >
            {t("resourcePanel.locateMessage")}
          </button>
        ) : null}
        {/* A PDF can open in its own browser tab, except inside a Connect frame, where a new
            tab would leave the frame's authorization behind. */}
        {active.media === "pdf" && !readFrameProof(window.location) ? (
          <a
            href={attachmentLinks(active).previewHref}
            target="_blank"
            rel="noopener noreferrer"
            className={`${buttonVariants({ variant: "ghost", size: "icon" })} h-7 w-7`}
            title={t("workspaceOpenSeparate")}
            aria-label={t("workspaceOpenSeparate")}
            data-lcd-key
          >
            <LcdIcon icon={ExternalLink} glyph="external-link" className="h-4 w-4" />
          </a>
        ) : null}
        <AuthenticatedDownloadLink
          href={attachmentLinks(active).downloadHref}
          download={attachmentLinks(active).downloadName}
          className={`${buttonVariants({ variant: "ghost", size: "icon" })} h-7 w-7`}
          title={t("download", { name: active.title })}
          aria-label={t("download", { name: active.title })}
          data-lcd-key
        >
          <DownloadIcon size={16} aria-hidden="true" />
        </AuthenticatedDownloadLink>
      </div>
    ) : null;

  return (
    <div
      ref={root}
      className={classNames(
        "flex min-h-0 min-w-0 flex-col overflow-hidden bg-[var(--color-bg-primary)]",
        phone ? "flex-1" : maximized ? "absolute z-40 shadow-xl" : "absolute z-30",
        !phone && dock === "right" && !maximized && "border-l border-[var(--glass-border-subtle)]",
      )}
      hidden={!shown}
      data-resource-panel
      data-dock={phone ? "phone" : dock}
      data-maximized={maximized || undefined}
    >
      <div
        className="flex h-10 pointer-coarse:h-12 shrink-0 items-stretch border-b border-[var(--glass-border-subtle)]"
        // Esc belongs to the panel only from its own bar; editors and viewers keep theirs. It
        // leaves the side-by-side view first, then restores a maximized panel, then collapses it.
        onKeyDown={(event) => {
          if (event.key !== "Escape" || event.defaultPrevented) return;
          event.preventDefault();
          if (split) leaveSplit();
          else if (maximized) setMaximized(groupId, false);
          else collapse();
        }}
        data-resource-bar
      >
        {phone ? (
          <button
            type="button"
            // A key like the bar's others, the same height, not a taller segment.
            className={`${barKey} mx-1 self-center px-2`}
            onClick={collapse}
            title={t("resourcePanel.hide")}
            data-resource-collapse
            data-lcd-key
          >
            <LcdIcon icon={ChevronLeft} glyph="chevron-left" className="h-4 w-4" />
            {/* On the narrowest phones the arrow alone leaves the tabs room. */}
            <span className="max-[359px]:sr-only">{t("common:back")}</span>
          </button>
        ) : null}
        <div
          ref={strip}
          role="tablist"
          aria-label={t("resourcePanel.tabs")}
          className="flex min-w-0 flex-1 overflow-x-auto [scrollbar-width:none]"
          data-resource-tabs
        >
          {tabs.map((tab) => {
            const selected = tab.key === panel.activeKey;
            const state = editState(tab);
            return (
              <div
                key={tab.key}
                className={classNames(
                  "group/tab flex max-w-[12rem] min-w-[7.5rem] shrink-0 items-center gap-1.5 border-r border-[var(--glass-border-subtle)] pl-2.5 pr-1 text-sm",
                  selected
                    ? "bg-[var(--glass-tab-bg-active)] font-semibold text-[var(--color-text-primary)] shadow-[inset_0_-2px_0_var(--color-text-primary)]"
                    : "text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)]",
                )}
                title={tab.kind === "workspace" ? tab.path : tab.title}
                onAuxClick={(event) => {
                  if (event.button !== 1) return;
                  event.preventDefault();
                  close([tab.key]);
                }}
                onContextMenu={(event) => {
                  event.preventDefault();
                  const anchor =
                    event.currentTarget.querySelector<HTMLElement>('[role="tab"]') ??
                    event.currentTarget;
                  setMenu({ tab, x: event.clientX, y: event.clientY, anchor });
                }}
              >
                <button
                  type="button"
                  role="tab"
                  aria-selected={selected}
                  tabIndex={selected ? 0 : -1}
                  className="flex min-w-0 flex-1 items-center gap-1.5 self-stretch text-left"
                  onClick={() => select(tab)}
                  onKeyDown={(event) => {
                    if (event.key === "Delete") {
                      event.preventDefault();
                      close([tab.key]);
                      return;
                    }
                    if ((event.key === "F10" && event.shiftKey) || event.key === "ContextMenu") {
                      event.preventDefault();
                      const box = event.currentTarget.getBoundingClientRect();
                      setMenu({ tab, x: box.left, y: box.bottom, anchor: event.currentTarget });
                      return;
                    }
                    const index = tabs.indexOf(tab);
                    const target =
                      event.key === "ArrowRight"
                        ? tabs[index + 1]
                        : event.key === "ArrowLeft"
                          ? tabs[index - 1]
                          : event.key === "Home"
                            ? tabs[0]
                            : event.key === "End"
                              ? tabs[tabs.length - 1]
                              : undefined;
                    if (!target) return;
                    event.preventDefault();
                    select(target);
                    requestAnimationFrame(() =>
                      strip.current?.querySelector<HTMLElement>('[aria-selected="true"]')?.focus(),
                    );
                  }}
                  data-resource-tab={tab.key}
                >
                  <TabIcon tab={tab} />
                  <span className="truncate">{tab.title}</span>
                  {state.dirty || state.saving ? (
                    <span className="shrink-0 opacity-60" aria-label={t("resourcePanel.unsaved")}>
                      •
                    </span>
                  ) : null}
                </button>
                <button
                  type="button"
                  className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded opacity-60 hover:bg-[var(--glass-tab-bg)] hover:opacity-100"
                  aria-label={t("resourcePanel.closeTab", { name: tab.title })}
                  title={t("resourcePanel.closeTab", { name: tab.title })}
                  onClick={() => close([tab.key])}
                >
                  <CloseIcon size={14} />
                </button>
              </div>
            );
          })}
        </div>
        <div className="flex shrink-0 items-center gap-0.5 border-l border-[var(--glass-border-subtle)] px-1">
          <Popover open={listOpen} onOpenChange={setListOpen}>
            <PopoverAnchor asChild>
              <button
                ref={listAnchor}
                type="button"
                className={`${barKey} px-2`}
                onClick={() => setListOpen((value) => !value)}
                aria-expanded={listOpen}
                aria-haspopup="listbox"
                title={t("resourcePanel.tabs")}
                data-resource-all
                data-lcd-key
              >
                {t("resourcePanel.all", { open: tabs.length })}
                <LcdIcon icon={ChevronDown} glyph="chevron-down" className="h-3.5 w-3.5" />
              </button>
            </PopoverAnchor>
            <PopoverContent
              align="end"
              sideOffset={4}
              collisionPadding={8}
              className="rounded-xl p-0"
              data-lcd-menu
            >
              <ResourceTabsList
                tabs={tabs}
                activeKey={panel.activeKey}
                closed={panel.closed}
                onSelect={select}
                onClose={(tab) => close([tab.key])}
                onRestore={restoreEntry}
                onCloseOthers={() => active && close(keysExcept(active))}
                onCloseAll={() => close(allKeys())}
                onDone={() => setListOpen(false)}
              />
            </PopoverContent>
          </Popover>
          {tabs.length ? (
            <button
              type="button"
              className={barKey}
              aria-label={t(split ? "resourcePanel.unsplit" : "resourcePanel.split")}
              title={t(split ? "resourcePanel.unsplit" : "resourcePanel.split")}
              aria-pressed={!!split}
              onClick={toggleSplit}
              data-resource-split-toggle
              data-lcd-key
            >
              <LcdIcon icon={Columns2} glyph="columns-2" className="h-[18px] w-[18px]" />
            </button>
          ) : null}
          {phone ? null : (
            <>
              {/* One icon, set in reverse while maximized, so it never looks like Collapse. */}
              <button
                type="button"
                className={barKey}
                aria-label={t(maximized ? "resourcePanel.restore" : "resourcePanel.maximize")}
                title={t(maximized ? "resourcePanel.restore" : "resourcePanel.maximize")}
                aria-pressed={maximized}
                onClick={() => setMaximized(groupId, !maximized)}
                data-resource-maximize
                data-lcd-key
              >
                <LcdIcon icon={Maximize2} glyph="maximize" className="h-[18px] w-[18px]" />
              </button>
              {!maximized ? (
                <button
                  type="button"
                  className={barKey}
                  aria-label={t(
                    dock === "right" ? "resourcePanel.moveLeft" : "resourcePanel.moveRight",
                  )}
                  title={t(dock === "right" ? "resourcePanel.moveLeft" : "resourcePanel.moveRight")}
                  onClick={() => setDock(dock === "right" ? "left" : "right")}
                  data-resource-dock-toggle
                  data-lcd-key
                >
                  <LcdIcon
                    icon={dock === "right" ? SquareChevronLeft : SquareChevronRight}
                    glyph={dock === "right" ? "square-chevron-left" : "square-chevron-right"}
                    className="h-[18px] w-[18px]"
                  />
                </button>
              ) : null}
              {/* Collapse is one key on either side. While the panel covers the messages, it
                  shows how many arrived, as the terminal view's switch does. */}
              <button
                type="button"
                className={barKey}
                aria-label={collapseLabel}
                title={collapseLabel}
                onClick={collapse}
                data-resource-collapse
                data-lcd-key
              >
                <LcdIcon icon={Minimize2} glyph="minimize" className="h-[18px] w-[18px]" />
                {unreadShown ? (
                  <span aria-hidden="true" data-resource-unread>
                    {unread > 99 ? "99+" : unread}
                  </span>
                ) : null}
              </button>
            </>
          )}
        </div>
      </div>
      {pending ? (
        <div
          role="alertdialog"
          aria-label={t("resourcePanel.unsaved")}
          className="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-2 text-sm"
          data-resource-close-guard
        >
          <span className="min-w-0 flex-1">
            {pending.blocked.some((tab) => editState(tab).saving)
              ? t("resourcePanel.stillSaving", { names: names(pending.blocked) })
              : t("resourcePanel.unsavedFiles", { names: names(pending.blocked) })}
          </span>
          {pending.keys.length === 1 && pending.blocked.length === 1 ? (
            <>
              <button
                type="button"
                className={`${buttonVariants({ variant: "ghost", size: "sm" })} h-8`}
                disabled={
                  readOnly ||
                  files.saving ||
                  !(
                    pending.blocked[0].kind === "workspace" &&
                    sameScope(pending.blocked[0], files) &&
                    files.file?.path === pending.blocked[0].path
                  )
                }
                onClick={() => void savePending()}
              >
                {t("resourcePanel.saveAndClose")}
              </button>
              <button
                type="button"
                className={`${buttonVariants({ variant: "ghost", size: "sm" })} h-8`}
                disabled={editState(pending.blocked[0]).saving}
                onClick={discardPending}
              >
                {t("resourcePanel.discardAndClose")}
              </button>
            </>
          ) : pending.keys.length > pending.blocked.length ? (
            <button
              type="button"
              className={`${buttonVariants({ variant: "ghost", size: "sm" })} h-8`}
              onClick={closeClean}
              data-resource-close-clean
            >
              {t("resourcePanel.closeCleanOnly")}
            </button>
          ) : null}
          <button
            type="button"
            className={`${buttonVariants({ variant: "ghost", size: "sm" })} h-8`}
            onClick={() => setPending(null)}
          >
            {t("common:cancel")}
          </button>
        </div>
      ) : null}
      {split ? null : toolbar}
      <div
        ref={body}
        className="relative flex min-h-0 flex-1 flex-col"
        role="tabpanel"
        data-resource-split={split ? (phone ? "stacked" : "columns") : undefined}
      >
        {split
          ? (["left", "right"] as const).map((side) => {
              const tab = tabs.find((item) => item.key === split[side]);
              if (!tab) return null;
              const state = editState(tab);
              return (
                <SplitPaneHeader
                  key={`header:${side}`}
                  side={side}
                  stacked={phone}
                  tab={tab}
                  icon={<TabIcon tab={tab} />}
                  focused={split.focus === side}
                  unsaved={state.dirty || state.saving}
                  choices={tabs}
                  isDark={isDark}
                  style={splitBoxes(side, split.ratio, phone, splitHeader).header}
                  onFocus={() => focusSide(side)}
                  onPick={(choice) => {
                    setSplit(
                      (current) => current && { ...current, [side]: choice.key, focus: side },
                    );
                    select(choice);
                  }}
                />
              );
            })
          : null}
        {split ? (
          <SplitDivider
            ratio={split.ratio}
            stacked={phone}
            body={body}
            onRatio={(ratio) => setSplit((current) => current && { ...current, ratio })}
          />
        ) : null}
        {/* What only the split view needs: workspace files read on their own, a second copy of
            an attachment shown on both sides, and a note where the other side already shows a
            Presentation slot. */}
        {split
          ? (["left", "right"] as const).map((side) => {
              const tab = tabs.find((item) => item.key === split[side]);
              if (!tab) return null;
              const box = splitBoxes(side, split.ratio, phone, splitHeader).content;
              const both = split.left === split.right;
              const content =
                tab.kind === "workspace" ? (
                  !scopeReady ? (
                    workspaceLoading
                  ) : sameScope(tab, files) ? (
                    <WorkspaceResourceView
                      groupId={groupId}
                      resource={tab}
                      draft={files.getFileDraft(tab.path, tab.scopeKey, tab.scopeUrl)}
                      isDark={isDark}
                      // Following a link makes this side current, since a click inside a
                      // framed document does not reach the panel.
                      onOpenFile={(path, options) => {
                        focusSide(side);
                        if (path === tab.path && options?.fragment)
                          setSideNavigation({
                            navigation: { fragment: options.fragment },
                            side,
                            path,
                          });
                        else void files.openFile(path, options);
                      }}
                      navigation={
                        sideNavigation.side === side && sideNavigation.path === tab.path
                          ? sideNavigation.navigation
                          : null
                      }
                      visible={shown}
                    />
                  ) : (
                    <OtherScopeNotice
                      draft={files.getFileDraft(tab.path, tab.scopeKey, tab.scopeUrl)}
                    />
                  )
                ) : tab.kind === "attachment" && both && side === "right" ? (
                  // The tab's own view stays on the first side; this copy keeps its own page,
                  // zoom or position.
                  <AttachmentResourceView
                    media={tab.media}
                    href={attachmentLinks(tab).previewHref}
                    title={tab.title}
                    isSvg={tab.isSvg}
                    isDark={isDark}
                    active={false}
                    visible={shown}
                    resourceKey={`${tab.key}:right`}
                  />
                ) : tab.kind === "presentation" &&
                  (both
                    ? side !== split.focus
                    : !presentationViews.some((view) => view.key === tab.key)) ? (
                  <p className="p-4 text-sm text-[var(--color-text-secondary)]">
                    {t("resourcePanel.splitSameSlot")}
                  </p>
                ) : null;
              if (!content) return null;
              return (
                <div
                  key={`side:${side}:${tab.key}`}
                  className="absolute flex min-h-0 flex-col overflow-hidden"
                  style={box}
                  onPointerDownCapture={() => focusSide(side)}
                  onFocusCapture={() => focusSide(side)}
                  data-resource-split-side={side}
                >
                  {content}
                </div>
              );
            })
          : null}
        {!split && active?.kind === "workspace" && (!scopeReady || !sameScope(active, files)) ? (
          scopeReady ? (
            <OtherScopeNotice
              draft={files.getFileDraft(active.path, active.scopeKey, active.scopeUrl)}
            />
          ) : (
            workspaceLoading
          )
        ) : null}
        {!split && workspacePending ? (
          files.selectedPath === workspacePending.path && files.fileError ? (
            <div
              role="alert"
              className="flex flex-col items-start gap-3 p-4 text-sm"
              data-resource-workspace-error
            >
              <p>{files.fileError}</p>
              <button
                type="button"
                className={`${buttonVariants({ variant: "ghost", size: "sm" })} h-8`}
                onClick={() => void files.openFile(workspacePending.path)}
                data-lcd-key
              >
                {t("common:retry")}
              </button>
            </div>
          ) : (
            workspaceLoading
          )
        ) : null}
        {/* The editor's file stays mounted while another tab is in front, so coming back finds
            the same document and its anchor is not applied again. */}
        {workspaceShown || workspaceKept ? (
          <div className="flex min-h-0 flex-1 flex-col" hidden={!workspaceShown || !!split}>
            <WorkspaceViewer
              files={workspaceView}
              isDark={isDark}
              readOnly={readOnly}
              onAttach={onAttachPath}
              inPanel
              visible={shown && workspaceShown && !split}
            />
          </div>
        ) : null}
        {presentation
          ? presentationViews.map((tab) => {
              const side =
                split?.left === tab.key && split.right === tab.key
                  ? split.focus
                  : splitSide(tab.key);
              const front = split
                ? side !== null
                : active?.kind === "presentation" && active.slotId === tab.slotId;
              return (
                <div
                  key={`presentation:${tab.slotId}`}
                  className="absolute inset-0 flex min-h-0 flex-col"
                  style={
                    split && side
                      ? splitBoxes(side, split.ratio, phone, splitHeader).content
                      : undefined
                  }
                  hidden={!front}
                  onPointerDownCapture={side ? () => focusSide(side) : undefined}
                  onFocusCapture={side ? () => focusSide(side) : undefined}
                  data-resource-presentation={tab.slotId}
                >
                  <Suspense fallback={null}>
                    <PresentationResourceView
                      groupId={groupId}
                      slotId={tab.slotId}
                      presentation={presentation.data}
                      focusRef={tab.focusRef ?? null}
                      focusEventId={tab.focusEventId ?? null}
                      referenceKey={tab.refKey}
                      isDark={isDark}
                      readOnly={presentation.readOnly}
                      visible={shown && front}
                      handlers={{
                        ...presentation.handlers,
                        onOpenWindow: presentation.onOpenWindow
                          ? () => presentation.onOpenWindow?.(tab)
                          : undefined,
                      }}
                    />
                  </Suspense>
                </div>
              );
            })
          : null}
        {tabs.map((tab) => {
          if (tab.kind !== "attachment" || !mounted.has(tab.key)) return null;
          const side = splitSide(tab.key);
          const front = split ? side !== null : tab.key === panel.activeKey;
          return (
            <div
              key={tab.key}
              className="absolute inset-0 flex min-h-0 flex-col"
              style={
                split && side
                  ? splitBoxes(side, split.ratio, phone, splitHeader).content
                  : undefined
              }
              hidden={!front}
              onPointerDownCapture={side ? () => focusSide(side) : undefined}
              onFocusCapture={side ? () => focusSide(side) : undefined}
              data-resource-attachment={tab.key}
            >
              <AttachmentResourceView
                media={tab.media}
                href={attachmentLinks(tab).previewHref}
                title={tab.title}
                isSvg={tab.isSvg}
                isDark={isDark}
                active={shown && !split && tab.key === panel.activeKey}
                visible={shown && front}
                resourceKey={tab.key}
              />
            </div>
          );
        })}
      </div>
      {menu ? (
        <WorkspaceEntryMenu
          x={menu.x}
          y={menu.y}
          anchor={menu.anchor}
          label={t("resourcePanel.tabMenu", { name: menu.tab.title })}
          isDark={isDark}
          onClose={() => setMenu(null)}
          items={[
            {
              key: "close",
              label: t("resourcePanel.close"),
              onSelect: () => close([menu.tab.key]),
            },
            {
              key: "others",
              label: t("resourcePanel.closeOthers"),
              onSelect: () => close(keysExcept(menu.tab)),
              disabled: tabs.length < 2,
            },
            {
              key: "right",
              label: t("resourcePanel.closeRight"),
              onSelect: () => close(keysRightOf(menu.tab)),
              disabled: !keysRightOf(menu.tab).length,
            },
            { key: "all", label: t("resourcePanel.closeAll"), onSelect: () => close(allKeys()) },
          ]}
        />
      ) : null}
    </div>
  );
}
