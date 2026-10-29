import { requestWorkspaceNavigation, useWorkspaceNavigation } from "../stores/workspaceNavigation";
// useDeepLink - Handle deep links (?group=<id>&event=<event_id>)
// Extracts deep link parsing, application, and openMessageWindow function

import { useRef, useEffect, useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import type { GroupMeta } from "../types";
import { apiJson } from "../services/api/base";
import {
  isPushWorker,
  parsePushMessageTarget,
  type PushMessageTarget,
} from "../features/pushNotifications/navigation";

interface UseDeepLinkOptions {
  /** List of available groups */
  groups: GroupMeta[];
  /** Whether that list has been loaded, even an empty one. */
  groupsLoaded: boolean;
  /** Populate the list after resolving a notification received during startup. */
  setGroups?: (groups: GroupMeta[]) => void;
  /** Read the latest store state even before React has rendered it. */
  getGroupsSnapshot?: () => GroupMeta[];
  /** Currently selected group ID */
  selectedGroupId: string;
  /** Set selected group ID */
  setSelectedGroupId: (gid: string) => void;
  /** Set active tab */
  setActiveTab: (tab: string) => void;
  /** Open chat window for an event */
  openChatWindow: (groupId: string, eventId: string) => Promise<void>;
  /** Show error message */
  showError: (msg: string) => void;
}

interface UseDeepLinkResult {
  /** Open a message in the chat window (handles cross-group navigation) */
  openMessageWindow: (groupId: string, eventId: string) => void;
  /** Parse deep link from URL on mount (call in initial load effect) */
  parseUrlDeepLink: () => void;
}

export function useDeepLink({
  groups,
  groupsLoaded,
  setGroups,
  getGroupsSnapshot,
  selectedGroupId,
  setSelectedGroupId,
  setActiveTab,
  openChatWindow,
  showError,
}: UseDeepLinkOptions): UseDeepLinkResult {
  const { t } = useTranslation("chat");
  const urlParsedRef = useRef(false);
  const deepLinkRef = useRef<{ groupId: string; eventId: string } | null>(null);
  const lookupVersionRef = useRef(0);
  const groupsRef = useRef(groups);
  groupsRef.current = groups;
  const selectedGroupRef = useRef(selectedGroupId);
  selectedGroupRef.current = selectedGroupId;
  const [pushTarget, setPushTarget] = useState<PushMessageTarget | null>(null);
  const [urlLookupGroup, setUrlLookupGroup] = useState<string | null>(null);
  const pendingNavigation = useWorkspaceNavigation((state) => state.pending);

  // Parse URL params on mount
  const parseUrlDeepLink = useCallback(() => {
    lookupVersionRef.current += 1;
    const params = new URLSearchParams(window.location.search);
    const gid = String(params.get("group") || "").trim();
    const eid = String(params.get("event") || "").trim();
    deepLinkRef.current = gid ? { groupId: gid, eventId: eid } : null;
    urlParsedRef.current = true;
    setUrlLookupGroup(gid || null);
  }, []);

  // Apply deep link after groups are loaded
  useEffect(() => {
    const dl = deepLinkRef.current;
    if (!dl) return;
    const gid = String(dl.groupId || "").trim();
    const eid = String(dl.eventId || "").trim();
    if (!gid) {
      deepLinkRef.current = null;
      return;
    }
    const visibleGroups = getGroupsSnapshot?.() ?? groups;
    const exists = visibleGroups.some((g) => String(g.group_id || "") === gid);
    if (!exists) {
      // A nonempty list can still predate a newly created Group. The shared
      // lookup below checks the current visible list before calling it missing.
      if (!setGroups && visibleGroups.length > 0) {
        showError(t("notifications.groupNotFound", { groupId: gid }));
        deepLinkRef.current = null;
        setUrlLookupGroup(null);
      }
      return;
    }
    if (selectedGroupId !== gid) {
      setSelectedGroupId(gid);
      return;
    }

    setActiveTab("chat");
    if (eid) void openChatWindow(gid, eid);
    deepLinkRef.current = null;
    setUrlLookupGroup(null);
  }, [
    getGroupsSnapshot,
    groups,
    openChatWindow,
    selectedGroupId,
    setActiveTab,
    setGroups,
    setSelectedGroupId,
    showError,
    t,
  ]);

  // Once the incoming link is resolved, refresh should keep this tab's Group.
  useEffect(() => {
    if (!urlParsedRef.current || deepLinkRef.current) return;
    const gid = String(selectedGroupId || "").trim();
    if (!gid || !groups.some((group) => group.group_id === gid)) return;
    const url = new URL(window.location.href);
    if (url.searchParams.get("group") === gid) return;
    url.searchParams.set("group", gid);
    url.searchParams.delete("event");
    url.searchParams.delete("tab");
    window.history.replaceState(window.history.state, "", url.pathname + url.search + url.hash);
  }, [groups, selectedGroupId]);

  const navigateToMessage = useCallback(
    (gid: string, eid: string) => {
      lookupVersionRef.current += 1;
      // Update URL
      const url = new URL(window.location.href);
      url.searchParams.set("group", gid);
      url.searchParams.set("event", eid);
      url.searchParams.set("tab", "chat");
      window.history.replaceState({}, "", url.pathname + "?" + url.searchParams.toString());

      // If we're already in the target group, jump immediately
      if (selectedGroupRef.current === gid) {
        setActiveTab("chat");
        void openChatWindow(gid, eid);
        deepLinkRef.current = null;
        setUrlLookupGroup(null);
        return;
      }

      // Otherwise, queue a deep link and switch groups; the effect will open the window
      deepLinkRef.current = { groupId: gid, eventId: eid };
      setUrlLookupGroup(gid);
      setSelectedGroupId(gid);
    },
    [setActiveTab, openChatWindow, setSelectedGroupId],
  );

  // Open message window function
  const openMessageWindow = useCallback(
    (groupId: string, eventId: string) => {
      const gid = String(groupId || "").trim();
      const eid = String(eventId || "").trim();
      if (!gid || !eid) return;
      const navigate = () => navigateToMessage(gid, eid);
      if (selectedGroupId === gid) navigate();
      else requestWorkspaceNavigation(navigate);
    },
    [navigateToMessage, selectedGroupId],
  );

  useEffect(() => {
    const serviceWorker = navigator.serviceWorker;
    if (!serviceWorker) return;
    let latestClick = -Infinity;
    const onMessage = (event: MessageEvent) => {
      const target = parsePushMessageTarget(event.data);
      const port = event.ports[0];
      if (!target || !port || !isPushWorker(event.source)) return;
      // A click that arrives late, after a newer one, does not take its place.
      if (target.deadline <= latestClick) return;
      latestClick = target.deadline;
      // Keep the most recent explicit click until groups have loaded and an
      // earlier navigation confirmation, if any, has been resolved.
      lookupVersionRef.current += 1;
      deepLinkRef.current = null;
      setUrlLookupGroup(null);
      setPushTarget(target);
      port.postMessage({ handled: true });
    };
    serviceWorker.addEventListener("message", onMessage);
    return () => serviceWorker.removeEventListener("message", onMessage);
  }, []);

  useEffect(() => {
    const groupId = pushTarget?.groupId || urlLookupGroup;
    if (!groupId || !setGroups) return;
    const before = getGroupsSnapshot?.() ?? groupsRef.current;
    if (before.some((group) => group.group_id === groupId)) return;
    // A link waits for the list the page loads anyway: every reload carries
    // the open Group in its address. Only a list already loaded can be
    // missing a Group made since; a notification is checked either way.
    if (!pushTarget && !groupsLoaded) return;
    const unavailable = pushTarget
      ? t("notifications.groupsUnavailable")
      : t("notifications.groupsUnavailableForLink");
    let cancelled = false;
    const controller = new AbortController();
    const deadline = window.setTimeout(() => controller.abort(), 10000);
    const version = lookupVersionRef.current;
    // A newer list must win even if its React render has not happened yet.
    const changed = () =>
      lookupVersionRef.current !== version ||
      (getGroupsSnapshot?.() ?? groupsRef.current) !== before;
    const clearTarget = () => {
      if (pushTarget) setPushTarget(null);
      else {
        deepLinkRef.current = null;
        setUrlLookupGroup(null);
      }
    };
    void (async () => {
      try {
        const response = await apiJson<{ groups: GroupMeta[] }>("/api/v1/groups", {
          cache: "no-store",
          signal: controller.signal,
        });
        if (cancelled || changed()) return;
        if (!response.ok || !Array.isArray(response.result.groups)) {
          showError(unavailable);
          clearTarget();
          return;
        }
        if (!response.result.groups.some((group) => group.group_id === groupId)) {
          showError(t("notifications.groupNotFound", { groupId }));
          clearTarget();
          return;
        }
        setGroups(response.result.groups);
      } catch {
        if (!cancelled && !changed()) {
          showError(unavailable);
          clearTarget();
        }
      } finally {
        window.clearTimeout(deadline);
      }
    })();
    return () => {
      cancelled = true;
      window.clearTimeout(deadline);
      controller.abort();
    };
  }, [
    groups,
    groupsLoaded,
    pushTarget,
    urlLookupGroup,
    setGroups,
    getGroupsSnapshot,
    showError,
    t,
  ]);

  useEffect(() => {
    if (!pushTarget || groups.length === 0 || pendingNavigation) return;
    if (useWorkspaceNavigation.getState().pending) return;
    const groupExists = () =>
      (getGroupsSnapshot?.() ?? groupsRef.current).some(
        (group) => group.group_id === pushTarget.groupId,
      );
    if (!groupExists()) {
      if (!setGroups) {
        showError(t("notifications.groupNotFound", { groupId: pushTarget.groupId }));
        setPushTarget(null);
      }
      return;
    }
    requestWorkspaceNavigation(() => {
      // Group access can change while the discard dialog is open.
      if (!groupExists()) {
        showError(t("notifications.groupNotFound", { groupId: pushTarget.groupId }));
        return;
      }
      navigateToMessage(pushTarget.groupId, pushTarget.eventId);
    });
    setPushTarget(null);
  }, [
    getGroupsSnapshot,
    groups,
    navigateToMessage,
    pendingNavigation,
    pushTarget,
    setGroups,
    showError,
    t,
  ]);

  return { openMessageWindow, parseUrlDeepLink };
}
