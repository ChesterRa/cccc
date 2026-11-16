import { useLocalSessionSources } from "./useLocalSessionSources";
import { useLocalSourceControls } from "./useLocalSourceControls";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Actor, LocalSessionSource } from "../../types";
import {
  createLocalConversation,
  joinLocalSessions,
  resumeLocalSessionAccess,
} from "../../services/api/localSessions";
import { fetchActors } from "../../services/api/actors";
import { useGroupStore, useUIStore } from "../../stores";

export const MAX_SELECTED_SESSIONS = 16;

export function useLocalSessionsConnection({
  onClose,
  onSelectGroup,
  initialTargetGroupId = "",
  reviewing = true,
}: {
  onClose: () => void;
  onSelectGroup: (id: string) => void;
  initialTargetGroupId?: string;
  reviewing?: boolean;
}) {
  const { t } = useTranslation("layout");
  const groups = useGroupStore((state) => state.groups);
  const [targetGroupId, setTargetGroupId] = useState(initialTargetGroupId);
  const [selected, setSelected] = useState<LocalSessionSource[]>([]);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState("");
  const [membership, setMembership] = useState<{
    groupId: string;
    actors: Actor[];
    loaded: boolean;
    failed: boolean;
  }>({ groupId: "", actors: [], loaded: false, failed: false });
  const targetRef = useRef(targetGroupId);
  targetRef.current = targetGroupId;
  const membersRequest = useRef(0);
  const submitting = useRef(false);
  const alive = useRef(true);
  const loadMembers = useCallback(async (groupId: string) => {
    const request = ++membersRequest.current;
    try {
      const response = await fetchActors(groupId, false, { noCache: true });
      if (!alive.current || targetRef.current !== groupId || membersRequest.current !== request)
        return;
      const actors = response.ok ? response.result.actors : [];
      setMembership({ groupId, actors, loaded: true, failed: !response.ok });
      if (response.ok)
        setSelected((previous) =>
          previous.filter(
            (source) => !actors.some((actor) => actor.external_session_id === source.source_id),
          ),
        );
    } catch {
      if (alive.current && targetRef.current === groupId && membersRequest.current === request) {
        setMembership({ groupId, actors: [], loaded: true, failed: true });
      }
    }
  }, []);
  useEffect(() => {
    setNotice("");
    if (!targetGroupId || !reviewing) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    setMembership({ groupId: targetGroupId, actors: [], loaded: false, failed: false });
    const load = async () => {
      await loadMembers(targetGroupId);
      if (!cancelled) timer = setTimeout(() => void load(), 3000);
    };
    void load();
    return () => {
      cancelled = true;
      membersRequest.current += 1;
      clearTimeout(timer);
    };
  }, [targetGroupId, loadMembers, reviewing]);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  const directory = useLocalSessionSources(setSelected);
  const management = useLocalSourceControls(directory, submitting);
  const { sources, loading, error } = directory;
  const selectedIds = selected.map((source) => source.source_id);
  const unavailableCount = selected.filter((source) => !source.connected).length;
  const displayedSources = [
    ...sources,
    ...selected.filter((source) => !sources.some((item) => item.source_id === source.source_id)),
  ];
  const anyUnavailable = displayedSources.some((source) => !source.connected);
  const firstSelected = selected[0];
  const joining = !!targetGroupId;
  const membersReady =
    !joining || (membership.groupId === targetGroupId && membership.loaded && !membership.failed);
  const members = new Map(
    (joining && membership.groupId === targetGroupId ? membership.actors : [])
      .filter((actor) => actor.external_session_id)
      .map((actor) => [actor.external_session_id!, actor]),
  );
  const targetUnavailable = joining && !groups.some((group) => group.group_id === targetGroupId);
  const canSubmit =
    reviewing &&
    !busy &&
    !management.busy &&
    !loading &&
    !error &&
    !targetUnavailable &&
    membersReady &&
    selected.length > 0 &&
    selected.length <= MAX_SELECTED_SESSIONS &&
    unavailableCount === 0 &&
    !selected.some((source) => members.has(source.source_id));
  const start = async () => {
    if (!canSubmit || submitting.current || management.pending.current) return;
    submitting.current = true;
    setBusy(true);
    try {
      const response = joining
        ? await joinLocalSessions(targetGroupId, selectedIds)
        : await createLocalConversation(selectedIds);
      if (!response.ok) {
        useUIStore.getState().showError(response.error.message);
        return;
      }
      // A completed user mutation still refreshes Groups if the dialog was closed.
      await useGroupStore.getState().refreshGroups();
      if (joining) {
        const store = useGroupStore.getState();
        if (store.selectedGroupId === response.result.group_id) {
          store.setGroupDoc(response.result.group);
        }
        // Re-selecting the current Group does not reload its actors.
        await store.refreshActors(response.result.group_id);
      }
      if (alive.current) {
        onSelectGroup(response.result.group_id);
        onClose();
      }
    } catch (error) {
      useUIStore.getState().showError(error instanceof Error ? error.message : String(error));
    } finally {
      submitting.current = false;
      if (alive.current) setBusy(false);
    }
  };
  const resume = async (actor: Actor, source: LocalSessionSource) => {
    if (busy || submitting.current || management.pending.current || !targetGroupId) return;
    const groupId = targetGroupId;
    submitting.current = true;
    setBusy(true);
    setNotice("");
    try {
      const response = await resumeLocalSessionAccess(groupId, actor.id, source.source_id);
      if (!response.ok) useUIStore.getState().showError(response.error.message);
      else if (alive.current && targetRef.current === groupId)
        setNotice(t("localSessions.accessRestored", { name: actor.title || actor.id }));
      await loadMembers(groupId);
      await Promise.all([
        useGroupStore.getState().refreshActors(groupId),
        useGroupStore.getState().refreshGroups(),
      ]);
    } catch (error) {
      useUIStore.getState().showError(error instanceof Error ? error.message : String(error));
    } finally {
      submitting.current = false;
      if (alive.current) setBusy(false);
    }
  };
  return {
    groups,
    targetGroupId,
    setTargetGroupId,
    selected,
    setSelected,
    selectedIds,
    displayedSources,
    loading,
    error,
    busy: busy || management.busy,
    directory,
    management,
    notice,
    membership,
    membersReady,
    members,
    targetUnavailable,
    unavailableCount,
    anyUnavailable,
    firstSelected,
    joining,
    canSubmit,
    start,
    resume,
  };
}
