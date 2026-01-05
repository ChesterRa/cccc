import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import { apiForm, apiJson } from "./api";
import { TabBar } from "./components/TabBar";
import { ContextModal } from "./components/ContextModal";
import { SettingsModal } from "./components/SettingsModal";
import { SearchModal } from "./components/SearchModal";
import { DropOverlay } from "./components/DropOverlay";
import { AppHeader } from "./components/layout/AppHeader";
import { GroupSidebar } from "./components/layout/GroupSidebar";
import { MobileMenuSheet } from "./components/layout/MobileMenuSheet";
import { AddActorModal } from "./components/modals/AddActorModal";
import { CreateGroupModal } from "./components/modals/CreateGroupModal";
import { EditActorModal } from "./components/modals/EditActorModal";
import { GroupEditModal } from "./components/modals/GroupEditModal";
import { InboxModal } from "./components/modals/InboxModal";
import { RecipientsModal } from "./components/modals/RecipientsModal";
import { useTheme } from "./hooks/useTheme";
import { ActorTab } from "./pages/ActorTab";
import { ChatTab } from "./pages/ChatTab";
import {
  GroupMeta,
  GroupDoc,
  LedgerEvent,
  Actor,
  RuntimeInfo,
  ReplyTarget,
  GroupContext,
  GroupSettings,
  DirItem,
  DirSuggestion,
  SupportedRuntime,
  SUPPORTED_RUNTIMES,
  RUNTIME_INFO,
} from "./types";

function isSupportedRuntime(rt: string): rt is SupportedRuntime {
  return (SUPPORTED_RUNTIMES as readonly string[]).includes(rt);
}

function getRecipientActorIdsForEvent(ev: LedgerEvent, actors: Actor[]): string[] {
  if (!actors.length) return [];
  const actorIds = actors.map((a) => String(a.id || "")).filter((id) => id);
  const actorIdSet = new Set(actorIds);

  const toRaw = (ev.data && typeof ev.data === "object" && Array.isArray((ev.data as any).to))
    ? (ev.data as any).to
    : [];
  const tokens = (toRaw as unknown[])
    .map((x) => String(x || "").trim())
    .filter((s) => s.length > 0);
  const tokenSet = new Set(tokens);

  const by = String(ev.by || "").trim();

  // Empty 'to' means broadcast for visibility; effective delivery is all actors (except sender).
  if (tokenSet.size === 0 || tokenSet.has("@all")) {
    return actorIds.filter((id) => id !== by);
  }

  const out = new Set<string>();
  for (const t of tokenSet) {
    if (t === "user" || t === "@user") continue;
    if (t === "@peers") {
      for (const a of actors) {
        if (a.role === "peer") out.add(String(a.id));
      }
      continue;
    }
    if (t === "@foreman") {
      for (const a of actors) {
        if (a.role === "foreman") out.add(String(a.id));
      }
      continue;
    }
    if (actorIdSet.has(t)) out.add(t);
  }

  out.delete(by);
  return Array.from(out);
}

function getProjectRoot(group: GroupDoc | null): string {
  if (!group) return "";
  const key = String(group.active_scope_key || "");
  if (!key) return "";
  const scopes = Array.isArray(group.scopes) ? group.scopes : [];
  const hit = scopes.find((s) => String(s.scope_key || "") === key);
  return String(hit?.url || "");
}
const MAX_UI_EVENTS = 800;
const WEB_MAX_FILE_MB = 20;
const WEB_MAX_FILE_BYTES = WEB_MAX_FILE_MB * 1024 * 1024;

