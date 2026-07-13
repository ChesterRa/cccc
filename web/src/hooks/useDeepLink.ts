// useDeepLink - Handle deep links (?group=<id>&event=<event_id>)
// Extracts deep link parsing, application, and openMessageWindow function

import { useRef, useEffect, useCallback } from "react";
import type { GroupMeta } from "../types";

interface UseDeepLinkOptions {
  /** List of available groups */
  groups: GroupMeta[];
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
  selectedGroupId,
  setSelectedGroupId,
  setActiveTab,
  openChatWindow,
  showError,
}: UseDeepLinkOptions): UseDeepLinkResult {
  const deepLinkRef = useRef<{ groupId: string; eventId: string } | null>(null);

  // Parse URL params on mount
  const parseUrlDeepLink = useCallback(() => {
    const params = new URLSearchParams(window.location.search);
    const gid = String(params.get("group") || "").trim();
    const eid = String(params.get("event") || "").trim();
    if (gid && eid) {
      deepLinkRef.current = { groupId: gid, eventId: eid };
    }
  }, []);

  // Apply deep link after groups are loaded
  useEffect(() => {
    const dl = deepLinkRef.current;
    if (!dl) return;
    const gid = String(dl.groupId || "").trim();
    const eid = String(dl.eventId || "").trim();
    if (!gid || !eid) {
      deepLinkRef.current = null;
      return;
    }
    const exists = groups.some((g) => String(g.group_id || "") === gid);
    if (!exists) {
      if (groups.length > 0) {
        showError(`Group not found: ${gid}`);
        deepLinkRef.current = null;
      }
      return;
    }
    if (selectedGroupId !== gid) {
      setSelectedGroupId(gid);
      return;
    }

    setActiveTab("chat");
    void openChatWindow(gid, eid);
    deepLinkRef.current = null;
  }, [groups, openChatWindow, selectedGroupId, setActiveTab, setSelectedGroupId, showError]);

  // Open message window function
  const openMessageWindow = useCallback(
    (groupId: string, eventId: string) => {
      const gid = String(groupId || "").trim();
      const eid = String(eventId || "").trim();
      if (!gid || !eid) return;

      // Update URL
      const url = new URL(window.location.href);
      url.searchParams.set("group", gid);
      url.searchParams.set("event", eid);
      url.searchParams.set("tab", "chat");
      window.history.replaceState({}, "", url.pathname + "?" + url.searchParams.toString());

      // If we're already in the target group, jump immediately
      if (selectedGroupId === gid) {
        setActiveTab("chat");
        void openChatWindow(gid, eid);
        deepLinkRef.current = null;
        return;
      }

      // Otherwise, queue a deep link and switch groups; the effect will open the window
      deepLinkRef.current = { groupId: gid, eventId: eid };
      setSelectedGroupId(gid);
    },
    [selectedGroupId, setActiveTab, openChatWindow, setSelectedGroupId],
  );

  return { openMessageWindow, parseUrlDeepLink };
}
