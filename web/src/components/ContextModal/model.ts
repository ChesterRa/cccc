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
import { formatTime } from "../../utils/time";
import { getDefaultPetPersonaSeed } from "../../utils/rolePresets";
import { getTaskDisplaySummary, resolveTaskType, type TaskTypeId } from "../../utils/taskWorkflow";

export interface BriefDraft {
  objective: string;
  currentFocus: string;
  constraints: string;
  projectBrief: string;
  projectBriefStale: boolean;
}

export interface TaskDraft {
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
  checklist: string;
}

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
export type ContextModalView = "coordination" | "agents" | "desktop_pet";
export type SteeringTab = "summary" | "project" | "log" | "learning";
export type TaskFilterValue = "all" | "blocked" | "waiting_user" | "handoff" | "unassigned";
export type ContextTranslator = (key: string, defaultValue: string, options?: Record<string, unknown>) => string;
export type TaskDeleteBlockReason = "" | "self_history" | "subtree_history";
export type TaskDeleteInfo = { allowed: boolean; total: number; reason: TaskDeleteBlockReason };

export const WAITING_ON_VALUES: readonly TaskWaitingOn[] = [
  "none",
  "user",
  "actor",
  "external",
] as const;

export function getWaitingOnOptions(tr: ContextTranslator): Array<{ value: TaskWaitingOn; label: string }> {
  return [
    { value: "none", label: tr("context.none", "None") },
    { value: "user", label: tr("context.waitingOnUser", "Waiting on user") },
    { value: "actor", label: tr("context.waitingOnActor", "Waiting on agent") },
    { value: "external", label: tr("context.waitingOnExternal", "Waiting on external") },
  ];
}

export function resolvePetPersonaDraft(savedPetPersona: string): string {
  const saved = String(savedPetPersona || "").trim();
  if (saved) return saved;
  return getDefaultPetPersonaSeed();
}

export function isVisibleContextAgent(agent: AgentState | null | undefined): boolean {
  const id = String(agent?.id || "").trim();
  if (!id) return false;
  return id !== "pet-peer";
}

export function petPersonaDraftMatches(savedPetPersona: string, draft: string): boolean {
  return String(draft || "") === resolvePetPersonaDraft(savedPetPersona);
}

export function petPersonaDraftDirty(
  savedPetPersona: string,
  draft: string,
  options?: { loaded?: boolean | null },
): boolean {
  const loaded = options?.loaded === true;
  const draftText = String(draft || "");
  if (!loaded && draftText.trim() === "") return false;
  return !petPersonaDraftMatches(savedPetPersona, draftText);
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
  return String(task?.archived_from || "").trim().toLowerCase();
}

export function taskIsUnexecuted(task: Task | null | undefined): boolean {
  if (!task) return false;
  const status = taskStatus(task);
  const archivedFrom = taskArchivedFrom(task);
  return status === "planned" || (status === "archived" && (!archivedFrom || archivedFrom === "planned"));
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

export function listToText(items: string[] | null | undefined): string {
  return Array.isArray(items) ? items.join("\n") : "";
}

export function parseLineList(text: string): string[] {
  return text
    .split(/\n|,/)
    .map((item) => item.trim())
    .filter(Boolean);
}

export function checklistToText(items: TaskChecklistItem[] | null | undefined): string {
  if (!Array.isArray(items) || items.length === 0) return "";
  return items
    .map((item) => {
      const status = String(item.status || "pending").toLowerCase();
      const mark = status === "done" ? "[x]" : status === "in_progress" ? "[~]" : "[ ]";
      return `${mark} ${String(item.text || "").trim()}`.trim();
    })
    .join("\n");
}

export function parseChecklist(text: string, previous: TaskChecklistItem[] | null | undefined): TaskChecklistItem[] {
  const prior = Array.isArray(previous) ? previous : [];
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line, index) => {
      const match = line.match(/^(?:[-*]\s*)?\[(x|X|~| )\]\s*(.+)$/);
      const status = match
        ? match[1].toLowerCase() === "x"
          ? "done"
          : match[1] === "~"
            ? "in_progress"
            : "pending"
        : "pending";
      const textValue = (match ? match[2] : line.replace(/^[-*]\s*/, "")).trim();
      return {
        id: String(prior[index]?.id || `item-${index + 1}`),
        text: textValue,
        status,
      };
    });
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

export function taskToDraft(task: Task): TaskDraft {
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
    checklist: checklistToText(task.checklist),
  };
}

