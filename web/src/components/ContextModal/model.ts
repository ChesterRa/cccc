import type {
  AgentState,
  CoordinationBrief,
  CoordinationNote,
  GroupContext,
  Task,
  TaskBoardEntry,
  TaskChecklistItem,
  TaskWaitingOn,
} from "../../types";
import type { TaskUpdatePatch } from "../../services/api";
import { formatTime } from "../../utils/time";
import { getTaskDisplaySummary, resolveTaskType, type TaskTypeId } from "../../utils/taskWorkflow";

export interface BriefDraft {
  objective: string;
  currentFocus: string;
  constraints: string;
  projectBrief: string;
  projectBriefStale: boolean;
}

export interface TaskDraftFields {
  title: string;
  outcome: string;
  status: string;
  taskType: TaskTypeId;
  assignee: string;
  priority: string;
  parentId: string;
  blockedBy: string;
  waitingOn: TaskWaitingOn;
  handoffTo: string;
  notes: string;
  checklist: TaskChecklistItem[];
}

/**
 * A task being edited, with what it was made from: edits are told from
 * changes made elsewhere by comparing with `base`, and saving is refused
 * once the task has moved past `base.rev`. A new task has no revision.
 */
export interface TaskDraft extends TaskDraftFields {
  base: { rev: number | undefined; fields: TaskDraftFields };
}

/**
 * Why a draft may be behind its task: the task changed since the draft was
 * made from it, or a save went through but what it saved could not be read.
 */
export type TaskStale = "" | "changed" | "unconfirmed";

export interface NoteDraft {
  summary: string;
  taskId: string;
}

export interface BoardColumns {
  planned: Task[];
  active: Task[];
  done: Task[];
  archived: Task[];
}

export type BoardStatus = keyof BoardColumns;
export type ContextModalView = "tasks" | "instructions" | "agents" | "log" | "self_evolving_skills";
export type TaskFilterValue = "all" | "blocked" | "waiting_user" | "handoff" | "unassigned";
export type ContextTranslator = (
  key: string,
  defaultValue: string,
  options?: Record<string, unknown>,
) => string;
export type TaskDeleteBlockReason = "" | "self_history" | "subtree_history";
export type TaskDeleteInfo = { allowed: boolean; total: number; reason: TaskDeleteBlockReason };

export const WAITING_ON_VALUES: readonly TaskWaitingOn[] = [
  "none",
  "user",
  "actor",
  "external",
] as const;

export function getWaitingOnOptions(
  tr: ContextTranslator,
): Array<{ value: TaskWaitingOn; label: string }> {
  return [
    { value: "none", label: tr("context.none", "None") },
    { value: "user", label: tr("context.waitingOnUser", "Waiting on user") },
    { value: "actor", label: tr("context.waitingOnActor", "Waiting on agent") },
    { value: "external", label: tr("context.waitingOnExternal", "Waiting on external") },
  ];
}

/** The label `labels` has for `key` itself; a key is free text, so not one it inherits. */
function ownLabel(labels: Record<string, string>, key: string): string {
  return Object.prototype.hasOwnProperty.call(labels, key) ? labels[key] : "";
}

export function taskStatusLabel(status: string, tr: ContextTranslator): string {
  const labels: Record<string, string> = {
    planned: tr("context.planned", "Planned"),
    active: tr("context.active", "Active"),
    done: tr("context.done", "Done"),
    archived: tr("context.archived", "Archived"),
  };
  return ownLabel(labels, status) || status;
}

export function priorityLabel(value: string, tr: ContextTranslator): string {
  const labels: Record<string, string> = {
    low: tr("context.priorityLow", "Low"),
    medium: tr("context.priorityMedium", "Medium"),
    normal: tr("context.priorityNormal", "Normal"),
    high: tr("context.priorityHigh", "High"),
    urgent: tr("context.priorityUrgent", "Urgent"),
  };
  return ownLabel(labels, value.trim().toLowerCase()) || value;
}

export function isVisibleContextAgent(agent: AgentState | null | undefined): boolean {
  const id = String(agent?.id || "").trim();
  return Boolean(id);
}

export function taskTitle(task: Task | null | undefined): string {
  if (!task) return "";
  return String(task.title || task.id || "").trim();
}

export function taskOutcome(task: Task | null | undefined): string {
  if (!task) return "";
  return String(task.outcome || "");
}

export function taskDisplaySummary(task: Task | null | undefined): string {
  if (!task) return "";
  return getTaskDisplaySummary({
    parent_id: task.parent_id,
    outcome: task.outcome,
    notes: task.notes,
  });
}