export default function App() {
  // Theme
  const { theme, setTheme, isDark } = useTheme();

  // Core state
  const [groups, setGroups] = useState<GroupMeta[]>([]);
  const [selectedGroupId, setSelectedGroupId] = useState<string>("");
  const [groupDoc, setGroupDoc] = useState<GroupDoc | null>(null);
  const [events, setEvents] = useState<LedgerEvent[]>([]);
  const [actors, setActors] = useState<Actor[]>([]);
  const [groupContext, setGroupContext] = useState<GroupContext | null>(null);
  const [groupSettings, setGroupSettings] = useState<GroupSettings | null>(null);
  const [runtimes, setRuntimes] = useState<RuntimeInfo[]>([]);

  // Tab state
  const [activeTab, setActiveTab] = useState<string>("chat"); // "chat" or actor id

  // UI state
  const [busy, setBusy] = useState<string>("");
  const [errorMsg, setErrorMsg] = useState<string>("");
  const [isTransitioning, setIsTransitioning] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [termEpochByActor, setTermEpochByActor] = useState<Record<string, number>>({});
  const [showScrollButton, setShowScrollButton] = useState(false);
  const [chatUnreadCount, setChatUnreadCount] = useState(0);
  const [isSmallScreen, setIsSmallScreen] = useState(false);

  // Responsive hint for mobile-first copy (align with Tailwind `sm` breakpoint).
  useEffect(() => {
    const mq = window.matchMedia("(max-width: 639px)");
    const update = () => setIsSmallScreen(mq.matches);
    update();
    if (typeof mq.addEventListener === "function") {
      mq.addEventListener("change", update);
      return () => mq.removeEventListener("change", update);
    }
    // Safari < 14
    mq.addListener(update);
    return () => mq.removeListener(update);
  }, []);

  // Modal state
  const [showContextModal, setShowContextModal] = useState(false);
  const [showSettingsModal, setShowSettingsModal] = useState(false);
  const [showSearchModal, setShowSearchModal] = useState(false);
  const [showAddActor, setShowAddActor] = useState(false);
  const [showCreateGroup, setShowCreateGroup] = useState(false);
  const [showGroupEdit, setShowGroupEdit] = useState(false);
  const [inboxOpen, setInboxOpen] = useState(false);
  const [editingActor, setEditingActor] = useState<Actor | null>(null);
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);

  // Composer state
  const [composerText, setComposerText] = useState("");
  const [composerFiles, setComposerFiles] = useState<File[]>([]);
  const [toText, setToText] = useState("");
  const [replyTarget, setReplyTarget] = useState<ReplyTarget>(null);
  const [showMentionMenu, setShowMentionMenu] = useState(false);
  const [mentionFilter, setMentionFilter] = useState("");
  const [mentionSelectedIndex, setMentionSelectedIndex] = useState(0);
  const [dropOverlayOpen, setDropOverlayOpen] = useState(false);
  const [messageMetaEventId, setMessageMetaEventId] = useState<string | null>(null);

  // Add actor form state
  const [newActorId, setNewActorId] = useState("");
  const [newActorRole, setNewActorRole] = useState<"peer" | "foreman">("peer");
  const [newActorRuntime, setNewActorRuntime] = useState<SupportedRuntime>("codex");
  const [newActorCommand, setNewActorCommand] = useState("");
  const [showAdvancedActor, setShowAdvancedActor] = useState(false);
  const [addActorError, setAddActorError] = useState("");

  // Edit actor form state
  const [editActorRuntime, setEditActorRuntime] = useState<SupportedRuntime>("codex");
  const [editActorCommand, setEditActorCommand] = useState("");
  const [editActorTitle, setEditActorTitle] = useState("");

  // Group edit state
  const [editGroupTitle, setEditGroupTitle] = useState("");
  const [editGroupTopic, setEditGroupTopic] = useState("");

  // Create group state
  const [createGroupPath, setCreateGroupPath] = useState("");
  const [createGroupName, setCreateGroupName] = useState("");
  const [dirItems, setDirItems] = useState<DirItem[]>([]);
  const [dirSuggestions, setDirSuggestions] = useState<DirSuggestion[]>([]);
  const [currentDir, setCurrentDir] = useState("");
  const [parentDir, setParentDir] = useState<string | null>(null);
  const [showDirBrowser, setShowDirBrowser] = useState(false);

  // Inbox state
  const [inboxActorId, setInboxActorId] = useState("");
  const [inboxMessages, setInboxMessages] = useState<LedgerEvent[]>([]);

  // Refs
  const composerRef = useRef<HTMLTextAreaElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const errorTimeoutRef = useRef<number | null>(null);
  const eventSourceRef = useRef<EventSource | null>(null);
  const eventContainerRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const activeTabRef = useRef<string>("chat");
  const selectedGroupIdRef = useRef<string>("");
  const chatAtBottomRef = useRef<boolean>(true);
  const actorsRef = useRef<Actor[]>([]);
  const dragDepthRef = useRef<number>(0);
  const contextRefreshTimerRef = useRef<number | null>(null);
  const actorWarmupTimersRef = useRef<number[]>([]);
  const refreshGroupsInFlightRef = useRef<boolean>(false);
  const refreshGroupsQueuedRef = useRef<boolean>(false);
  const refreshActorsInFlightRef = useRef<Set<string>>(new Set());
  const refreshActorsQueuedRef = useRef<Set<string>>(new Set());
  const loadGroupSeqRef = useRef<number>(0);

  // Swipe gesture state
  const touchStartX = useRef<number>(0);
  const touchStartY = useRef<number>(0);

  // Show error with auto-dismiss
  const showError = useCallback((msg: string) => {
    if (errorTimeoutRef.current) window.clearTimeout(errorTimeoutRef.current);
    setErrorMsg(msg);
    errorTimeoutRef.current = window.setTimeout(() => {
      setErrorMsg("");
      errorTimeoutRef.current = null;
    }, 8000);
  }, []);

  const appendComposerFiles = useCallback(
    (incoming: File[]) => {
      const files = Array.from(incoming || []);
      if (files.length === 0) return;

      const tooLarge = files.filter((f) => f.size > WEB_MAX_FILE_BYTES);
      const ok = files.filter((f) => f.size <= WEB_MAX_FILE_BYTES);

      if (tooLarge.length > 0) {
        const names = tooLarge.slice(0, 3).map((f) => f.name || "file");
        const more = tooLarge.length > 3 ? ` (+${tooLarge.length - 3} more)` : "";
        showError(`File too large (> ${WEB_MAX_FILE_MB}MB): ${names.join(", ")}${more}`);
      }

      if (ok.length === 0) return;

      const keyOf = (f: File) => `${f.name}:${f.size}:${f.lastModified}`;
      setComposerFiles((prev) => {
        const next = prev.slice();
        const seen = new Set(next.map(keyOf));
        for (const f of ok) {
          const k = keyOf(f);
          if (seen.has(k)) continue;
          seen.add(k);
          next.push(f);
        }
        return next;
      });
    },
    [showError]
  );

  // Computed values
  const projectRoot = useMemo(() => getProjectRoot(groupDoc), [groupDoc]);
  const hasForeman = useMemo(() => actors.some((a) => a.role === "foreman"), [actors]);
  const selectedGroupMeta = useMemo(
    () => groups.find((g) => String(g.group_id || "") === selectedGroupId) || null,
    [groups, selectedGroupId]
  );
  const selectedGroupRunning = selectedGroupMeta?.running ?? false;

  const suggestedActorId = useMemo(() => {
    const prefix = newActorRuntime;
    const existing = new Set(actors.map((a) => String(a.id || "")));
    for (let i = 1; i <= 999; i++) {
      const candidate = `${prefix}-${i}`;
      if (!existing.has(candidate)) return candidate;
    }
    return `${prefix}-${Date.now()}`;
  }, [actors, newActorRuntime]);

  const canAddActor = useMemo(() => {
    if (busy === "actor-add") return false;
    const rtInfo = runtimes.find((r) => r.name === newActorRuntime);
    const available = rtInfo?.available ?? false;
    if (newActorRuntime === "custom" && !newActorCommand.trim()) return false;
    if (!available && !newActorCommand.trim()) return false;
    return true;
  }, [busy, newActorRuntime, newActorCommand, runtimes]);

  const addActorDisabledReason = useMemo(() => {
    if (busy === "actor-add") return "";
    const rtInfo = runtimes.find((r) => r.name === newActorRuntime);
    const available = rtInfo?.available ?? false;
    if (newActorRuntime === "custom" && !newActorCommand.trim()) {
      return "Custom runtime requires a command.";
    }
    if (!available && !newActorCommand.trim()) {
      return `${RUNTIME_INFO[newActorRuntime]?.label || newActorRuntime} is not installed. Install it, or set a command override.`;
    }
    return "";
  }, [busy, newActorRuntime, newActorCommand, runtimes]);

  const validRecipientSet = useMemo(() => {
    const out = new Set<string>(["@all", "@foreman", "@peers"]);
    for (const a of actors) {
      const id = String(a.id || "").trim();
      if (id) out.add(id);
    }
    return out;
  }, [actors]);

  const toTokens = useMemo(() => {
    const raw = toText.split(",").map((t) => t.trim()).filter((t) => t.length > 0);
    // 'user' is a system recipient token for agents; Web users shouldn't target it.
    const filtered = raw.filter((t) => t !== "user" && t !== "@user" && t !== "@");
    // Deduplicate while preserving order.
    const out: string[] = [];
    const seen = new Set<string>();
    for (const t of filtered) {
      if (!validRecipientSet.has(t)) continue;
      if (seen.has(t)) continue;
      seen.add(t);
      out.push(t);
    }
    return out;
  }, [toText, validRecipientSet]);

  // Keep composer recipients synced with the current actor list.
  useEffect(() => {
    if (!toText) return;
    const raw = toText.split(",").map((t) => t.trim()).filter((t) => t.length > 0);
    const filtered = raw.filter((t) => t !== "user" && t !== "@user" && t !== "@");
    const out: string[] = [];
    const seen = new Set<string>();
    for (const t of filtered) {
      if (!validRecipientSet.has(t)) continue;
      if (seen.has(t)) continue;
      seen.add(t);
      out.push(t);
    }
    const next = out.join(", ");
    if (next !== toText) setToText(next);
  }, [toText, validRecipientSet]);

  const mentionSuggestions = useMemo(() => {
    const base = ["@all", "@foreman", "@peers"];
    const actorIds = actors.map((a) => String(a.id || "")).filter((id) => id);
    const all = [...base, ...actorIds];
    if (!mentionFilter) return all;
    const lower = mentionFilter.toLowerCase();
    return all.filter((s) => s.toLowerCase().includes(lower));
  }, [actors, mentionFilter]);

  // Get all tabs for swipe navigation
  const allTabs = useMemo(() => {
    return ["chat", ...actors.map((a) => a.id)];
  }, [actors]);

  // API functions
  async function refreshGroups() {
    if (refreshGroupsInFlightRef.current) {
      refreshGroupsQueuedRef.current = true;
      return;
    }
    refreshGroupsInFlightRef.current = true;
    try {
      const resp = await apiJson<{ groups: GroupMeta[] }>("/api/v1/groups");
      if (resp.ok) {
        const next = resp.result.groups || [];
        setGroups(next);

        // Don't override user's selection on periodic refresh. Only auto-select when:
        // - nothing selected yet, or
        // - the selected group no longer exists (deleted elsewhere).
        const cur = selectedGroupIdRef.current;
        const curExists = !!cur && next.some((g) => String(g.group_id || "") === cur);
        if (!curExists && next.length > 0) {
          setSelectedGroupId(String(next[0].group_id || ""));
        }
      }
    } catch {
      // Ignore transient failures (daemon/web restarting); next poll will retry.
    } finally {
      refreshGroupsInFlightRef.current = false;
      if (refreshGroupsQueuedRef.current) {
        refreshGroupsQueuedRef.current = false;
        void refreshGroups();
      }
    }
  }

  async function fetchRuntimes() {
    const resp = await apiJson<{ runtimes: RuntimeInfo[]; available: string[] }>("/api/v1/runtimes");
    if (resp.ok) {
      setRuntimes(resp.result.runtimes || []);
      const availableNames = (resp.result.runtimes || []).filter((r) => r.available).map((r) => r.name);
      if (availableNames.length > 0 && !availableNames.includes(newActorRuntime)) {
        const pick = SUPPORTED_RUNTIMES.find((rt) => availableNames.includes(rt));
        if (pick) setNewActorRuntime(pick);
      }
    }
  }

  async function fetchDirSuggestions() {
    const resp = await apiJson<{ suggestions: DirSuggestion[] }>("/api/v1/fs/recent");
    if (resp.ok) setDirSuggestions(resp.result.suggestions || []);
  }

  async function fetchDirContents(path: string) {
    setShowDirBrowser(true);
    const resp = await apiJson<{ path: string; parent: string | null; items: DirItem[] }>(
      `/api/v1/fs/list?path=${encodeURIComponent(path)}`
    );
    if (resp.ok) {
      setDirItems(resp.result.items || []);
      setCurrentDir(resp.result.path || path);
      setParentDir(resp.result.parent || null);
    } else {
      showError(resp.error?.message || "Failed to list directory");
    }
  }

  async function fetchContext(groupId: string) {
    const resp = await apiJson<GroupContext>(`/api/v1/groups/${encodeURIComponent(groupId)}/context`);
    if (resp.ok && resp.result && typeof resp.result === "object") setGroupContext(resp.result);
    else setGroupContext(null);
  }

  async function fetchSettings(groupId: string) {
    const resp = await apiJson<{ settings: GroupSettings }>(`/api/v1/groups/${encodeURIComponent(groupId)}/settings`);
    if (resp.ok && resp.result.settings) setGroupSettings(resp.result.settings);
  }

  async function loadGroup(groupId: string) {
    const gid = String(groupId || "").trim();
    if (!gid) return;

    loadGroupSeqRef.current += 1;
    const seq = loadGroupSeqRef.current;

    const sleep = (ms: number) => new Promise<void>((r) => window.setTimeout(r, ms));
    const safe = async <T,>(p: Promise<any>) => {
      try {
        return await p;
      } catch {
        return { ok: false, error: { code: "network_error", message: "Network error" } } as any;
      }
    };

    setIsTransitioning(true);
    try {
      // Fade out to avoid intermediate empty states (jitter) while switching groups.
      await sleep(150);
      if (loadGroupSeqRef.current !== seq || selectedGroupIdRef.current !== gid) return;

      // Load in parallel; do not clear state first (prevents flicker on fast devices).
      const [show, tail, a, ctx, settings] = await Promise.all([
        safe(apiJson<{ group: GroupDoc }>(`/api/v1/groups/${encodeURIComponent(gid)}`)),
        safe(apiJson<{ events: LedgerEvent[] }>(
          `/api/v1/groups/${encodeURIComponent(gid)}/ledger/tail?lines=120&with_read_status=true`
        )),
        safe(apiJson<{ actors: Actor[] }>(`/api/v1/groups/${encodeURIComponent(gid)}/actors?include_unread=true`)),
        safe(apiJson<GroupContext>(`/api/v1/groups/${encodeURIComponent(gid)}/context`)),
        safe(apiJson<{ settings: GroupSettings }>(`/api/v1/groups/${encodeURIComponent(gid)}/settings`)),
      ]);

      if (loadGroupSeqRef.current !== seq || selectedGroupIdRef.current !== gid) return;

      setGroupDoc(show.ok ? show.result.group : null);
      setEvents(tail.ok ? (tail.result.events || []).filter((ev) => ev && (ev as any).kind !== "context.sync") : []);
      setActors(a.ok ? a.result.actors || [] : []);
      setGroupContext(ctx.ok ? ctx.result : null);
      setGroupSettings(settings.ok && settings.result.settings ? settings.result.settings : null);
      setErrorMsg("");
      setActiveTab("chat");

      // Let React flush layout before fading back in.
      await sleep(0);
    } finally {
      if (loadGroupSeqRef.current === seq) setIsTransitioning(false);
    }
  }

  function connectStream(groupId: string) {
    if (eventSourceRef.current) {
      eventSourceRef.current.close();
      eventSourceRef.current = null;
    }
    const es = new EventSource(`/api/v1/groups/${encodeURIComponent(groupId)}/ledger/stream`);
    es.addEventListener("ledger", (e) => {
      const msg = e as MessageEvent;
      try {
        const ev = JSON.parse(String(msg.data || "{}"));

        if (ev && typeof ev === "object" && ev.kind === "context.sync") {
          if (contextRefreshTimerRef.current) window.clearTimeout(contextRefreshTimerRef.current);
          contextRefreshTimerRef.current = window.setTimeout(() => {
            contextRefreshTimerRef.current = null;
            void fetchContext(groupId);
          }, 150);
          return;
        }

        // Real-time read status: apply chat.read without requiring a full refresh.
        if (ev && typeof ev === "object" && ev.kind === "chat.read") {
          const actorId = String(ev.data?.actor_id || "");
          const eventId = String(ev.data?.event_id || "");

          if (actorId && eventId) {
            setEvents((prev) => {
              const idx = prev.findIndex((x) => x.kind === "chat.message" && String(x.id || "") === eventId);
              if (idx < 0) return prev;

              const next = prev.slice();
              for (let i = 0; i <= idx; i++) {
                const m = next[i];
                if (!m || m.kind !== "chat.message") continue;

                const recipients = getRecipientActorIdsForEvent(m, actorsRef.current);
                if (!recipients.includes(actorId)) continue;

                const rs: Record<string, boolean> = (m._read_status && typeof m._read_status === "object") ? { ...m._read_status } : {};
                if (rs[actorId] === true) continue;
                rs[actorId] = true;
                next[i] = { ...m, _read_status: rs };
              }
              return next;
            });
          }

          void refreshActors(groupId);
          return;
        }

        // Initialize read-status keys for new messages so "○/✓" can update live.
        if (ev && typeof ev === "object" && ev.kind === "chat.message" && !ev._read_status) {
          const recipients = getRecipientActorIdsForEvent(ev, actorsRef.current);
          if (recipients.length > 0) {
            const rs: Record<string, boolean> = {};
            for (const id of recipients) rs[id] = false;
            ev._read_status = rs;
          }
        }

        setEvents((prev) => {
          const next = prev.concat([ev]);
          return next.length > MAX_UI_EVENTS ? next.slice(next.length - MAX_UI_EVENTS) : next;
        });
        if (ev && typeof ev === "object" && ev.kind === "chat.message") {
          const by = String(ev.by || "");
          if (by && by !== "user") {
            const chatActive = activeTabRef.current === "chat";
            const atBottom = chatAtBottomRef.current;
            if (!chatActive || !atBottom) {
              setChatUnreadCount((c) => c + 1);
            }
          }
        }
        const kind = String((ev as any).kind || "");
        if (
          kind === "chat.message" ||
          kind === "chat.read" ||
          kind.startsWith("actor.") ||
          kind === "group.start" ||
          kind === "group.stop" ||
          kind === "group.set_state"
        ) {
          void refreshActors(groupId);
        }
      } catch { /* ignore */ }
    });
    eventSourceRef.current = es;
  }

  async function refreshActors(groupId?: string) {
    const gid = String(groupId || selectedGroupIdRef.current || selectedGroupId || "").trim();
    if (!gid) return;
    if (refreshActorsInFlightRef.current.has(gid)) {
      refreshActorsQueuedRef.current.add(gid);
      return;
    }
    refreshActorsInFlightRef.current.add(gid);
    try {
      const a = await apiJson<{ actors: Actor[] }>(`/api/v1/groups/${encodeURIComponent(gid)}/actors?include_unread=true`);
      if (a.ok && selectedGroupIdRef.current === gid) setActors(a.result.actors || []);
    } catch {
      // Ignore transient failures (daemon/web restarting); subsequent refresh will retry.
    } finally {
      refreshActorsInFlightRef.current.delete(gid);
      if (refreshActorsQueuedRef.current.has(gid)) {
        refreshActorsQueuedRef.current.delete(gid);
        void refreshActors(gid);
      }
    }
  }

  function clearActorWarmupTimers() {
    for (const t of actorWarmupTimersRef.current) window.clearTimeout(t);
    actorWarmupTimersRef.current = [];
  }

  function scheduleActorWarmupRefresh(groupId: string) {
    const gid = String(groupId || "").trim();
    if (!gid) return;
    clearActorWarmupTimers();
    const delaysMs = [1000, 2500, 5000, 10000, 15000];
    for (const ms of delaysMs) {
      const t = window.setTimeout(() => {
        if (selectedGroupIdRef.current !== gid) return;
        void refreshActors(gid);
      }, ms);
      actorWarmupTimersRef.current.push(t);
    }
  }


  // Message functions
  function toggleRecipient(token: string) {
    const t = token.trim();
    if (!t) return;
    const cur = toTokens;
    const idx = cur.findIndex((x) => x === t);
    if (idx >= 0) {
      const next = cur.slice(0, idx).concat(cur.slice(idx + 1));
      setToText(next.join(", "));
    } else {
      setToText(cur.concat([t]).join(", "));
    }
  }

  async function sendMessage() {
    const txt = composerText.trim();
    if (!selectedGroupId) return;
    if (!txt && composerFiles.length === 0) return;
    setBusy("send");
    try {
      setErrorMsg("");
      const to = toTokens;
      let resp;
      if (replyTarget) {
        const replyBy = String(replyTarget.by || "").trim();
        const replyFallbackTo =
          replyBy && replyBy !== "user" && replyBy !== "unknown"
            ? [replyBy]
            : ["@all"];
        const replyTo = to.length ? to : replyFallbackTo;
        if (composerFiles.length > 0) {
          const form = new FormData();
          form.append("by", "user");
          form.append("text", txt);
          form.append("to_json", JSON.stringify(replyTo));
          form.append("reply_to", replyTarget.eventId);
          for (const f of composerFiles) form.append("files", f);
          resp = await apiForm(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/reply_upload`, form);
        } else {
          resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/reply`, {
            method: "POST",
            body: JSON.stringify({
              text: txt,
              by: "user",
              to: replyTo,
              reply_to: replyTarget.eventId,
            }),
          });
        }
      } else {
        if (composerFiles.length > 0) {
          const form = new FormData();
          form.append("by", "user");
          form.append("text", txt);
          form.append("to_json", JSON.stringify(to));
          form.append("path", "");
          for (const f of composerFiles) form.append("files", f);
          resp = await apiForm(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/send_upload`, form);
        } else {
          resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/send`, {
            method: "POST",
            body: JSON.stringify({ text: txt, by: "user", to, path: "" }),
          });
        }
      }
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      setComposerText("");
      setComposerFiles([]);
      if (fileInputRef.current) fileInputRef.current.value = "";
      setReplyTarget(null);
    } finally {
      setBusy("");
    }
  }

  function startReply(ev: LedgerEvent) {
    if (!ev.id || ev.kind !== "chat.message") return;
    const text = ev.data?.text ? String(ev.data.text) : "";
    setReplyTarget({
      eventId: String(ev.id),
      by: String(ev.by || "unknown"),
      text: text.slice(0, 100) + (text.length > 100 ? "..." : ""),
    });
  }

  // Actor functions
  async function addActor() {
    if (!selectedGroupId) return;
    const actorId = newActorId.trim();
    setBusy("actor-add");
    setAddActorError("");
    try {
      const resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/actors`, {
        method: "POST",
        body: JSON.stringify({
          actor_id: actorId,
          role: newActorRole,
          runner: "pty",
          runtime: newActorRuntime,
          command: newActorCommand,
          env: {},
          default_scope_key: "",
          by: "user",
        }),
      });
      if (!resp.ok) {
        setAddActorError(resp.error?.message || "Failed to add agent");
        return;
      }
      setShowAddActor(false);
      setNewActorId("");
      setNewActorCommand("");
      setNewActorRole("peer");
      setNewActorRuntime("codex");
      setShowAdvancedActor(false);
      setAddActorError("");
      await refreshActors();
    } finally {
      setBusy("");
    }
  }

  async function toggleActorEnabled(actor: Actor) {
    if (!selectedGroupId) return;
    const actorId = String(actor.id || "").trim();
    if (!actorId) return;
    const isRunning = actor.running ?? actor.enabled ?? false;
    setBusy(`actor-${isRunning ? "stop" : "start"}:${actorId}`);
    try {
      setErrorMsg("");
      const action = isRunning ? "stop" : "start";
      const resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/actors/${encodeURIComponent(actorId)}/${action}`, {
        method: "POST",
      });
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      await refreshActors();
    } finally {
      setBusy("");
    }
  }

  async function removeActor(actor: Actor) {
    if (!selectedGroupId) return;
    const actorId = String(actor.id || "").trim();
    if (!actorId) return;
    if (!window.confirm(`Remove actor ${actorId}?`)) return;
    setBusy(`actor-remove:${actorId}`);
    try {
      setErrorMsg("");
      const resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/actors/${encodeURIComponent(actorId)}?by=user`, {
        method: "DELETE",
      });
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      // If we're viewing this actor's tab, switch to chat
      if (activeTab === actorId) setActiveTab("chat");
      await refreshActors();
      await loadGroup(selectedGroupId);
    } finally {
      setBusy("");
    }
  }

  function openEditActor(actor: Actor) {
    const isRunning = actor.running ?? actor.enabled ?? false;
    if (isRunning) {
      showError("Stop the actor before editing. Use stop → edit → start workflow.");
      return;
    }
    setEditingActor(actor);
    const rtRaw = String(actor.runtime || "").trim();
    const rt: SupportedRuntime = isSupportedRuntime(rtRaw) ? rtRaw : "codex";
    setEditActorRuntime(rt);
    const cmd = Array.isArray(actor.command) ? actor.command.join(" ") : "";
    setEditActorCommand(cmd);
    setEditActorTitle(actor.title || "");
  }

  async function updateActor() {
    if (!selectedGroupId || !editingActor) return;
    const actorId = String(editingActor.id || "").trim();
    if (!actorId) return;
    setBusy("actor-update");
    try {
      setErrorMsg("");
      const resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/actors/${encodeURIComponent(actorId)}`, {
        method: "POST",
        body: JSON.stringify({
          runtime: editActorRuntime,
          command: editActorCommand.trim(),
          title: editActorTitle.trim(),
          by: "user",
        }),
      });
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      setEditingActor(null);
      await refreshActors();
    } finally {
      setBusy("");
    }
  }

  // Group functions
  async function startGroup() {
    if (!selectedGroupId) return;
    setBusy("group-start");
    try {
      setErrorMsg("");
      const resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/start?by=user`, { method: "POST" });
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      await refreshActors();
      await refreshGroups();
    } finally {
      setBusy("");
    }
  }

  async function stopGroup() {
    if (!selectedGroupId) return;
    setBusy("group-stop");
    try {
      setErrorMsg("");
      const resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/stop?by=user`, { method: "POST" });
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      await refreshActors();
      await refreshGroups();
    } finally {
      setBusy("");
    }
  }

  async function createGroup() {
    const path = createGroupPath.trim();
    if (!path) return;
    const dirName = path.split("/").filter(Boolean).pop() || "working-group";
    const title = createGroupName.trim() || dirName;
    setBusy("create");
    try {
      setErrorMsg("");
      const resp = await apiJson<{ group_id: string }>("/api/v1/groups", {
        method: "POST",
        body: JSON.stringify({ title, topic: "", by: "user" }),
      });
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      const groupId = resp.result.group_id;
      const attachResp = await apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/attach`, {
        method: "POST",
        body: JSON.stringify({ path, by: "user" }),
      });
      if (!attachResp.ok) {
        showError(`Created group but failed to attach: ${attachResp.error.message}`);
      }
      setCreateGroupPath("");
      setCreateGroupName("");
      setShowCreateGroup(false);
      await refreshGroups();
      setSelectedGroupId(groupId);
    } finally {
      setBusy("");
    }
  }

  async function updateGroup() {
    if (!selectedGroupId) return;
    setBusy("group-update");
    try {
      setErrorMsg("");
      const resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}`, {
        method: "PUT",
        body: JSON.stringify({ title: editGroupTitle.trim(), topic: editGroupTopic.trim(), by: "user" }),
      });
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      setShowGroupEdit(false);
      await refreshGroups();
      await loadGroup(selectedGroupId);
    } finally {
      setBusy("");
    }
  }

  async function deleteGroup() {
    if (!selectedGroupId) return;
    if (!window.confirm(`Delete group "${groupDoc?.title || selectedGroupId}"? This cannot be undone.`)) return;
    setBusy("group-delete");
    try {
      setErrorMsg("");
      const resp = await apiJson(
        `/api/v1/groups/${encodeURIComponent(selectedGroupId)}?confirm=${encodeURIComponent(selectedGroupId)}&by=user`,
        { method: "DELETE" }
      );
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      setSelectedGroupId("");
      setGroupDoc(null);
      setEvents([]);
      setActors([]);
      setGroupContext(null);
      setGroupSettings(null);
      await refreshGroups();
    } finally {
      setBusy("");
    }
  }

  // Context/Settings update functions
  async function updateVision(vision: string) {
    if (!selectedGroupId) return;
    setBusy("context-update");
    try {
      const resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/context`, {
        method: "POST",
        body: JSON.stringify({ ops: [{ op: "vision.update", vision }], by: "user" }),
      });
      if (!resp.ok) showError(`${resp.error.code}: ${resp.error.message}`);
      await fetchContext(selectedGroupId);
    } finally {
      setBusy("");
    }
  }

  async function updateSketch(sketch: string) {
    if (!selectedGroupId) return;
    setBusy("context-update");
    try {
      const resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/context`, {
        method: "POST",
        body: JSON.stringify({ ops: [{ op: "sketch.update", sketch }], by: "user" }),
      });
      if (!resp.ok) showError(`${resp.error.code}: ${resp.error.message}`);
      await fetchContext(selectedGroupId);
    } finally {
      setBusy("");
    }
  }

  async function updateSettings(settings: Partial<GroupSettings>) {
    if (!selectedGroupId) return;
    setBusy("settings-update");
    try {
      const resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/settings`, {
        method: "PUT",
        body: JSON.stringify({ ...settings, by: "user" }),
      });
      if (!resp.ok) showError(`${resp.error.code}: ${resp.error.message}`);
      await fetchSettings(selectedGroupId);
    } finally {
      setBusy("");
    }
  }

  async function setGroupState(state: "active" | "idle" | "paused") {
    if (!selectedGroupId) return;
    setBusy("group-state");
    const beforeScrollTop = eventContainerRef.current?.scrollTop ?? null;
    try {
      const resp = await apiJson(
        `/api/v1/groups/${encodeURIComponent(selectedGroupId)}/state?state=${encodeURIComponent(state)}&by=user`,
        { method: "POST" }
      );
      if (!resp.ok) showError(`${resp.error.code}: ${resp.error.message}`);
      else {
        setGroupDoc((prev) => prev ? { ...prev, state } : prev);
        await refreshGroups();
      }
    } finally {
      setBusy("");
      if (beforeScrollTop !== null && eventContainerRef.current) {
        eventContainerRef.current.scrollTop = beforeScrollTop;
      }
    }
  }

  // Inbox functions
  async function openInbox(actorId: string) {
    const aid = String(actorId || "").trim();
    if (!aid || !selectedGroupId) return;
    setBusy(`inbox:${aid}`);
    try {
      setErrorMsg("");
      setInboxActorId(aid);
      setInboxMessages([]);
      setInboxOpen(true);
      const resp = await apiJson<{ messages: LedgerEvent[] }>(
        `/api/v1/groups/${encodeURIComponent(selectedGroupId)}/inbox/${encodeURIComponent(aid)}?by=user&limit=200`
      );
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      setInboxMessages(resp.result.messages || []);
    } finally {
      setBusy("");
    }
  }

  async function markInboxAllRead() {
    if (!selectedGroupId || !inboxActorId) return;
    const last = inboxMessages.length ? inboxMessages[inboxMessages.length - 1] : null;
    const eventId = last?.id ? String(last.id) : "";
    if (!eventId) return;
    setBusy(`inbox-read:${inboxActorId}`);
    try {
      setErrorMsg("");
      const resp = await apiJson(`/api/v1/groups/${encodeURIComponent(selectedGroupId)}/inbox/${encodeURIComponent(inboxActorId)}/read`, {
        method: "POST",
        body: JSON.stringify({ event_id: eventId, by: "user" }),
      });
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      await openInbox(inboxActorId);
    } finally {
      setBusy("");
    }
  }


  // Effects
  useEffect(() => {
    refreshGroups();
    fetchRuntimes();
    fetchDirSuggestions();
    const t = window.setInterval(refreshGroups, 5000);
    return () => window.clearInterval(t);
  }, []);

  useEffect(() => {
    selectedGroupIdRef.current = selectedGroupId;
    // Group selection is the top-level routing context. Clear per-group composer state
    // to avoid sending to the wrong actors after switching groups.
    setComposerText("");
    setComposerFiles([]);
    setToText("");
    setReplyTarget(null);
    if (fileInputRef.current) fileInputRef.current.value = "";
    if (contextRefreshTimerRef.current) {
      window.clearTimeout(contextRefreshTimerRef.current);
      contextRefreshTimerRef.current = null;
    }
    clearActorWarmupTimers();
    dragDepthRef.current = 0;
    setDropOverlayOpen(false);
    if (!selectedGroupId) return;
    loadGroup(selectedGroupId);
    connectStream(selectedGroupId);
    scheduleActorWarmupRefresh(selectedGroupId);
    return () => {
      if (eventSourceRef.current) {
        eventSourceRef.current.close();
        eventSourceRef.current = null;
      }
      clearActorWarmupTimers();
    };
  }, [selectedGroupId]);

  useEffect(() => {
    const hasFiles = (e: DragEvent) => {
      const dt = e.dataTransfer;
      if (!dt) return false;
      try {
        if (dt.types && Array.from(dt.types).includes("Files")) return true;
        if (dt.items && Array.from(dt.items).some((it) => it.kind === "file")) return true;
      } catch {
        // ignore
      }
      return dt.files && dt.files.length > 0;
    };

    const onDragEnter = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      dragDepthRef.current += 1;
      setDropOverlayOpen(true);
    };

    const onDragOver = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
    };

    const onDragLeave = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      dragDepthRef.current = Math.max(0, dragDepthRef.current - 1);
      if (dragDepthRef.current === 0) setDropOverlayOpen(false);
    };

    const onDrop = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      dragDepthRef.current = 0;
      setDropOverlayOpen(false);

      const files = Array.from(e.dataTransfer?.files || []);
      if (files.length === 0) return;
      if (!selectedGroupId) {
        showError("Select a group to attach files.");
        return;
      }
      appendComposerFiles(files);
    };

    window.addEventListener("dragenter", onDragEnter, true);
    window.addEventListener("dragover", onDragOver, true);
    window.addEventListener("dragleave", onDragLeave, true);
    window.addEventListener("drop", onDrop, true);
    return () => {
      window.removeEventListener("dragenter", onDragEnter, true);
      window.removeEventListener("dragover", onDragOver, true);
      window.removeEventListener("dragleave", onDragLeave, true);
      window.removeEventListener("drop", onDrop, true);
    };
  }, [appendComposerFiles, selectedGroupId, showError]);

  useEffect(() => {
    activeTabRef.current = activeTab;
    if (activeTab !== "chat") return;
    const el = eventContainerRef.current;
    if (!el) return;
    const threshold = 100;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < threshold;
    chatAtBottomRef.current = atBottom;
    setShowScrollButton(!atBottom);
    if (atBottom) setChatUnreadCount(0);
  }, [activeTab]);

  useEffect(() => {
    actorsRef.current = actors;
  }, [actors]);

  useEffect(() => {
    if (newActorRuntime === "custom") setShowAdvancedActor(true);
  }, [newActorRuntime]);

  const scrollToBottom = () => {
    const container = eventContainerRef.current;
    if (container) {
      container.scrollTo({ top: container.scrollHeight, behavior: "smooth" });
    }
    chatAtBottomRef.current = true;
    setShowScrollButton(false);
    setChatUnreadCount(0);
  };

  // Swipe gesture handlers for mobile tab switching
  const handleTouchStart = (e: React.TouchEvent) => {
    touchStartX.current = e.touches[0].clientX;
    touchStartY.current = e.touches[0].clientY;
  };

  const handleTouchEnd = (e: React.TouchEvent) => {
    const touchEndX = e.changedTouches[0].clientX;
    const touchEndY = e.changedTouches[0].clientY;
    const deltaX = touchEndX - touchStartX.current;
    const deltaY = touchEndY - touchStartY.current;

    // Only trigger if horizontal swipe is dominant and significant
    if (Math.abs(deltaX) > 50 && Math.abs(deltaX) > Math.abs(deltaY) * 1.5) {
      const currentIndex = allTabs.indexOf(activeTab);
      if (deltaX > 0 && currentIndex > 0) {
        // Swipe right - go to previous tab
        setActiveTab(allTabs[currentIndex - 1]);
      } else if (deltaX < 0 && currentIndex < allTabs.length - 1) {
        // Swipe left - go to next tab
        setActiveTab(allTabs[currentIndex + 1]);
      }
    }
  };

  // Tab change handler
  const handleTabChange = (tab: string) => {
    setActiveTab(tab);
  };

  // Get current actor for agent tab
  const currentActor = useMemo(() => {
    if (activeTab === "chat") return null;
    return actors.find((a) => a.id === activeTab) || null;
  }, [activeTab, actors]);

  const messageMetaEvent = useMemo(() => {
    if (!messageMetaEventId) return null;
    return events.find((x) => x.kind === "chat.message" && String(x.id || "") === messageMetaEventId) || null;
  }, [events, messageMetaEventId]);

  const messageMeta = useMemo(() => {
    if (!messageMetaEvent) return null;
    const toRaw = (messageMetaEvent.data && typeof messageMetaEvent.data === "object" && Array.isArray((messageMetaEvent.data as any).to))
      ? (messageMetaEvent.data as any).to
      : [];
    const toTokens = (toRaw as unknown[])
      .map((x) => String(x || "").trim())
      .filter((s) => s.length > 0);
    const toLabel = toTokens.length > 0 ? toTokens.join(", ") : "@all";

    const rs = (messageMetaEvent._read_status && typeof messageMetaEvent._read_status === "object")
      ? messageMetaEvent._read_status
      : null;
    const recipientIds = rs ? Object.keys(rs) : getRecipientActorIdsForEvent(messageMetaEvent, actors);
    const recipientIdSet = new Set(recipientIds);
    const entries = actors
      .map((a) => String(a.id || ""))
      .filter((id) => id && recipientIdSet.has(id))
      .map((id) => [id, !!(rs && rs[id])] as const);

    return { toLabel, entries };
  }, [actors, messageMetaEvent]);

  useEffect(() => {
    if (!messageMetaEventId) return;
    if (messageMetaEvent) return;
    setMessageMetaEventId(null);
  }, [messageMetaEvent, messageMetaEventId]);

  // Open group edit modal
  function openGroupEdit() {
    if (!groupDoc) return;
    setEditGroupTitle(groupDoc.title || "");
    setEditGroupTopic(groupDoc.topic || "");
    setShowGroupEdit(true);
  }

  const chatMessages = events.filter((ev) => ev.kind === "chat.message");
  const needsScope = !!selectedGroupId && !projectRoot;
  const needsActors = !!selectedGroupId && actors.length === 0;
  const needsStart = !!selectedGroupId && actors.length > 0 && !selectedGroupRunning;
  const showSetupCard = needsScope || needsActors || needsStart;

  // Render
  return (
    <div
      className={`h-full w-full relative overflow-hidden ${
        isDark
          ? "bg-gradient-to-br from-slate-950 via-slate-900 to-slate-950"
          : "bg-gradient-to-br from-slate-50 via-white to-slate-100"
      }`}
    >
      {/* Liquid Glass Background - Animated gradient orbs */}
      <div className="pointer-events-none absolute inset-0 overflow-hidden">
        {/* Primary cyan orb */}
        <div
          className={`absolute -top-32 -left-32 w-96 h-96 rounded-full liquid-blob ${
            isDark
              ? "bg-gradient-to-br from-cyan-500/20 via-cyan-600/10 to-transparent"
              : "bg-gradient-to-br from-cyan-400/25 via-cyan-500/15 to-transparent"
          }`}
          style={{ filter: 'blur(60px)' }}
        />
        {/* Secondary purple orb */}
        <div
          className={`absolute top-1/4 -right-24 w-80 h-80 rounded-full liquid-blob ${
            isDark
              ? "bg-gradient-to-bl from-purple-500/15 via-indigo-600/10 to-transparent"
              : "bg-gradient-to-bl from-purple-400/20 via-indigo-500/10 to-transparent"
          }`}
          style={{ filter: 'blur(50px)', animationDelay: '-3s' }}
        />
        {/* Tertiary blue orb */}
        <div
          className={`absolute -bottom-20 left-1/3 w-72 h-72 rounded-full liquid-blob ${
            isDark
              ? "bg-gradient-to-tr from-blue-500/12 via-sky-600/8 to-transparent"
              : "bg-gradient-to-tr from-blue-400/15 via-sky-500/10 to-transparent"
          }`}
          style={{ filter: 'blur(45px)', animationDelay: '-5s' }}
        />
      </div>

      {/* Subtle noise texture overlay for glass depth */}
      <div
        className="pointer-events-none absolute inset-0 opacity-[0.015]"
        style={{
          backgroundImage: `url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='noise'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23noise)'/%3E%3C/svg%3E")`,
        }}
      />

      <div className="relative h-full grid grid-cols-1 md:grid-cols-[280px_1fr] transition-all duration-300">
        <GroupSidebar
          groups={groups}
          selectedGroupId={selectedGroupId}
          isOpen={sidebarOpen}
          isDark={isDark}
          onSelectGroup={(gid) => setSelectedGroupId(gid)}
          onCreateGroup={() => {
            setShowCreateGroup(true);
            fetchDirSuggestions();
          }}
          onClose={() => setSidebarOpen(false)}
        />

        {/* Main content */}
        <main className={`h-full flex flex-col overflow-hidden backdrop-blur-sm ${isDark ? "bg-slate-950/40" : "bg-white/60"}`}>
          <AppHeader
            isDark={isDark}
            theme={theme}
            onThemeChange={setTheme}
            selectedGroupId={selectedGroupId}
            groupDoc={groupDoc}
            selectedGroupRunning={selectedGroupRunning}
            actors={actors}
            busy={busy}
            errorMsg={errorMsg}
            onDismissError={() => setErrorMsg("")}
            onOpenSidebar={() => setSidebarOpen(true)}
            onOpenGroupEdit={openGroupEdit}
            onOpenContext={() => {
              if (selectedGroupId) void fetchContext(selectedGroupId);
              setShowContextModal(true);
            }}
            onStartGroup={startGroup}
            onStopGroup={stopGroup}
            onSetGroupState={setGroupState}
            onOpenSettings={() => setShowSettingsModal(true)}
            onOpenMobileMenu={() => setMobileMenuOpen(true)}
          />

          {/* Tab Bar */}
          {selectedGroupId && (
            <TabBar
              actors={actors}
              activeTab={activeTab}
              onTabChange={handleTabChange}
              unreadChatCount={chatUnreadCount}
              isDark={isDark}
              onAddAgent={() => {
                setNewActorRole(hasForeman ? "peer" : "foreman");
                setShowAddActor(true);
              }}
              canAddAgent={!!selectedGroupId}
            />
          )}

          {/* Tab Content */}
          <div
            ref={contentRef}
            className={`relative flex-1 min-h-0 flex flex-col overflow-hidden transition-opacity duration-150 ${
              isTransitioning ? "opacity-0" : "opacity-100"
            }`}
            onTouchStart={handleTouchStart}
            onTouchEnd={handleTouchEnd}
          >
            <div
              className={`absolute inset-0 flex min-h-0 flex-col ${activeTab === "chat" ? "" : "invisible pointer-events-none"}`}
              aria-hidden={activeTab !== "chat"}
            >
              <ChatTab
                isDark={isDark}
                isSmallScreen={isSmallScreen}
                selectedGroupId={selectedGroupId}
                actors={actors}
                busy={busy}
                showSetupCard={showSetupCard}
                needsScope={needsScope}
                needsActors={needsActors}
                needsStart={needsStart}
                hasForeman={hasForeman}
                onAddAgent={() => {
                  setNewActorRole(hasForeman ? "peer" : "foreman");
                  setShowAddActor(true);
                }}
                onStartGroup={startGroup}
                chatMessages={chatMessages}
                scrollRef={eventContainerRef}
                showScrollButton={showScrollButton}
                chatUnreadCount={chatUnreadCount}
                onScrollButtonClick={scrollToBottom}
                onScrollChange={(isAtBottom) => {
                  chatAtBottomRef.current = isAtBottom;
                  setShowScrollButton(!isAtBottom);
                  if (isAtBottom) setChatUnreadCount(0);
                }}
                onReply={startReply}
                onShowRecipients={(eventId) => setMessageMetaEventId(eventId)}
                replyTarget={replyTarget}
                onCancelReply={() => setReplyTarget(null)}
                toTokens={toTokens}
                onToggleRecipient={toggleRecipient}
                onClearRecipients={() => setToText("")}
                composerFiles={composerFiles}
                onRemoveComposerFile={(idx) => setComposerFiles((prev) => prev.filter((_, i) => i !== idx))}
                appendComposerFiles={appendComposerFiles}
                fileInputRef={fileInputRef}
                composerRef={composerRef}
                composerText={composerText}
                setComposerText={setComposerText}
                onSendMessage={sendMessage}
                showMentionMenu={showMentionMenu}
                setShowMentionMenu={setShowMentionMenu}
                mentionSuggestions={mentionSuggestions}
                mentionSelectedIndex={mentionSelectedIndex}
                setMentionSelectedIndex={setMentionSelectedIndex}
                setMentionFilter={setMentionFilter}
                onAppendRecipientToken={(token) => setToText((prev) => (prev ? prev + ", " + token : token))}
              />
            </div>
            {activeTab !== "chat" && (
              <div className="absolute inset-0 flex min-h-0 flex-col">
                <ActorTab
                  actor={currentActor}
                  groupId={selectedGroupId}
                  termEpoch={currentActor ? (termEpochByActor[currentActor.id] || 0) : 0}
                  busy={busy}
                  isDark={isDark}
                  onToggleEnabled={() => {
                    if (!currentActor) return;
                    toggleActorEnabled(currentActor);
                  }}
                  onRelaunch={() => {
                    void (async () => {
                      if (!selectedGroupId || !currentActor) return;
                      setBusy(`actor-relaunch:${currentActor.id}`);
                      try {
                        const resp = await apiJson(
                          `/api/v1/groups/${encodeURIComponent(selectedGroupId)}/actors/${encodeURIComponent(currentActor.id)}/restart?by=user`,
                          { method: "POST" }
                        );
                        if (!resp.ok) showError(`${resp.error.code}: ${resp.error.message}`);
                        await refreshActors();
                        setTermEpochByActor((prev) => ({
                          ...prev,
                          [currentActor.id]: (prev[currentActor.id] || 0) + 1,
                        }));
                      } finally {
                        setBusy("");
                      }
                    })();
                  }}
                  onEdit={() => {
                    if (!currentActor) return;
                    openEditActor(currentActor);
                  }}
                  onRemove={() => {
                    if (!currentActor) return;
                    removeActor(currentActor);
                  }}
                  onInbox={() => {
                    if (!currentActor) return;
                    openInbox(currentActor.id);
                  }}
                  onStatusChange={() => {
                    // Refresh actor list when component detects status change
                    void refreshActors();
                  }}
                />
              </div>
            )}
          </div>
        </main>
      </div>

      {/* Mobile menu (single entry point for actions) */}
      <MobileMenuSheet
        isOpen={mobileMenuOpen}
        isDark={isDark}
        selectedGroupId={selectedGroupId}
        groupDoc={groupDoc}
        selectedGroupRunning={selectedGroupRunning}
        actors={actors}
        busy={busy}
        onClose={() => setMobileMenuOpen(false)}
        onToggleTheme={() => setTheme(isDark ? "light" : "dark")}
        onOpenSearch={() => setShowSearchModal(true)}
        onOpenContext={() => {
          if (selectedGroupId) void fetchContext(selectedGroupId);
          setShowContextModal(true);
        }}
        onOpenSettings={() => setShowSettingsModal(true)}
        onOpenGroupEdit={openGroupEdit}
        onStartGroup={startGroup}
        onStopGroup={stopGroup}
        onSetGroupState={setGroupState}
      />

      <SearchModal
        isOpen={showSearchModal}
        onClose={() => setShowSearchModal(false)}
        groupId={selectedGroupId}
        actors={actors}
        isDark={isDark}
        onReply={(ev) => {
          startReply(ev);
          setActiveTab("chat");
          setShowSearchModal(false);
          window.setTimeout(() => composerRef.current?.focus(), 0);
        }}
      />

      <ContextModal
        isOpen={showContextModal}
        onClose={() => setShowContextModal(false)}
        groupId={selectedGroupId}
        context={groupContext}
        onUpdateVision={updateVision}
        onUpdateSketch={updateSketch}
        busy={busy === "context-update"}
        isDark={isDark}
      />

      <SettingsModal
        isOpen={showSettingsModal}
        onClose={() => setShowSettingsModal(false)}
        settings={groupSettings}
        onUpdateSettings={updateSettings}
        busy={busy.startsWith("settings")}
        isDark={isDark}
        groupId={selectedGroupId}
      />

      <RecipientsModal
        isOpen={!!messageMeta}
        isDark={isDark}
        isSmallScreen={isSmallScreen}
        toLabel={messageMeta?.toLabel || ""}
        entries={messageMeta?.entries || []}
        onClose={() => setMessageMetaEventId(null)}
      />

      <InboxModal
        isOpen={inboxOpen}
        isDark={isDark}
        actorId={inboxActorId}
        messages={inboxMessages}
        busy={busy}
        onClose={() => setInboxOpen(false)}
        onMarkAllRead={markInboxAllRead}
      />

      <GroupEditModal
        isOpen={showGroupEdit}
        isDark={isDark}
        busy={busy}
        title={editGroupTitle}
        topic={editGroupTopic}
        onChangeTitle={setEditGroupTitle}
        onChangeTopic={setEditGroupTopic}
        onSave={updateGroup}
        onCancel={() => setShowGroupEdit(false)}
        onDelete={deleteGroup}
      />

      <EditActorModal
        isOpen={!!editingActor}
        isDark={isDark}
        busy={busy}
        actorId={editingActor?.id || ""}
        runtimes={runtimes}
        runtime={editActorRuntime}
        onChangeRuntime={setEditActorRuntime}
        command={editActorCommand}
        onChangeCommand={setEditActorCommand}
        onSave={updateActor}
        onCancel={() => setEditingActor(null)}
      />

      <CreateGroupModal
        isOpen={showCreateGroup}
        isDark={isDark}
        busy={busy}
        dirSuggestions={dirSuggestions}
        dirItems={dirItems}
        currentDir={currentDir}
        parentDir={parentDir}
        showDirBrowser={showDirBrowser}
        createGroupPath={createGroupPath}
        setCreateGroupPath={setCreateGroupPath}
        createGroupName={createGroupName}
        setCreateGroupName={setCreateGroupName}
        onFetchDirContents={fetchDirContents}
        onCreateGroup={createGroup}
        onClose={() => setShowCreateGroup(false)}
        onCancelAndReset={() => {
          setShowCreateGroup(false);
          setCreateGroupPath("");
          setCreateGroupName("");
          setDirItems([]);
          setShowDirBrowser(false);
        }}
      />

      <AddActorModal
        isOpen={showAddActor}
        isDark={isDark}
        busy={busy}
        hasForeman={hasForeman}
        runtimes={runtimes}
        suggestedActorId={suggestedActorId}
        newActorId={newActorId}
        setNewActorId={setNewActorId}
        newActorRole={newActorRole}
        setNewActorRole={setNewActorRole}
        newActorRuntime={newActorRuntime}
        setNewActorRuntime={setNewActorRuntime}
        newActorCommand={newActorCommand}
        setNewActorCommand={setNewActorCommand}
        showAdvancedActor={showAdvancedActor}
        setShowAdvancedActor={setShowAdvancedActor}
        addActorError={addActorError}
        setAddActorError={setAddActorError}
        canAddActor={canAddActor}
        addActorDisabledReason={addActorDisabledReason}
        onAddActor={addActor}
        onClose={() => setShowAddActor(false)}
        onCancelAndReset={() => {
          setShowAddActor(false);
          setNewActorId("");
          setNewActorCommand("");
          setNewActorRole("peer");
          setShowAdvancedActor(false);
          setAddActorError("");
        }}
      />

      <DropOverlay isOpen={dropOverlayOpen} isDark={isDark} maxFileMb={WEB_MAX_FILE_MB} />

    </div >
  );
}
