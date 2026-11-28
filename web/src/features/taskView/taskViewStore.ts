import { useEffect } from "react";
import { create } from "zustand";

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