export function taskStatus(task: Task | null | undefined): string {
  return String(task?.status || "planned").toLowerCase();
}

export function taskArchivedFrom(task: Task | null | undefined): string {
  return String(task?.archived_from || "")
    .trim()
    .toLowerCase();
}

export function taskIsUnexecuted(task: Task | null | undefined): boolean {
  if (!task) return false;
  const status = taskStatus(task);
  const archivedFrom = taskArchivedFrom(task);
  return (
    status === "planned" || (status === "archived" && (!archivedFrom || archivedFrom === "planned"))
  );
}

export function getTaskDeleteInfo(task: Task | null | undefined, allTasks: Task[]): TaskDeleteInfo {
  if (!task) return { allowed: false, total: 0, reason: "" };
  const byParent = new Map<string, Task[]>();
  for (const candidate of allTasks) {
    const parentId = String(candidate.parent_id || "").trim();
    if (!parentId) continue;
    const current = byParent.get(parentId) || [];
    current.push(candidate);
    byParent.set(parentId, current);
  }
  for (const children of byParent.values()) {
    children.sort((a, b) => String(a.id || "").localeCompare(String(b.id || "")));
  }

  const seen = new Set<string>();
  const stack: Task[] = [task];
  const subtree: Task[] = [];
  while (stack.length > 0) {
    const current = stack.pop();
    if (!current) continue;
    const currentId = String(current.id || "").trim();
    if (!currentId || seen.has(currentId)) continue;
    seen.add(currentId);
    subtree.push(current);
    const children = byParent.get(currentId) || [];
    for (let index = children.length - 1; index >= 0; index -= 1) {
      stack.push(children[index]);
    }
  }

  for (const candidate of subtree) {
    if (!taskIsUnexecuted(candidate)) {
      return {
        allowed: false,
        total: subtree.length,
        reason: candidate.id === task.id ? "self_history" : "subtree_history",
      };
    }
  }
  return { allowed: true, total: subtree.length, reason: "" };
}

export type SubtaskProgress = { done: number; total: number };

function liveParentId(task: Task): string {
  const parentId = String(task.parent_id || "").trim();
  if (!parentId || parentId === String(task.id || "").trim()) return "";
  return taskStatus(task) === "archived" ? "" : parentId;
}

// Direct, non-archived subtasks only; a grandchild counts for its own parent.
export function subtaskProgressByParent(tasks: readonly Task[]): Map<string, SubtaskProgress> {
  const progress = new Map<string, SubtaskProgress>();
  for (const task of tasks) {
    const parentId = liveParentId(task);
    if (!parentId) continue;
    const current = progress.get(parentId) || { done: 0, total: 0 };
    progress.set(parentId, {
      done: current.done + (taskStatus(task) === "done" ? 1 : 0),
      total: current.total + 1,
    });
  }
  return progress;
}

export function directSubtasks(taskId: string, tasks: readonly Task[]): Task[] {
  const id = String(taskId || "").trim();
  if (!id) return [];
  return tasks
    .filter((task) => liveParentId(task) === id)
    .sort((left, right) =>
      String(left.id || "").localeCompare(String(right.id || ""), undefined, { numeric: true }),
    );
}

export function listToText(items: string[] | null | undefined): string {
  return Array.isArray(items) ? items.join("\n") : "";
}

export function parseLineList(text: string): string[] {
  return text
    .split(/\n|,/)
    .map((item) => item.trim())
    .filter(Boolean);
}

/**
 * An id no item in `items` has, in the `item-N` form agents also use: one
 * past the highest N, or the lowest free N once that is past what a number
 * holds exactly. Ids are any text, so their N can be that large.
 */
export function nextChecklistItemId(items: readonly TaskChecklistItem[]): string {
  const highest = items.reduce((max, item) => {
    const n = Number(/^item-(\d+)$/.exec(item.id)?.[1] || 0);
    return Number.isSafeInteger(n) ? Math.max(max, n) : max;
  }, 0);
  if (Number.isSafeInteger(highest + 1)) return `item-${highest + 1}`;
  const taken = new Set(items.map((item) => item.id));
  let next = 1;
  while (taken.has(`item-${next}`)) next += 1;
  return `item-${next}`;
}

/** The items worth keeping: those with any text, kept as written. */
export function checklistToSave(items: readonly TaskChecklistItem[]): TaskChecklistItem[] {
  return items.filter((item) => item.text.trim());
}

