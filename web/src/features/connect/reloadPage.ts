import { useComposerStore } from "../../stores/useComposerStore";
import { usePendingPageWork } from "../../stores/usePendingPageWork";
import { useWorkspaceNavigation } from "../../stores/workspaceNavigation";
import { CONNECT_CHANNEL, readFrameProof } from "./protocol";

// React's dirty-reporting effect may not have run when a lazy import fails in
// the same turn as an edit. Report the current stores before requesting reload.
export function reloadPage() {
  const proof = readFrameProof(window.location);
  if (!proof || window.parent === window) {
    window.location.reload();
    return;
  }
  const composer = useComposerStore.getState();
  const dirty =
    useWorkspaceNavigation.getState().owners.size > 0 ||
    usePendingPageWork.getState().owners.size > 0 ||
    Object.values(composer.failedSendsByGroup).some((items) => items.length > 0) ||
    Boolean(composer.composerText.trim()) ||
    composer.composerFiles.length > 0 ||
    Object.entries(composer.drafts).some(
      ([groupId, draft]) =>
        groupId !== composer.activeGroupId &&
        (Boolean(draft.composerText.trim()) || draft.composerFiles.length > 0),
    );
  const send = (message: Record<string, unknown>) =>
    window.parent.postMessage(
      { ...message, channel: CONNECT_CHANNEL, frame_id: proof.frame_id },
      proof.parent_origin,
    );
  send({ type: "workspace_dirty", dirty });
  send({ type: "reload" });
}
