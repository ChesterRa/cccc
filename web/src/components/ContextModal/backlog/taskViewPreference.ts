export type TasksView = "board" | "backlog";

const TASK_VIEW_KEY = "cccc-context-task-view";

/** Every Group's last view; an unreadable value counts as none, so the next save replaces it. */
function readAll(): Record<string, unknown> {
  try {
    const raw = JSON.parse(localStorage.getItem(TASK_VIEW_KEY) || "{}");
    return raw && typeof raw === "object" && !Array.isArray(raw) ? raw : {};
  } catch {
    return {};
  }
}

/** Which view of a Group's tasks this browser showed last; the board unless it was the backlog. */
export function loadTasksView(groupId: string): TasksView {
  return readAll()[groupId] === "backlog" ? "backlog" : "board";
}

export function saveTasksView(groupId: string, view: TasksView): void {
  if (!groupId) return;
  try {
    const all = readAll();
    if (view === "backlog") all[groupId] = "backlog";
    else delete all[groupId];
    localStorage.setItem(TASK_VIEW_KEY, JSON.stringify(all));
  } catch {
    // The choice still applies until the page is closed.
  }
}
