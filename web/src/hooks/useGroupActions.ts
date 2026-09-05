// Group action helpers (start/stop/state).
import { useCallback } from "react";
import { useGroupStore, useUIStore } from "../stores";
import * as api from "../services/api";
import { useShallow } from "zustand/react/shallow";

export function useGroupActions() {
  const { selectedGroupId, groupDoc, setGroupDoc, refreshGroups, refreshActors } = useGroupStore(
    useShallow((s) => ({
      selectedGroupId: s.selectedGroupId,
      groupDoc: s.groupDoc,
      setGroupDoc: s.setGroupDoc,
      refreshGroups: s.refreshGroups,
      refreshActors: s.refreshActors,
    })),
  );

  const { setBusy, showError } = useUIStore(
    useShallow((s) => ({ setBusy: s.setBusy, showError: s.showError })),
  );

  // Start group
  const handleStartGroup = useCallback(async () => {
    if (!selectedGroupId) return;
    setBusy("group-start");
    try {
      const resp = await api.startGroup(selectedGroupId);
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      await refreshActors();
      await refreshGroups();
    } finally {
      setBusy("");
    }
  }, [selectedGroupId, setBusy, showError, refreshActors, refreshGroups]);

  // Stop group
  const handleStopGroup = useCallback(async () => {
    if (!selectedGroupId) return;
    setBusy("group-stop");
    try {
      const resp = await api.stopGroup(selectedGroupId);
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      await refreshActors();
      await refreshGroups();
    } finally {
      setBusy("");
    }
  }, [selectedGroupId, setBusy, showError, refreshActors, refreshGroups]);

  // Set group state
  const handleSetGroupState = useCallback(
    async (s: "active" | "idle" | "paused") => {
      if (!selectedGroupId) return;
      setBusy(s === "active" ? "group-activate" : s === "paused" ? "group-pause" : "group-idle");
      try {
        const resp = await api.setGroupState(selectedGroupId, s);
        if (!resp.ok) {
          showError(`${resp.error.code}: ${resp.error.message}`);
          return;
        }
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
        // When resuming to active and no actors are running, also start
        // the group so processes get relaunched (not just the state flag).
        if (s === "active" && groupDoc && !groupDoc.running) {
          const startResp = await api.startGroup(selectedGroupId);
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
    [selectedGroupId, groupDoc, setBusy, showError, setGroupDoc, refreshGroups, refreshActors],
  );

  return { handleStartGroup, handleStopGroup, handleSetGroupState };
}
