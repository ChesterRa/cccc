import { useComposerStore } from "../../stores/useComposerStore";
import { usePendingPageWork } from "../../stores/usePendingPageWork";
import { useWorkspaceNavigation } from "../../stores/workspaceNavigation";
import { claimVitePreloadReload } from "../../utils/vitePreloadRecovery";
import { CONNECT_CHANNEL, readFrameProof } from "./protocol";
import { hasUnsentComposerWork } from "./composerWork";

/** Work a reload would discard: unsaved workspace edits, other pending page work or an unsent draft. */
export function hasUnsavedPageWork(): boolean {
  return (
    useWorkspaceNavigation.getState().owners.size > 0 ||
    usePendingPageWork.getState().owners.size > 0 ||
    hasUnsentComposerWork(useComposerStore.getState())
  );
}

/**
 * The reload CCCC starts by itself to load a newer build after one of its modules failed to load,
 * at most once per cooldown. It waits while the page holds work a reload would discard: the part
 * that failed shows its own error, and the user reloads after saving. Returns whether it reloads.
 */
export function reloadForNewBuild(storage: Storage, now: number = Date.now()): boolean {
  if (hasUnsavedPageWork() || !claimVitePreloadReload(storage, now)) return false;
  reloadPage();
  return true;
}

// React's dirty-reporting effect may not have run when a lazy import fails in
// the same turn as an edit. Report the current stores before requesting reload.
export function reloadPage() {
  const proof = readFrameProof(window.location);
  if (!proof || window.parent === window) {
    window.location.reload();
    return;
  }
  const dirty = hasUnsavedPageWork();
  const send = (message: Record<string, unknown>) =>
    window.parent.postMessage(
      { ...message, channel: CONNECT_CHANNEL, frame_id: proof.frame_id },
      proof.parent_origin,
    );
  send({ type: "workspace_dirty", dirty });
  send({ type: "reload" });
}