export function emptyTaskDraft(status: BoardStatus = "planned"): TaskDraft {
  return {
    title: "",
    outcome: "",
    status,
    taskType: "standard",
    assignee: "",
    priority: "",
    parentId: "",
    blockedBy: "",
    waitingOn: "none",
    handoffTo: "",
    notes: "",
    checklist: "",
  };
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

export function taskDraftDirty(draft: TaskDraft | null | undefined): boolean {
  if (!draft) return false;
  return !!(
    draft.title.trim()
    || draft.outcome.trim()
    || draft.taskType !== "standard"
    || draft.assignee.trim()
    || draft.priority.trim()
    || draft.parentId.trim()
    || draft.blockedBy.trim()
    || (draft.waitingOn && draft.waitingOn !== "none")
    || draft.handoffTo.trim()
    || draft.notes.trim()
    || draft.checklist.trim()
    || String(draft.status || "planned") !== "planned"
  );
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
    resumeHint: String(agent?.warm?.resume_hint || "").trim(),
  };
}

export function hasMindContext(agent: AgentState | null | undefined): boolean {
  const warm = agentWarm(agent);
  return !!(
    warm.environmentSummary
    || warm.userModel
    || warm.personaNotes
  );
}

export function hasRecoveryCues(agent: AgentState | null | undefined): boolean {
  const warm = agentWarm(agent);
  return !!(
    warm.whatChanged
    || warm.openLoops.length
    || warm.commitments.length
    || warm.resumeHint
  );
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
  return (Date.now() - timestamp) > 20 * 60 * 1000;
}

export function recoverySummary(agent: AgentState, tr: ContextTranslator): string {
  const warm = agentWarm(agent);
  const parts: string[] = [];
  if (warm.resumeHint) parts.push(tr("context.resumeReady", "resume ready"));
  if (warm.openLoops.length > 0) {
    parts.push(tr("context.openLoopsCount", "{{count}} open loops", { count: warm.openLoops.length }));
  }
  if (warm.commitments.length > 0) {
    parts.push(tr("context.commitmentsCount", "{{count}} commitments", { count: warm.commitments.length }));
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
    planned: sortTasks(tasks.filter((task) => taskStatus(task) === "planned"), "created"),
    active: sortTasks(tasks.filter((task) => taskStatus(task) === "active"), "updated"),
    done: sortTasks(tasks.filter((task) => taskStatus(task) === "done"), "updated"),
    archived: sortTasks(tasks.filter((task) => taskStatus(task) === "archived"), "updated"),
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
      blocked_by: Array.isArray(entry.blocked_by) ? entry.blocked_by.map((item) => String(item)) : [],
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

export function buildBoard(tasks: Task[], board: GroupContext["board"] | null | undefined): BoardColumns {
  const fallback = boardFallback(tasks);
  if (!board) return fallback;

  const taskMap = new Map(tasks.map((task) => [task.id, task] as const));
  const seen = new Set<string>();
  const resolve = (entries: TaskBoardEntry[] | undefined, statusKey: keyof BoardColumns): Task[] => {
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
  if (status === "active") return "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 border-emerald-500/30";
  if (status === "done") return "bg-blue-500/15 text-blue-600 dark:text-blue-400 border-blue-500/30";
  if (status === "archived") return "bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)] border-[var(--glass-border-subtle)]";
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

export function briefDraftMatches(brief: CoordinationBrief | null | undefined, draft: BriefDraft): boolean {
  const currentConstraints = Array.isArray(brief?.constraints) ? brief.constraints.map((item) => String(item || "").trim()).filter(Boolean) : [];
  const draftConstraints = parseLineList(draft.constraints);
  return (
    String(brief?.objective || "") === draft.objective
    && String(brief?.current_focus || "") === draft.currentFocus
    && JSON.stringify(currentConstraints) === JSON.stringify(draftConstraints)
    && String(brief?.project_brief || "") === draft.projectBrief
    && !!brief?.project_brief_stale === !!draft.projectBriefStale
  );
}

export function taskOptionLabel(task: Task): string {
  const title = taskTitle(task);
  return title ? `${task.id} · ${title}` : task.id;
}

export function taskDraftMatches(task: Task | null | undefined, draft: TaskDraft | null | undefined): boolean {
  if (!task || !draft) return false;
  return (
    taskTitle(task) === draft.title
    && taskOutcome(task) === draft.outcome
    && taskStatus(task) === draft.status
    && resolveTaskType({
      parent_id: task.parent_id,
      task_type: task.task_type,
      status: task.status,
      assignee: task.assignee,
      outcome: task.outcome,
      notes: task.notes,
      checklist: task.checklist,
    }) === draft.taskType
    && String(task.assignee || "") === draft.assignee
    && String(task.priority || "") === draft.priority
    && String(task.parent_id || "") === draft.parentId
    && listToText(task.blocked_by) === draft.blockedBy
    && String(task.waiting_on || "none") === draft.waitingOn
    && String(task.handoff_to || "") === draft.handoffTo
    && String(task.notes || "") === draft.notes
    && checklistToText(task.checklist) === draft.checklist
  );
}