function draftChecklist(items: TaskChecklistItem[] | null | undefined): TaskChecklistItem[] {
  const draft: TaskChecklistItem[] = [];
  for (const item of Array.isArray(items) ? items : []) {
    // Items stored without an id, or with one an earlier item has, get one,
    // the same each time. The status is kept as stored, whatever it is,
    // until the item is ticked or unticked.
    const stored = String(item.id || "");
    const id =
      stored && !draft.some((kept) => kept.id === stored)
        ? stored
        : nextChecklistItemId([...(items || []), ...draft]);
    draft.push({ id, text: String(item.text || ""), status: item.status });
  }
  return draft;
}

export function briefToDraft(brief: CoordinationBrief | null | undefined): BriefDraft {
  return {
    objective: String(brief?.objective || ""),
    currentFocus: String(brief?.current_focus || ""),
    constraints: listToText(brief?.constraints),
    projectBrief: String(brief?.project_brief || ""),
    projectBriefStale: !!brief?.project_brief_stale,
  };
}

function taskDraftFields(task: Task): TaskDraftFields {
  return {
    title: taskTitle(task),
    outcome: taskOutcome(task),
    status: taskStatus(task),
    taskType: resolveTaskType({
      parent_id: task.parent_id,
      task_type: task.task_type,
      status: task.status,
      assignee: task.assignee,
      outcome: task.outcome,
      notes: task.notes,
      checklist: task.checklist,
    }),
    assignee: String(task.assignee || ""),
    priority: String(task.priority || ""),
    parentId: String(task.parent_id || ""),
    blockedBy: listToText(task.blocked_by),
    waitingOn: (String(task.waiting_on || "none") || "none") as TaskWaitingOn,
    handoffTo: String(task.handoff_to || ""),
    notes: String(task.notes || ""),
    checklist: draftChecklist(task.checklist),
  };
}

export function taskToDraft(task: Task): TaskDraft {
  const fields = taskDraftFields(task);
  return { ...fields, base: { rev: task.rev, fields } };
}

export function emptyTaskDraft(status: BoardStatus = "planned", parentId = ""): TaskDraft {
  const fields: TaskDraftFields = {
    title: "",
    outcome: "",
    status,
    taskType: parentId ? "free" : "standard",
    assignee: "",
    priority: "",
    parentId,
    blockedBy: "",
    waitingOn: "none",
    handoffTo: "",
    notes: "",
    checklist: [],
  };
  return { ...fields, base: { rev: undefined, fields } };
}

export function alignTaskDraftTaskType(
  taskType: TaskTypeId,
  nextParentId: string | null | undefined,
  previousParentId?: string | null | undefined,
): TaskTypeId {
  const hadParent = !!String(previousParentId || "").trim();
  const hasParent = !!String(nextParentId || "").trim();
  const previousDefault: TaskTypeId = hadParent ? "free" : "standard";
  const nextDefault: TaskTypeId = hasParent ? "free" : "standard";
  if (taskType === previousDefault) return nextDefault;
  return taskType;
}

/** Whether the draft holds edits: it differs from what it was made from. */
export function taskDraftChanged(draft: TaskDraft | null | undefined): boolean {
  return !!draft && !sameTaskDraftFields(draft, draft.base.fields);
}

function sameTaskDraftFields(left: TaskDraftFields, right: TaskDraftFields): boolean {
  return (
    left.title === right.title &&
    left.outcome === right.outcome &&
    left.status === right.status &&
    left.taskType === right.taskType &&
    left.assignee === right.assignee &&
    left.priority === right.priority &&
    left.parentId === right.parentId &&
    left.blockedBy === right.blockedBy &&
    left.waitingOn === right.waitingOn &&
    left.handoffTo === right.handoffTo &&
    left.notes === right.notes &&
    sameChecklist(left.checklist, right.checklist)
  );
}

function sameChecklist(left: TaskChecklistItem[], right: TaskChecklistItem[]): boolean {
  return (
    left.length === right.length &&
    left.every((item, index) => {
      const other = right[index];
      return item.id === other.id && item.text === other.text && item.status === other.status;
    })
  );
}

/**
 * What a draft changed from what it was made from, and nothing else: a field
 * left alone is not sent, so it is not rewritten in the form this editor
 * would give it.
 */
