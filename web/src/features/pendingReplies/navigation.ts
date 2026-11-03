import { useGroupStore } from "../../stores/useGroupStore";
import { useUIStore } from "../../stores/useUIStore";
import { requestWorkspaceNavigation } from "../../stores/workspaceNavigation";
import { loadPendingReplyReminders } from "../../utils/pendingReplyReminders";

function canOpenPendingReplies() {
  const ui = useUIStore.getState();
  return ui.showPendingPrompts && ui.showPendingReplyReminders && loadPendingReplyReminders();
}

export function openPendingReplies(
  groupId: string,
  onSelectGroup: (groupId: string) => void,
  onCloseSidebar: () => void,
) {
  if (!canOpenPendingReplies()) return;
  requestWorkspaceNavigation(() => {
    // The unsaved-changes confirmation can outlive a visibility change in another tab.
    if (!canOpenPendingReplies()) return;
    onSelectGroup(groupId);
    useGroupStore.getState().closeChatWindow(groupId);
    const ui = useUIStore.getState();
    ui.setChatSenderFilter(groupId, []);
    ui.setChatFilter(groupId, "pending_user");
    ui.setGroupWorkView(groupId, "messages");
    ui.setChatMobileSurface(groupId, "messages");
    ui.setActiveTab("chat");
    onCloseSidebar();
  });
}
