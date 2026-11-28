import { useEffect } from "react";
import { create } from "zustand";
import { clearOpenedMessageFromUrl } from "../../hooks/chat/chatTabBasics";
import { useGroupStore } from "../../stores/useGroupStore";
import { useResourcePanelStore } from "../../stores/useResourcePanelStore";
import { groupMessagesVisible, useUIStore } from "../../stores/useUIStore";

export type TaskView = { groupId: string; taskId: string; title: string };

/**
 * The task whose conversation the chat shows instead of the full one. It is a view, not a
 * preference: it is not stored, and it applies only to the Group it was opened in.
 */
interface TaskViewState {
  view: TaskView | null;
  open: (groupId: string, taskId: string, title?: string) => void;
  close: () => void;
}

export const useTaskViewStore = create<TaskViewState>((set) => ({
  view: null,
  open: (groupId, taskId, title = "") => {
    const group = String(groupId || "").trim();
    const task = String(taskId || "").trim();
    if (group && task) set({ view: { groupId: group, taskId: task, title: title.trim() } });
  },
  close: () => set({ view: null }),
}));

/** The task view open in this Group, or null. */
export function useTaskView(groupId: string): TaskView | null {
  return useTaskViewStore((state) => (state.view?.groupId === groupId ? state.view : null));
}

export function getTaskView(groupId: string): TaskView | null {
  const view = useTaskViewStore.getState().view;
  return view?.groupId === groupId ? view : null;
}

/** The composer only inherits the task while its conversation is on screen. */
export function getDisplayedTaskView(groupId: string): TaskView | null {
  const groups = useGroupStore.getState();
  const ui = useUIStore.getState();
  if (
    groups.selectedGroupId !== groupId ||
    ui.webReadOnly ||
    groups.chatByGroup[groupId]?.chatWindow?.groupId === groupId ||
    !groupMessagesVisible(groupId, ui)
  ) {
    return null;
  }
  return getTaskView(groupId);
}

/** Both entry points leave other chat surfaces before activating a task filter. */
export function openTaskConversation(groupId: string, taskId: string, title = "") {
  const groups = useGroupStore.getState();
  const ui = useUIStore.getState();
  if (!groupId || !taskId.trim() || groups.selectedGroupId !== groupId || ui.webReadOnly) return;

  groups.closeChatWindow(groupId);
  clearOpenedMessageFromUrl();
  const resources = useResourcePanelStore.getState();
  const panel = resources.groups[groupId];
  if (
    panel?.open &&
    (ui.isSmallScreen || panel.maximized || (panel.tabs.length > 0 && resources.dock === "left"))
  ) {
    resources.setOpen(groupId, false);
  }
  ui.setGroupWorkView(groupId, "messages");
  ui.setChatMobileSurface(groupId, "messages");
  ui.setActiveTab("chat");
  useTaskViewStore.getState().open(groupId, taskId, title);
}

/**
 * Ends a task view when the chat turns to another Group, so coming back shows the full
 * conversation rather than a view the user left behind.
 */
export function useEndTaskViewOnGroupChange(groupId: string): void {
  useEffect(() => {
    const view = useTaskViewStore.getState().view;
    if (view && view.groupId !== groupId) useTaskViewStore.getState().close();
  }, [groupId]);
}