export function taskDraftPatch(draft: TaskDraft): TaskUpdatePatch {
  const base = draft.base.fields;
  const patch: TaskUpdatePatch = {};
  if (draft.title !== base.title) patch.title = draft.title.trim();
  if (draft.outcome !== base.outcome) patch.outcome = draft.outcome;
  if (draft.assignee !== base.assignee) patch.assignee = draft.assignee.trim() || null;
  if (draft.priority !== base.priority) patch.priority = draft.priority.trim() || null;
  if (draft.parentId !== base.parentId) patch.parent_id = draft.parentId.trim() || null;
  if (draft.blockedBy !== base.blockedBy) patch.blocked_by = parseLineList(draft.blockedBy);
  if (draft.waitingOn !== base.waitingOn) patch.waiting_on = draft.waitingOn;
  if (draft.handoffTo !== base.handoffTo) patch.handoff_to = draft.handoffTo.trim() || null;
  if (draft.taskType !== base.taskType) patch.task_type = draft.taskType;
  if (draft.notes !== base.notes) patch.notes = draft.notes;
  if (!sameChecklist(draft.checklist, base.checklist)) {
    patch.checklist = checklistToSave(draft.checklist);
  }
  if (draft.status !== base.status) patch.status = draft.status;
  return patch;
}

/**
 * Whether `task` is the draft's base with what saving the draft sends, and
 * nothing else, changed: how a save whose answer was lost shows it went through.
 */
export function taskHoldsDraftSave(task: Task, draft: TaskDraft): boolean {
  const since = taskDraftPatch({ ...taskDraftFields(task), base: draft.base });
  return JSON.stringify(since) === JSON.stringify(taskDraftPatch(draft));
}

export function countLike(value: number | TaskBoardEntry[] | undefined, fallback: number): number {
  if (typeof value === "number") return value;
  if (Array.isArray(value)) return value.length;
  return fallback;
}

export function agentHot(agent: AgentState | null | undefined) {
  return {
    activeTaskId: String(agent?.hot?.active_task_id || "").trim(),
    focus: String(agent?.hot?.focus || "").trim(),
    nextAction: String(agent?.hot?.next_action || "").trim(),
    blockers: Array.isArray(agent?.hot?.blockers) ? agent.hot.blockers : [],
  };
}

export function agentWarm(agent: AgentState | null | undefined) {
  return {
    whatChanged: String(agent?.warm?.what_changed || "").trim(),
    openLoops: Array.isArray(agent?.warm?.open_loops) ? agent.warm.open_loops : [],
    commitments: Array.isArray(agent?.warm?.commitments) ? agent.warm.commitments : [],
    environmentSummary: String(agent?.warm?.environment_summary || "").trim(),
    userModel: String(agent?.warm?.user_model || "").trim(),
    personaNotes: String(agent?.warm?.persona_notes || "").trim(),
  };
}

export function hasMindContext(agent: AgentState | null | undefined): boolean {
  const warm = agentWarm(agent);
  return !!(warm.environmentSummary || warm.userModel || warm.personaNotes);
}

export function hasRecoveryCues(agent: AgentState | null | undefined): boolean {
  const warm = agentWarm(agent);
  return !!(warm.whatChanged || warm.openLoops.length || warm.commitments.length);
}

export function agentUpdatedAtTimestamp(agent: AgentState | null | undefined): number {
  const raw = String(agent?.updated_at || "").trim();
  if (!raw) return 0;
  const timestamp = Date.parse(raw);
  return Number.isFinite(timestamp) ? timestamp : 0;
}

export function isAgentStale(agent: AgentState | null | undefined): boolean {
  const timestamp = agentUpdatedAtTimestamp(agent);
  if (!timestamp) return false;
  return Date.now() - timestamp > 20 * 60 * 1000;
}

export function recoverySummary(agent: AgentState, tr: ContextTranslator): string {
  const warm = agentWarm(agent);
  const parts: string[] = [];
  if (warm.openLoops.length > 0) {
    parts.push(
      tr("context.openLoopsCount", "{{count}} open loops", { count: warm.openLoops.length }),
    );
  }
  if (warm.commitments.length > 0) {
    parts.push(
      tr("context.commitmentsCount", "{{count}} commitments", { count: warm.commitments.length }),
    );
  }
  if (parts.length === 0 && warm.whatChanged) {
    parts.push(tr("context.changeCaptured", "change captured"));
  }
  return parts.join(" · ") || tr("context.noRecoveryCues", "No recovery cues");
}

function sortTasks(tasks: Task[], mode: "created" | "updated"): Task[] {
  const key = mode === "created" ? "created_at" : "updated_at";
  return [...tasks].sort((left, right) => {
    const a = String(left[key] || "");
    const b = String(right[key] || "");
    return b.localeCompare(a) || String(left.id).localeCompare(String(right.id));
  });
}

