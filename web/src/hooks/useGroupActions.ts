// Group action helpers (start/stop/state).
import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useGroupStore, useUIStore } from "../stores";
import * as api from "../services/api";
import type { GroupControl } from "../utils/groupControls";
import { useShallow } from "zustand/react/shallow";

type GroupLifecycleState = "active" | "idle" | "paused";

export function useGroupActions() {
  const { t } = useTranslation("actors");
  const { selectedGroupId, groupDoc, groups, setGroupDoc, refreshGroups, refreshActors } =
    useGroupStore(
      useShallow((s) => ({
        selectedGroupId: s.selectedGroupId,
        groupDoc: s.groupDoc,
        groups: s.groups,
        setGroupDoc: s.setGroupDoc,
        refreshGroups: s.refreshGroups,
        refreshActors: s.refreshActors,
      })),
    );

  const { setBusy, showError } = useUIStore(
    useShallow((s) => ({ setBusy: s.setBusy, showError: s.showError })),
  );

  const runGroupRequest = useCallback(
    async (busyKey: string, request: () => ReturnType<typeof api.startGroup>) => {
      setBusy(busyKey);
      try {
        const resp = await request();
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        await refreshActors();
        await refreshGroups();
      } finally {
        setBusy("");
      }
    },
    [setBusy, showError, refreshActors, refreshGroups],
  );

  const setGroupStateFor = useCallback(
    async (groupId: string, s: GroupLifecycleState) => {
      const isSelected = groupId === selectedGroupId;
      setBusy(s === "active" ? "group-activate" : s === "paused" ? "group-pause" : "group-idle");
      try {
        const resp = await api.setGroupState(groupId, s);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        if (isSelected) {
          setGroupDoc(
            groupDoc
              ? {
                  ...groupDoc,
                  state: s,
                  runtime_status: {
                    runtime_running: groupDoc.runtime_status?.runtime_running ?? false,
                    running_actor_count: groupDoc.runtime_status?.running_actor_count ?? 0,
                    has_running_foreman: groupDoc.runtime_status?.has_running_foreman ?? false,
                    ...groupDoc.runtime_status,
                    lifecycle_state: s,
                  },
                }
              : null,
          );
        }
        // When resuming to active and no actors are running, also start
        // the group so processes get relaunched (not just the state flag).
        const known = isSelected ? groupDoc : groups.find((g) => g.group_id === groupId);
        if (s === "active" && known && !known.running) {
          const startResp = await api.startGroup(groupId);
          if (!startResp.ok) {
            showError(`${startResp.error.code}: ${startResp.error.message}`);
          }
          await refreshActors();
        }
        await refreshGroups();
      } finally {
        setBusy("");
      }
    },
    [
      selectedGroupId,
      groupDoc,
      groups,
      setBusy,
      showError,
      setGroupDoc,
      refreshGroups,
      refreshActors,
    ],
  );

  // Start group
  const handleStartGroup = useCallback(async () => {
    if (!selectedGroupId) return;
    await runGroupRequest("group-start", () => api.startGroup(selectedGroupId));
  }, [selectedGroupId, runGroupRequest]);

  // Stop group
  const handleStopGroup = useCallback(async () => {
    if (!selectedGroupId) return;
    await runGroupRequest("group-stop", () => api.stopGroup(selectedGroupId));
  }, [selectedGroupId, runGroupRequest]);

  // Set group state
  const handleSetGroupState = useCallback(
    async (s: GroupLifecycleState) => {
      if (!selectedGroupId) return;
      await setGroupStateFor(selectedGroupId, s);
    },
    [selectedGroupId, setGroupStateFor],
  );

  // Run control for any group, selected or not (sidebar group menu).
  const handleGroupControl = useCallback(
    async (groupId: string, control: GroupControl) => {
      const gid = groupId.trim();
      if (!gid) return;
      if (control === "launch") {
        await runGroupRequest("group-start", () => api.startGroup(gid));
      } else if (control === "stop") {
        await runGroupRequest("group-stop", () => api.stopGroup(gid));
      } else {
        await setGroupStateFor(gid, control === "pause" ? "paused" : "active");
      }
    },
    [runGroupRequest, setGroupStateFor],
  );

  // Delete a group after confirmation. Deleting the selected group empties the workbench.
  const handleDeleteGroup = useCallback(
    async (groupId: string) => {
      const gid = groupId.trim();
      if (!gid) return;
      const isSelected = gid === selectedGroupId;
      const name =
        (isSelected ? groupDoc?.title : groups.find((g) => g.group_id === gid)?.title) || gid;
      if (!window.confirm(t("deleteGroupConfirm", { name }))) return;
      setBusy("group-delete");
      try {
        const resp = await api.deleteGroup(gid);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
        if (isSelected) {
          const store = useGroupStore.getState();
          store.setSelectedGroupId("");
          store.setGroupDoc(null);
          store.setEvents([]);
          store.setActors([]);
          store.setGroupContext(null);
          store.setGroupSettings(null);
        }
        await refreshGroups();
      } finally {
        setBusy("");
      }
    },
    [selectedGroupId, groupDoc, groups, t, setBusy, showError, refreshGroups],
  );

  return {
    handleStartGroup,
    handleStopGroup,
    handleSetGroupState,
    handleGroupControl,
    handleDeleteGroup,
  };
}
