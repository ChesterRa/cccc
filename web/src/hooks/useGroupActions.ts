// Explicit targets keep asynchronous Group actions independent of the current view.
import { useCallback, useRef } from "react";
import { useGroupStore, useUIStore } from "../stores";
import * as api from "../services/api";
import type { GroupControl } from "../utils/groupControls";
import i18n from "../i18n";
import { getCachedGroupActors } from "../stores/groupStoreCore";
import { formatDaemonError } from "../utils/daemonErrorText";
import { noticeGroupSessionsReplaced } from "./runtimeSessionReplacedNotice";

export function useGroupActions() {
  const pending = useRef(false);
  const run = useCallback(async (groupId: string, control: GroupControl | "delete") => {
    const gid = groupId.trim();
    if (!gid || pending.current) return;
    const store = useGroupStore.getState();
    const title = store.groups.find((group) => group.group_id === gid)?.title || gid;
    if (
      control === "delete" &&
      !window.confirm(i18n.t("actors:deleteGroupConfirm", { name: title }))
    )
      return;
    pending.current = true;
    const { setBusy, showError } = useUIStore.getState();
    const busyKey = `group-${control === "launch" ? "start" : control}`;
    setBusy(busyKey);
    try {
      if (control === "delete") {
        const response = await api.deleteGroup(gid);
        if (!response.ok) showError(`${title}: ${formatDaemonError(response.error)}`);
        else if (useGroupStore.getState().selectedGroupId === gid) {
          // This setter already clears the selected view and handles its draft.
          useGroupStore.getState().setSelectedGroupId("");
        }
      } else {
        let response =
          control === "launch"
            ? await api.startGroup(gid)
            : control === "stop"
              ? await api.stopGroup(gid)
              : await api.setGroupState(gid, control === "pause" ? "paused" : "active");
        if (
          response.ok &&
          control === "activate" &&
          // An unobservable runtime is not a stopped one: never start agents on a guess.
          response.result.group.runtime_status?.available !== false &&
          (response.result.group.runtime_status?.runtime_running ??
            response.result.group.running) === false
        ) {
          response = await api.startGroup(gid);
        }
        if (!response.ok) showError(`${title}: ${formatDaemonError(response.error)}`);
        else if (control === "launch" || control === "activate") {
          const current = useGroupStore.getState();
          const actors =
            current.selectedGroupId === gid ? current.actors : getCachedGroupActors(gid);
          noticeGroupSessionsReplaced(
            (actorId) => actors.find((actor) => actor.id === actorId)?.title || actorId,
            response.result,
          );
        }
      }
    } catch {
      showError(i18n.t("layout:groupRun.failed", { group: title }));
    } finally {
      try {
        // These store methods refresh by Group ID and guard navigation races.
        await useGroupStore.getState().refreshGroups();
        if (control !== "delete") await useGroupStore.getState().refreshActors(gid);
      } finally {
        pending.current = false;
        if (useUIStore.getState().busy === busyKey) setBusy("");
      }
    }
  }, []);
  const handleGroupControl = useCallback(
    (groupId: string, control: GroupControl) => run(groupId, control),
    [run],
  );
  const handleStartGroup = useCallback(
    () => run(useGroupStore.getState().selectedGroupId, "launch"),
    [run],
  );
  const handleDeleteGroup = useCallback((groupId: string) => run(groupId, "delete"), [run]);
  return { handleStartGroup, handleGroupControl, handleDeleteGroup };
}