function boardFallback(tasks: Task[]): BoardColumns {
  return {
    planned: sortTasks(
      tasks.filter((task) => taskStatus(task) === "planned"),
      "created",
    ),
    active: sortTasks(
      tasks.filter((task) => taskStatus(task) === "active"),
      "updated",
    ),
    done: sortTasks(
      tasks.filter((task) => taskStatus(task) === "done"),
      "updated",
    ),
    archived: sortTasks(
      tasks.filter((task) => taskStatus(task) === "archived"),
      "updated",
    ),
  };
}

function coerceBoardTask(entry: TaskBoardEntry, taskMap: Map<string, Task>): Task | null {
  if (typeof entry === "string") {
    return taskMap.get(entry) || null;
  }
  const id = String(entry.id || "").trim();
  if (!id) return null;
  return (
    taskMap.get(id) || {
      id,
      title: String(entry.title || id),
      outcome: String(entry.outcome || ""),
      status: String(entry.status || "planned"),
      assignee: typeof entry.assignee === "string" ? entry.assignee : null,
      priority: typeof entry.priority === "string" ? entry.priority : null,
      parent_id: typeof entry.parent_id === "string" ? entry.parent_id : null,
      blocked_by: Array.isArray(entry.blocked_by)
        ? entry.blocked_by.map((item) => String(item))
        : [],
      waiting_on: typeof entry.waiting_on === "string" ? entry.waiting_on : undefined,
      handoff_to: typeof entry.handoff_to === "string" ? entry.handoff_to : null,
      task_type: typeof entry.task_type === "string" ? entry.task_type : null,
      notes: typeof entry.notes === "string" ? entry.notes : "",
      checklist: [],
      created_at: typeof entry.created_at === "string" ? entry.created_at : null,
      updated_at: typeof entry.updated_at === "string" ? entry.updated_at : null,
      archived_from: typeof entry.archived_from === "string" ? entry.archived_from : null,
    }
  );
}

export function buildBoard(
  tasks: Task[],
  board: GroupContext["board"] | null | undefined,
): BoardColumns {
  const fallback = boardFallback(tasks);
  if (!board) return fallback;

  const taskMap = new Map(tasks.map((task) => [task.id, task] as const));
  const seen = new Set<string>();
  const resolve = (
    entries: TaskBoardEntry[] | undefined,
    statusKey: keyof BoardColumns,
  ): Task[] => {
    const projected = Array.isArray(entries)
      ? entries
          .map((entry) => coerceBoardTask(entry, taskMap))
          .filter((task): task is Task => !!task)
      : [];
    for (const task of projected) seen.add(task.id);
    const remainder = fallback[statusKey].filter((task) => !seen.has(task.id));
    return [...projected, ...remainder];
  };

  return {
    planned: resolve(board.planned, "planned"),
    active: resolve(board.active, "active"),
    done: resolve(board.done, "done"),
    archived: resolve(board.archived, "archived"),
  };
}

export function statusTone(status: string): string {
  if (status === "active")
    return "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 border-emerald-500/30";
  if (status === "done")
    return "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white";
  if (status === "archived")
    return "bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)] border-[var(--glass-border-subtle)]";
  return "bg-amber-500/15 text-amber-600 dark:text-amber-400 border-amber-500/30";
}

export function waitingLabel(value: string, tr: ContextTranslator): string {
  const match = getWaitingOnOptions(tr).find((option) => option.value === value);
  return match ? match.label : value || tr("context.none", "None");
}

export function noteTimestamp(note: CoordinationNote): string {
  const at = String(note.at || "").trim();
  return at ? formatTime(at) : "";
}

export function emptyNoteDraft(): NoteDraft {
  return { summary: "", taskId: "" };
}

export function briefDraftMatches(
  brief: CoordinationBrief | null | undefined,
  draft: BriefDraft,
): boolean {
  const currentConstraints = Array.isArray(brief?.constraints)
    ? brief.constraints.map((item) => String(item || "").trim()).filter(Boolean)
    : [];
  const draftConstraints = parseLineList(draft.constraints);
  return (
    String(brief?.objective || "") === draft.objective &&
    String(brief?.current_focus || "") === draft.currentFocus &&
    JSON.stringify(currentConstraints) === JSON.stringify(draftConstraints) &&
    String(brief?.project_brief || "") === draft.projectBrief &&
    !!brief?.project_brief_stale === !!draft.projectBriefStale
  );
}

export function taskOptionLabel(task: Task): string {
  const title = taskTitle(task);
  return title ? `${task.id} · ${title}` : task.id;
}

/** Whether a task, at the revision a draft was made from, still reads as that draft's base. */
export function taskMatchesDraftBase(task: Task, draft: TaskDraft): boolean {
  return (
    task.rev === draft.base.rev && sameTaskDraftFields(taskDraftFields(task), draft.base.fields)
  );
}
