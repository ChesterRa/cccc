// Explicit Group targets keep sidebar actions independent of the current view.
import { useCallback, useMemo, useRef, useState } from "react";
import { useGroupStore, useUIStore } from "../stores";
import * as api from "../services/api";
import type { GroupRunAction, GroupRunControls, PendingGroupAction } from "../utils/groupControls";
import i18n from "../i18n";

export function useGroupActions() {
  const [pending, setPending] = useState<PendingGroupAction | null>(null);
  const pendingRef = useRef<PendingGroupAction | null>(null);
  const run = useCallback(async (groupId: string, action: GroupRunAction) => {
    if (!groupId || pendingRef.current) return;
    const operation = { groupId, action };
    pendingRef.current = operation;
    setPending(operation);
    const { setBusy, showError } = useUIStore.getState();
    const busyKey = `group-${action === "resume" ? "activate" : action}`;
    setBusy(busyKey);
    const title =
      useGroupStore.getState().groups.find((g) => g.group_id === groupId)?.title || groupId;
    try {
      let response =
        action === "start"
          ? await api.startGroup(groupId)
          : action === "stop"
            ? await api.stopGroup(groupId)
            : await api.setGroupState(groupId, action === "pause" ? "paused" : "active");
      // Resume delivery in existing sessions. Relaunch only when the authoritative
      // response says there are no running sessions, including background Groups.
      if (
        response.ok &&
        action === "resume" &&
        !(response.result.group.runtime_status?.runtime_running ?? response.result.group.running)
      ) {
        response = await api.startGroup(groupId);
      }
      if (!response.ok) showError(`${title}: ${response.error.message}`);
    } catch {
      showError(i18n.t("layout:groupRun.failed", { group: title }));
    } finally {
      // These store methods apply/cache by Group ID and guard navigation races.
      await useGroupStore.getState().refreshGroups();
      await useGroupStore.getState().refreshActors(groupId);
      pendingRef.current = null;
      setPending(null);
      if (useUIStore.getState().busy === busyKey) setBusy("");
    }
  }, []);
  const handleStartGroup = useCallback(
    () => run(useGroupStore.getState().selectedGroupId, "start"),
    [run],
  );
  const groupRunControls = useMemo<GroupRunControls>(() => ({ pending, run }), [pending, run]);
  return { handleStartGroup, groupRunControls };
}
