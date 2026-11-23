import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Actor, LocalSessionSource } from "../../types";
import {
  createLocalConversation,
  joinLocalSessions,
  resumeLocalSessionAccess,
} from "../../services/api/localSessions";
import { fetchActors } from "../../services/api/actors";
import type { SessionNodeDevice } from "../../services/api/sessionNodes";
import { useGroupStore, useUIStore } from "../../stores";
import { isDeviceSessionSource } from "./remoteSessionPolicy";
import { useLocalSessionSources } from "./useLocalSessionSources";
import { useLocalSourceControls } from "./useLocalSourceControls";

export const MAX_SELECTED_SESSIONS = 16;

export function useLocalSessionsConnection({
  active,
  groupId,
  devices,
  onClose,
  onSelectGroup,
}: {
  active: boolean;
  groupId: string | null;
  devices?: readonly SessionNodeDevice[];
  onClose: () => void;
  onSelectGroup: (id: string) => void;
}) {
  const { t } = useTranslation("layout");
  const target = useGroupStore((state) => state.groups.find((group) => group.group_id === groupId));
  const [selected, setSelected] = useState<LocalSessionSource[]>([]);
  const [action, setAction] = useState<"submit" | "resume" | null>(null);
  const busy = action !== null;
  const [notice, setNotice] = useState("");
  const [membership, setMembership] = useState<{
    groupId: string;
    actors: Actor[];
    failed: boolean;
  } | null>(null);
  const membersRequest = useRef(0);
  const submitting = useRef(false);
  const alive = useRef(true);
  const targetUnavailable = groupId !== null && !target;
  const reading = active && !targetUnavailable;
  const directory = useLocalSessionSources({
    active: reading,
    includeDeviceSources: true,
    setSelected,
  });
  const management = useLocalSourceControls(directory, submitting);

  const loadMembers = useCallback(async (id: string) => {
    const request = ++membersRequest.current;
    try {
      const response = await fetchActors(id, false, { noCache: true });
      if (!alive.current || membersRequest.current !== request) return;
      const actors = response.ok ? response.result.actors : [];
      setMembership({ groupId: id, actors, failed: !response.ok });
      if (response.ok) {
        const joined = new Set(actors.map((actor) => actor.external_session_id));
        setSelected((previous) => previous.filter((source) => !joined.has(source.source_id)));
      }
    } catch {
      if (alive.current && membersRequest.current === request) {
        setMembership({ groupId: id, actors: [], failed: true });
      }
    }
  }, []);

  useEffect(() => {
    if (!reading || !groupId) return;
    setMembership(null);
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const load = async () => {
      await loadMembers(groupId);
      if (!cancelled) timer = setTimeout(() => void load(), 3000);
    };
    void load();
    return () => {
      cancelled = true;
      membersRequest.current += 1;
      clearTimeout(timer);
    };
  }, [groupId, reading, loadMembers]);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const currentMembership = membership?.groupId === groupId ? membership : null;
  const members = new Map(
    (currentMembership?.actors || [])
      .filter((actor) => actor.external_session_id)
      .map((actor) => [actor.external_session_id!, actor]),
  );
  const membersReady =
    groupId === null || (currentMembership !== null && !currentMembership.failed);
  const unavailableCount = selected.filter(
    (source) =>
      !source.connected ||
      (isDeviceSessionSource(source) &&
        devices &&
        !devices.some(
          (device) => device.node_id === source.node_id && device.enabled && device.connected,
        )),
  ).length;
  const hasDeviceSelection = selected.some(isDeviceSessionSource);
  const hasLocalSelection = selected.some((source) => !isDeviceSessionSource(source));
  const canSubmit =
    reading &&
    !busy &&
    !management.busy &&
    (!hasLocalSelection || (!directory.loading && !directory.error)) &&
    (!hasDeviceSelection || (!!devices && !directory.deviceLoading && !directory.deviceError)) &&
    membersReady &&
    selected.length > 0 &&
    selected.length <= MAX_SELECTED_SESSIONS &&
    unavailableCount === 0 &&
    !selected.some((source) => members.has(source.source_id));

  const start = async (title?: string) => {
    if (!canSubmit || submitting.current || management.pending.current) return;
    if (
      groupId !== null &&
      !useGroupStore.getState().groups.some((group) => group.group_id === groupId)
    ) {
      return;
    }
    submitting.current = true;
    setAction("submit");
    const selectedIds = selected.map((source) => source.source_id);
    try {
      const response = groupId
        ? await joinLocalSessions(groupId, selectedIds)
        : await createLocalConversation(selectedIds, title);
      if (!response.ok) {
        useUIStore.getState().showError(response.error.message);
        return;
      }
      // The write may finish after its dialog closes. Refresh state without navigating back.
      await useGroupStore.getState().refreshGroups();
      if (groupId) {
        const store = useGroupStore.getState();
        if (store.selectedGroupId === response.result.group_id) {
          store.setGroupDoc(response.result.group);
        }
        await store.refreshActors(response.result.group_id);
      }
      if (alive.current) {
        useUIStore
          .getState()
          .showNotice({
            message: t("localSessions.joinedGroupNotice", {
              group: response.result.group?.title || response.result.group_id,
            }),
          });
        onClose();
        onSelectGroup(response.result.group_id);
      }
    } catch (error) {
      useUIStore.getState().showError(error instanceof Error ? error.message : String(error));
    } finally {
      submitting.current = false;
      if (alive.current) setAction(null);
    }
  };

  const resume = async (actor: Actor, source: LocalSessionSource) => {
    if (
      !reading ||
      !groupId ||
      !source.connected ||
      submitting.current ||
      management.pending.current
    ) {
      return;
    }
    submitting.current = true;
    setAction("resume");
    setNotice("");
    try {
      const response = await resumeLocalSessionAccess(groupId, actor.id, source.source_id);
      if (!response.ok) useUIStore.getState().showError(response.error.message);
      else if (alive.current)
        setNotice(t("localSessions.accessRestored", { name: actor.title || actor.id }));
      if (alive.current) await loadMembers(groupId);
      await Promise.all([
        useGroupStore.getState().refreshActors(groupId),
        useGroupStore.getState().refreshGroups(),
      ]);
    } catch (error) {
      useUIStore.getState().showError(error instanceof Error ? error.message : String(error));
    } finally {
      submitting.current = false;
      if (alive.current) setAction(null);
    }
  };

  return {
    target,
    targetUnavailable,
    selected,
    setSelected,
    busy: busy || management.busy,
    admitting: action === "submit",
    directory,
    management,
    notice,
    membersReady,
    membersFailed: currentMembership?.failed === true,
    members,
    unavailableCount,
    canSubmit,
    start,
    resume,
  };
}
