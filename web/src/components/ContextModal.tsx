import { useCallback, useEffect, useMemo, useState } from "react";
import { DndContext, DragEndEvent, DragOverlay, DragStartEvent, PointerSensor, TouchSensor, useDraggable, useDroppable, useSensor, useSensors } from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import { useTranslation } from "react-i18next";
import { addCoordinationNote, apiJson, contextSync, updateCoordinationBrief, updateCoordinationTask } from "../services/api";
import type {
  AgentState,
  CoordinationBrief,
  CoordinationNote,
  GroupContext,
  ProjectMdInfo,
  Task,
  TaskBoardEntry,
  TaskChecklistItem,
  TaskWaitingOn,
} from "../types";
import { formatFullTime, formatTime } from "../utils/time";
import { classNames } from "../utils/classNames";
import { MarkdownRenderer } from "./MarkdownRenderer";
import { useModalA11y } from "../hooks/useModalA11y";
import { ModalFrame } from "./modals/ModalFrame";

interface ContextModalProps {
  isOpen: boolean;
  onClose: () => void;
  groupId: string;
  context: GroupContext | null;
  onRefreshContext: () => Promise<void>;
  isDark: boolean;
}

interface BriefDraft {
  objective: string;
  currentFocus: string;
  constraints: string;
  projectBrief: string;
  projectBriefStale: boolean;
}

interface TaskDraft {
  title: string;
  outcome: string;
  status: string;
  assignee: string;
  priority: string;
  parentId: string;
  blockedBy: string;
  waitingOn: TaskWaitingOn;
  handoffTo: string;
  notes: string;
  checklist: string;
}

interface NoteDraft {
  summary: string;
  taskId: string;
}

interface BoardColumns {
  planned: Task[];
  active: Task[];
  done: Task[];
  archived: Task[];
}

type BoardStatus = keyof BoardColumns;

const WAITING_ON_OPTIONS: Array<{ value: TaskWaitingOn; label: string }> = [
  { value: "none", label: "None" },
  { value: "user", label: "Waiting on user" },
  { value: "actor", label: "Waiting on agent" },
  { value: "external", label: "Waiting on external" },
];

function taskTitle(task: Task | null | undefined): string {
  if (!task) return "";
  return String(task.title || task.id || "").trim();
}

function taskOutcome(task: Task | null | undefined): string {
  if (!task) return "";
  return String(task.outcome || "");
}

function taskStatus(task: Task | null | undefined): string {
  return String(task?.status || "planned").toLowerCase();
}

function listToText(items: string[] | null | undefined): string {
  return Array.isArray(items) ? items.join("\n") : "";
}

function parseLineList(text: string): string[] {
  return text
    .split(/\n|,/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function checklistToText(items: TaskChecklistItem[] | null | undefined): string {
  if (!Array.isArray(items) || items.length === 0) return "";
  return items
    .map((item) => {
      const status = String(item.status || "pending").toLowerCase();
      const mark = status === "done" ? "[x]" : status === "in_progress" ? "[~]" : "[ ]";
      return `${mark} ${String(item.text || "").trim()}`.trim();
    })
    .join("\n");
}

function parseChecklist(text: string, previous: TaskChecklistItem[] | null | undefined): TaskChecklistItem[] {
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

function briefToDraft(brief: CoordinationBrief | null | undefined): BriefDraft {
  return {
    objective: String(brief?.objective || ""),
    currentFocus: String(brief?.current_focus || ""),
    constraints: listToText(brief?.constraints),
    projectBrief: String(brief?.project_brief || ""),
    projectBriefStale: !!brief?.project_brief_stale,
  };
}

function taskToDraft(task: Task): TaskDraft {
  return {
    title: taskTitle(task),
    outcome: taskOutcome(task),
    status: taskStatus(task),
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

function emptyTaskDraft(status: BoardStatus = "planned"): TaskDraft {
  return {
    title: "",
    outcome: "",
    status,
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

function taskDraftDirty(draft: TaskDraft | null | undefined): boolean {
  if (!draft) return false;
  return !!(
    draft.title.trim()
    || draft.outcome.trim()
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

function countLike(value: number | TaskBoardEntry[] | undefined, fallback: number): number {
  if (typeof value === "number") return value;
  if (Array.isArray(value)) return value.length;
  return fallback;
}

function agentHot(agent: AgentState | null | undefined) {
  return {
    activeTaskId: String(agent?.hot?.active_task_id || "").trim(),
    focus: String(agent?.hot?.focus || "").trim(),
    nextAction: String(agent?.hot?.next_action || "").trim(),
    blockers: Array.isArray(agent?.hot?.blockers) ? agent.hot.blockers : [],
  };
}

function agentWarm(agent: AgentState | null | undefined) {
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

function hasWarmState(agent: AgentState): boolean {
  const warm = agentWarm(agent);
  return !!(
    warm.whatChanged ||
    warm.openLoops.length ||
    warm.commitments.length ||
    warm.environmentSummary ||
    warm.userModel ||
    warm.personaNotes ||
    warm.resumeHint
  );
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
      notes: typeof entry.notes === "string" ? entry.notes : "",
      checklist: [],
      created_at: typeof entry.created_at === "string" ? entry.created_at : null,
      updated_at: typeof entry.updated_at === "string" ? entry.updated_at : null,
      archived_from: typeof entry.archived_from === "string" ? entry.archived_from : null,
    }
  );
}

function buildBoard(tasks: Task[], board: GroupContext["board"] | null | undefined): BoardColumns {
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

function statusTone(status: string, isDark: boolean): string {
  if (status === "active") return isDark ? "bg-emerald-500/15 text-emerald-200 border-emerald-500/30" : "bg-emerald-50 text-emerald-700 border-emerald-200";
  if (status === "done") return isDark ? "bg-blue-500/15 text-blue-200 border-blue-500/30" : "bg-blue-50 text-blue-700 border-blue-200";
  if (status === "archived") return isDark ? "bg-slate-700/60 text-slate-200 border-slate-600" : "bg-slate-100 text-slate-700 border-slate-200";
  return isDark ? "bg-amber-500/15 text-amber-200 border-amber-500/30" : "bg-amber-50 text-amber-700 border-amber-200";
}

function waitingLabel(value: string): string {
  const match = WAITING_ON_OPTIONS.find((option) => option.value === value);
  return match ? match.label : value || "None";
}

function noteTimestamp(note: CoordinationNote): string {
  const at = String(note.at || "").trim();
  return at ? formatTime(at) : "";
}

function emptyNoteDraft(): NoteDraft {
  return { summary: "", taskId: "" };
}

function briefDraftMatches(brief: CoordinationBrief | null | undefined, draft: BriefDraft): boolean {
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

function taskOptionLabel(task: Task): string {
  const title = taskTitle(task);
  return title ? `${task.id} · ${title}` : task.id;
}

function taskDraftMatches(task: Task | null | undefined, draft: TaskDraft | null | undefined): boolean {
  if (!task || !draft) return false;
  return (
    taskTitle(task) === draft.title
    && taskOutcome(task) === draft.outcome
    && taskStatus(task) === draft.status
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

export function ContextModal({
  isOpen,
  onClose,
  groupId,
  context,
  onRefreshContext,
  isDark,
}: ContextModalProps) {
  const { t } = useTranslation("modals");
  const tr = (key: string, fallback: string, vars?: Record<string, unknown>) =>
    String(t(key as never, { defaultValue: fallback, ...(vars || {}) } as never));

  const [activeView, setActiveView] = useState<"coordination" | "agents">("coordination");
  const [steeringTab, setSteeringTab] = useState<"summary" | "project" | "log">("summary");
  const [taskFilter, setTaskFilter] = useState<"all" | "blocked" | "waiting_user" | "handoff" | "unassigned">("all");
  const [assigneeFilter, setAssigneeFilter] = useState<string>("__all__");
  const [taskQuery, setTaskQuery] = useState("");
  const [dragTaskId, setDragTaskId] = useState("");
  const [editingBrief, setEditingBrief] = useState(false);
  const [briefDraft, setBriefDraft] = useState<BriefDraft>(briefToDraft(null));
  const [taskEditorMode, setTaskEditorMode] = useState<"none" | "create" | "edit">("none");
  const [selectedTaskId, setSelectedTaskId] = useState("");
  const [taskDraft, setTaskDraft] = useState<TaskDraft | null>(null);
  const [syncBusy, setSyncBusy] = useState(false);
  const [syncError, setSyncError] = useState("");

  const [projectMd, setProjectMd] = useState<ProjectMdInfo | null>(null);
  const [projectBusy, setProjectBusy] = useState(false);
  const [projectError, setProjectError] = useState("");
  const [projectNotice, setProjectNotice] = useState("");
  const [editingProject, setEditingProject] = useState(false);
  const [projectText, setProjectText] = useState("");
  const [notifyAgents, setNotifyAgents] = useState(false);
  const [notifyError, setNotifyError] = useState("");
  const [decisionDraft, setDecisionDraft] = useState<NoteDraft>(emptyNoteDraft());
  const [handoffDraft, setHandoffDraft] = useState<NoteDraft>(emptyNoteDraft());
  const [activityBusyKind, setActivityBusyKind] = useState<"decision" | "handoff" | null>(null);
  const [activityError, setActivityError] = useState("");

  const brief = context?.coordination?.brief || null;
  const tasks = useMemo(() => (Array.isArray(context?.coordination?.tasks) ? context.coordination.tasks : []), [context]);
  const agents = useMemo(() => (Array.isArray(context?.agent_states) ? context.agent_states : []), [context]);
  const board = useMemo(() => buildBoard(tasks, context?.board), [context?.board, tasks]);

  const allBoardTasks = useMemo(
    () => [...board.active, ...board.planned, ...board.done, ...board.archived],
    [board.active, board.archived, board.done, board.planned]
  );

  const taskMap = useMemo(() => {
    const map = new Map<string, Task>();
    for (const task of allBoardTasks) map.set(task.id, task);
    return map;
  }, [allBoardTasks]);

  const selectedTask = selectedTaskId ? taskMap.get(selectedTaskId) || null : null;

  const tasksSummary = useMemo(() => {
    const fallback = {
      total: tasks.length,
      planned: board.planned.length,
      active: board.active.length,
      done: board.done.length,
      archived: board.archived.length,
    };
    return context?.tasks_summary || fallback;
  }, [board.active.length, board.archived.length, board.done.length, board.planned.length, context?.tasks_summary, tasks.length]);

  const attentionCounts = useMemo(() => {
    const blockedFallback = tasks.filter((task) => taskStatus(task) === "active" && Array.isArray(task.blocked_by) && task.blocked_by.length > 0).length;
    const waitingUserFallback = tasks.filter((task) => String(task.waiting_on || "none") === "user").length;
    const handoffFallback = tasks.filter((task) => !!String(task.handoff_to || "").trim() && taskStatus(task) !== "archived").length;
    return {
      blocked: countLike(context?.attention?.blocked, blockedFallback),
      waitingUser: countLike(context?.attention?.waiting_user, waitingUserFallback),
      pendingHandoffs: countLike(context?.attention?.pending_handoffs, handoffFallback),
    };
  }, [context?.attention, tasks]);

  const recentDecisions = useMemo(
    () => (Array.isArray(context?.coordination?.recent_decisions) ? context.coordination.recent_decisions : []),
    [context]
  );
  const recentHandoffs = useMemo(
    () => (Array.isArray(context?.coordination?.recent_handoffs) ? context.coordination.recent_handoffs : []),
    [context]
  );

  const projectPathLabel = useMemo(() => {
    const path = String(projectMd?.path || "").trim();
    return path || "PROJECT.md";
  }, [projectMd?.path]);

  const notifyMessage = useMemo(
    () => String(t("context.projectUpdatedNotify", { defaultValue: `PROJECT.md updated. Please re-read and realign. (${projectPathLabel})`, path: projectPathLabel })),
    [projectPathLabel, t]
  );

  const assigneeOptions = useMemo(
    () => Array.from(new Set(tasks.map((task) => String(task.assignee || "").trim()).filter(Boolean))).sort((a, b) => a.localeCompare(b)),
    [tasks]
  );

  const unassignedCount = useMemo(
    () => tasks.filter((task) => taskStatus(task) !== "archived" && !String(task.assignee || "").trim()).length,
    [tasks]
  );

  const activeTaskOptions = useMemo(
    () => tasks.filter((task) => taskStatus(task) !== "archived"),
    [tasks]
  );

  const hasBriefUnsaved = useMemo(
    () => editingBrief && !briefDraftMatches(brief, briefDraft),
    [brief, briefDraft, editingBrief]
  );

  const hasProjectUnsaved = useMemo(
    () => editingProject && projectText !== String(projectMd?.content || ""),
    [editingProject, projectMd?.content, projectText]
  );

  const hasSteeringUnsaved = hasBriefUnsaved || hasProjectUnsaved;
  const hasTaskUnsaved = useMemo(() => {
    if (!taskDraft || taskEditorMode === "none") return false;
    if (taskEditorMode === "create") return taskDraftDirty(taskDraft);
    return !!selectedTask && !taskDraftMatches(selectedTask, taskDraft);
  }, [selectedTask, taskDraft, taskEditorMode]);

  const taskEditorVisible = taskEditorMode !== "none" && !!taskDraft;

  const loadProjectMd = useCallback(async (force: boolean = false): Promise<ProjectMdInfo | null> => {
    if (!groupId) return null;
    if (!force && projectMd !== null) return projectMd;
    setProjectBusy(true);
    setProjectError("");
    try {
      const resp = await apiJson<ProjectMdInfo>(`/api/v1/groups/${encodeURIComponent(groupId)}/project_md`);
      if (!resp.ok) {
        setProjectMd(null);
        setProjectError(resp.error?.message || tr("context.failedToLoadProject", "Failed to load PROJECT.md"));
        return null;
      }
      setProjectMd(resp.result);
      return resp.result ?? null;
    } finally {
      setProjectBusy(false);
    }
  }, [groupId, projectMd, tr]);

  useEffect(() => {
    if (!isOpen || !groupId) return;

    setActiveView("coordination");
    setSteeringTab("summary");
    setTaskFilter("all");
    setAssigneeFilter("__all__");
    setTaskQuery("");
    setDragTaskId("");
    setTaskEditorMode("none");
    setSelectedTaskId("");
    setTaskDraft(null);
    setSyncError("");
    setEditingBrief(false);
    setProjectMd(null);
    setProjectBusy(false);
    setProjectError("");
    setProjectNotice("");
    setEditingProject(false);
    setProjectText("");
    setNotifyError("");
    setNotifyAgents(false);
    setDecisionDraft(emptyNoteDraft());
    setHandoffDraft(emptyNoteDraft());
    setActivityBusyKind(null);
    setActivityError("");
  }, [groupId, isOpen]);

  useEffect(() => {
    if (!isOpen || !groupId) return;
    if ((steeringTab === "project" || editingProject) && projectMd === null && !projectBusy) {
      void loadProjectMd();
    }
  }, [editingProject, groupId, isOpen, loadProjectMd, projectBusy, projectMd, steeringTab]);

  const taskMatches = useCallback(
    (task: Task): boolean => {
      const assignee = String(task.assignee || "").trim();
      const status = taskStatus(task);
      const blocked = Array.isArray(task.blocked_by) && task.blocked_by.length > 0;
      const waitingUser = String(task.waiting_on || "none") === "user";
      const handoff = !!String(task.handoff_to || "").trim();
      const query = taskQuery.trim().toLowerCase();

      if (assigneeFilter === "__unassigned__") {
        if (assignee) return false;
      } else if (assigneeFilter !== "__all__" && assignee !== assigneeFilter) {
        return false;
      }

      if (taskFilter === "blocked" && !(status !== "archived" && blocked)) return false;
      if (taskFilter === "waiting_user" && !(status !== "archived" && waitingUser)) return false;
      if (taskFilter === "handoff" && !(status !== "archived" && handoff)) return false;
      if (taskFilter === "unassigned" && !(status !== "archived" && !assignee)) return false;

      if (query) {
        const haystack = [
          task.id,
          taskTitle(task),
          taskOutcome(task),
          assignee,
          String(task.priority || ""),
          String(task.handoff_to || ""),
        ].join(" ").toLowerCase();
        if (!haystack.includes(query)) return false;
      }
      return true;
    },
    [assigneeFilter, taskFilter, taskQuery]
  );

  const filteredBoard = useMemo(
    () => ({
      planned: board.planned.filter(taskMatches),
      active: board.active.filter(taskMatches),
      done: board.done.filter(taskMatches),
      archived: board.archived.filter(taskMatches),
    }),
    [board, taskMatches]
  );

  const filteredTaskTotal = useMemo(
    () => filteredBoard.planned.length + filteredBoard.active.length + filteredBoard.done.length + filteredBoard.archived.length,
    [filteredBoard]
  );


  useEffect(() => {
    if (taskEditorMode !== "edit") return;
    if (!selectedTaskId) {
      setTaskEditorMode("none");
      if (taskDraft) setTaskDraft(null);
      return;
    }
    if (!selectedTask) {
      setSelectedTaskId("");
      setTaskDraft(null);
      setTaskEditorMode("none");
      return;
    }
    if (!taskDraft) {
      setTaskDraft(taskToDraft(selectedTask));
    }
  }, [selectedTaskId, selectedTask, taskDraft, taskEditorMode]);

  const surfaceClass = classNames(
    "rounded-2xl border shadow-sm",
    isDark ? "border-slate-800 bg-slate-900/80" : "border-gray-200 bg-white"
  );
  const mutedTextClass = isDark ? "text-slate-400" : "text-gray-500";
  const subtleTextClass = isDark ? "text-slate-300" : "text-gray-700";
  const inputClass = classNames(
    "w-full rounded-lg border px-3 py-2 text-sm outline-none transition-colors",
    isDark
      ? "border-slate-700 bg-slate-950 text-slate-100 placeholder:text-slate-500 focus:border-blue-500"
      : "border-gray-300 bg-white text-gray-900 placeholder:text-gray-400 focus:border-blue-500"
  );
  const textareaClass = classNames(inputClass, "min-h-[96px] resize-y");
  const buttonSecondaryClass = classNames(
    "rounded-lg px-3 py-2 text-sm transition-colors disabled:cursor-not-allowed disabled:opacity-50",
    isDark ? "bg-slate-800 text-slate-200 hover:bg-slate-700" : "bg-gray-100 text-gray-700 hover:bg-gray-200"
  );
  const buttonPrimaryClass = "rounded-lg bg-blue-600 px-3 py-2 text-sm text-white transition-colors hover:bg-blue-500 disabled:cursor-not-allowed disabled:opacity-50";
  const chipBaseClass = classNames(
    "rounded-full border px-3 py-1.5 text-xs font-medium transition-colors",
    isDark ? "border-slate-700 bg-slate-900 text-slate-200 hover:border-slate-600" : "border-gray-200 bg-white text-gray-700 hover:border-gray-300"
  );

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 180, tolerance: 6 } })
  );

  const moveTaskToStatus = async (task: Task, nextStatus: BoardStatus) => {
    if (!groupId) return;
    if (taskStatus(task) === nextStatus) return;
    setSyncBusy(true);
    setSyncError("");
    try {
      const resp = await updateCoordinationTask(groupId, {
        ...task,
        status: nextStatus,
      });
      if (!resp.ok) {
        setSyncError(resp.error?.message || tr("context.failedToApplyChanges", "Failed to apply changes"));
        return;
      }
      await onRefreshContext();
      if (selectedTaskId === task.id && taskEditorMode === "edit") {
        setTaskDraft((prev) => (prev ? { ...prev, status: nextStatus } : prev));
      }
    } finally {
      setSyncBusy(false);
    }
  };

  const handleDragStart = (event: DragStartEvent) => {
    const id = String(event.active.id || "");
    if (id.startsWith("task:")) setDragTaskId(id.slice(5));
  };

  const handleDragEnd = (event: DragEndEvent) => {
    setDragTaskId("");
    const activeId = String(event.active.id || "");
    if (!activeId.startsWith("task:")) return;
    const task = taskMap.get(activeId.slice(5));
    if (!task || !event.over) return;
    const overId = String(event.over.id || "");
    let nextStatus: BoardStatus | null = null;
    if (overId.startsWith("column:")) {
      nextStatus = overId.slice(7) as BoardStatus;
    } else if (overId.startsWith("task:")) {
      const overTask = taskMap.get(overId.slice(5));
      if (overTask) nextStatus = taskStatus(overTask) as BoardStatus;
    }
    if (nextStatus && nextStatus !== taskStatus(task)) {
      void moveTaskToStatus(task, nextStatus);
    }
  };

  const TaskGhostCard = ({ task }: { task: Task }) => {
    const status = taskStatus(task);
    const blocked = Array.isArray(task.blocked_by) && task.blocked_by.length > 0;
    return (
      <div className={classNames(
        "w-[320px] rounded-2xl border p-3 shadow-2xl",
        isDark ? "border-slate-700 bg-slate-900/95" : "border-gray-200 bg-white"
      )}>
        <div className="flex items-start justify-between gap-2">
          <div className="min-w-0">
            <div className={classNames("truncate text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{taskTitle(task)}</div>
            <div className={classNames("mt-1 text-xs", mutedTextClass)}>{task.id}</div>
          </div>
          <span className={classNames("rounded-full border px-2 py-0.5 text-[11px] font-medium", statusTone(status, isDark))}>{status}</span>
        </div>
        {taskOutcome(task) ? <div className={classNames("mt-2 line-clamp-3 text-xs", subtleTextClass)}>{taskOutcome(task)}</div> : null}
        <div className="mt-3 flex flex-wrap gap-1.5 text-[11px]">
          {task.assignee ? <span className={classNames("rounded-full px-2 py-0.5", isDark ? "bg-slate-800 text-slate-200" : "bg-gray-100 text-gray-700")}>{task.assignee}</span> : null}
          {blocked ? <span className={classNames("rounded-full px-2 py-0.5", isDark ? "bg-rose-500/15 text-rose-200" : "bg-rose-50 text-rose-700")}>{tr("context.blocked", "Blocked")}</span> : null}
        </div>
      </div>
    );
  };

  const TaskCard = ({ task }: { task: Task }) => {
    const status = taskStatus(task);
    const blocked = Array.isArray(task.blocked_by) && task.blocked_by.length > 0;
    const waiting = String(task.waiting_on || "none").trim();
    const handoff = String(task.handoff_to || "").trim();
    const { attributes, listeners, setNodeRef, transform, isDragging } = useDraggable({
      id: `task:${task.id}`,
      disabled: syncBusy,
      data: { type: "task", taskId: task.id, status },
    });
    const style = { transform: CSS.Translate.toString(transform) };
    const quickAction = status === "planned" ? { label: tr("context.start", "Start"), next: "active" as BoardStatus }
      : status === "active" ? { label: tr("context.done", "Done"), next: "done" as BoardStatus }
      : status === "done" ? { label: tr("context.reopen", "Reopen"), next: "active" as BoardStatus }
      : { label: tr("context.restore", "Restore"), next: "planned" as BoardStatus };

    return (
      <div ref={setNodeRef} style={style} className={classNames("group/task", isDragging && "z-20 opacity-80")}>
        <div
          {...attributes}
          onClick={() => selectTask(task)}
          className={classNames(
            "w-full cursor-pointer rounded-2xl border p-3 text-left transition-all",
            blocked
              ? isDark ? "border-rose-500/30 bg-rose-500/5" : "border-rose-200 bg-rose-50/40"
              : selectedTaskId === task.id
                ? isDark
                  ? "border-blue-500 bg-blue-500/10 shadow-[0_0_0_1px_rgba(59,130,246,0.4)]"
                  : "border-blue-400 bg-blue-50 shadow-[0_0_0_1px_rgba(96,165,250,0.35)]"
                : isDark
                  ? "border-slate-800 bg-slate-950/50 hover:border-slate-700 hover:bg-slate-900"
                  : "border-gray-200 bg-white hover:border-gray-300 hover:bg-gray-50"
          )}
        >
          <div className="flex items-start justify-between gap-2">
            <div className="min-w-0 flex-1 text-left">
              <div className={classNames("truncate text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{taskTitle(task)}</div>
              <div className={classNames("mt-1 text-xs", mutedTextClass)}>{task.id}</div>
            </div>
            <div className="flex items-center gap-2">
              <span className={classNames("rounded-full border px-2 py-0.5 text-[11px] font-medium", statusTone(status, isDark))}>{status}</span>
              <button
                type="button"
                {...listeners}
                className={classNames(
                  "rounded-lg px-2 py-1 text-[11px] md:opacity-0 md:group-hover/task:opacity-100",
                  isDark ? "bg-slate-800 text-slate-300 hover:bg-slate-700" : "bg-gray-100 text-gray-600 hover:bg-gray-200"
                )}
                onClick={(event) => event.stopPropagation()}
                aria-label={tr("context.dragTask", "Drag task")}
                title={tr("context.dragTask", "Drag task")}
              >
                ⋮⋮
              </button>
            </div>
          </div>

          {taskOutcome(task) ? (
            <div className={classNames("mt-2 block w-full text-left line-clamp-3 text-xs", subtleTextClass)}>{taskOutcome(task)}</div>
          ) : null}

          <div className="mt-3 flex flex-wrap gap-1.5 text-[11px]">
            {task.assignee ? (
              <span className={classNames("rounded-full px-2 py-0.5", isDark ? "bg-slate-800 text-slate-200" : "bg-gray-100 text-gray-700")}>{task.assignee}</span>
            ) : (
              <span className={classNames("rounded-full px-2 py-0.5", isDark ? "bg-slate-800 text-slate-400" : "bg-gray-100 text-gray-500")}>{tr("context.unassigned", "Unassigned")}</span>
            )}
            {task.priority ? <span className={classNames("rounded-full px-2 py-0.5", isDark ? "bg-slate-800 text-slate-200" : "bg-gray-100 text-gray-700")}>{task.priority}</span> : null}
            {blocked ? <span className={classNames("rounded-full px-2 py-0.5", isDark ? "bg-rose-500/15 text-rose-200" : "bg-rose-50 text-rose-700")}>{tr("context.blocked", "Blocked")}</span> : null}
            {waiting && waiting !== "none" ? <span className={classNames("rounded-full px-2 py-0.5", isDark ? "bg-violet-500/15 text-violet-200" : "bg-violet-50 text-violet-700")}>{waitingLabel(waiting)}</span> : null}
            {handoff ? <span className={classNames("rounded-full px-2 py-0.5", isDark ? "bg-cyan-500/15 text-cyan-200" : "bg-cyan-50 text-cyan-700")}>{tr("context.handoffTo", "Handoff →")} {handoff}</span> : null}
          </div>

          <div className="mt-3 flex items-center gap-2 border-t pt-3" onClick={(event) => event.stopPropagation()}>
            <button type="button" onClick={() => void moveTaskToStatus(task, quickAction.next)} disabled={syncBusy} className={buttonSecondaryClass}>{quickAction.label}</button>
            <button type="button" onClick={() => selectTask(task)} className={buttonSecondaryClass}>{tr("context.edit", "Edit")}</button>
          </div>
        </div>
      </div>
    );
  };

  const ColumnDropZone = ({ columnKey, label, items }: { columnKey: BoardStatus; label: string; items: Task[] }) => {
    const { setNodeRef, isOver } = useDroppable({ id: `column:${columnKey}`, data: { type: "column", status: columnKey } });
    return (
      <section ref={setNodeRef} className={classNames(
        "rounded-2xl border p-3 transition-all",
        isOver
          ? isDark ? "border-blue-500 bg-blue-500/5 shadow-[0_0_0_1px_rgba(59,130,246,0.25)]" : "border-blue-300 bg-blue-50/70 shadow-[0_0_0_1px_rgba(96,165,250,0.2)]"
          : isDark ? "border-slate-800 bg-slate-950/30" : "border-gray-200 bg-gray-50/70"
      )}>
        <div className="flex items-center justify-between gap-2">
          <div className="min-w-0">
            <div className={classNames("text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{label}</div>
            <div className={classNames("mt-1 text-xs", mutedTextClass)}>{items.length} {tr("context.items", "items")}</div>
          </div>
          <span className={classNames("rounded-full px-2 py-0.5 text-[11px]", isDark ? "bg-slate-800 text-slate-300" : "bg-white text-gray-600")}>{items.length}</span>
        </div>
        <div className="mt-3">
          <div className="space-y-2">
            {items.length > 0 ? items.map((task) => <TaskCard key={task.id} task={task} />) : (
              <div className={classNames("rounded-lg border border-dashed px-3 py-5 text-xs", isDark ? "border-slate-800 text-slate-500" : "border-gray-200 text-gray-400")}>
                {tr(`context.empty.${columnKey}`, "No tasks here")}
              </div>
            )}
          </div>
        </div>
      </section>
    );
  };

  const confirmDiscardTaskChanges = useCallback(() => {
    if (!hasTaskUnsaved || typeof window === "undefined") return true;
    return window.confirm(tr("context.unsavedTaskConfirm", "You have unsaved task edits. Discard them and continue?"));
  }, [hasTaskUnsaved, tr]);

  const selectTask = (task: Task) => {
    if (selectedTaskId === task.id && taskEditorMode === "edit") return;
    if (!confirmDiscardTaskChanges()) return;
    setSelectedTaskId(task.id);
    setTaskDraft(taskToDraft(task));
    setTaskEditorMode("edit");
    setSyncError("");
    setActiveView("coordination");
  };

  const closeTaskEditor = () => {
    if (!confirmDiscardTaskChanges()) return;
    setTaskEditorMode("none");
    setSelectedTaskId("");
    setTaskDraft(null);
    setSyncError("");
  };

  const openSteeringTab = useCallback((tab: "summary" | "project" | "log") => {
    if (!confirmDiscardTaskChanges()) return;
    setTaskEditorMode("none");
    setActiveView("coordination");
    setSteeringTab(tab);
    setSelectedTaskId("");
    setTaskDraft(null);
    setSyncError("");
    setProjectNotice("");
    setProjectError("");
    setNotifyError("");
    setActivityError("");
    if (tab === "project") {
      void loadProjectMd();
    }
  }, [confirmDiscardTaskChanges, loadProjectMd]);

  const handleSwitchActiveView = useCallback((next: "coordination" | "agents") => {
    if (next === "agents") {
      if (!confirmDiscardTaskChanges()) return;
      setTaskEditorMode("none");
      setSelectedTaskId("");
      setTaskDraft(null);
    }
    setActiveView(next);
  }, [confirmDiscardTaskChanges]);

  const handleModalClose = useCallback(() => {
    if (typeof window !== "undefined") {
      if (hasSteeringUnsaved && hasTaskUnsaved) {
        const ok = window.confirm(tr("context.unsavedCloseConfirm", "You have unsaved project and task changes. Discard them and close?"));
        if (!ok) return;
      } else if (hasSteeringUnsaved) {
        const ok = window.confirm(tr("context.unsavedChangesConfirm", "You have unsaved changes in project steering. Discard them and close?"));
        if (!ok) return;
      } else if (hasTaskUnsaved) {
        const ok = window.confirm(tr("context.unsavedTaskConfirm", "You have unsaved task edits. Discard them and continue?"));
        if (!ok) return;
      }
    }
    onClose();
  }, [hasSteeringUnsaved, hasTaskUnsaved, onClose, tr]);

  const { modalRef } = useModalA11y(isOpen, handleModalClose);

  const handleSaveBrief = async () => {
    if (!groupId) return;
    setSyncBusy(true);
    setSyncError("");
    try {
      const resp = await updateCoordinationBrief(groupId, {
        objective: briefDraft.objective,
        current_focus: briefDraft.currentFocus,
        constraints: parseLineList(briefDraft.constraints),
        project_brief: briefDraft.projectBrief,
        project_brief_stale: briefDraft.projectBriefStale,
      });
      if (!resp.ok) {
        setSyncError(resp.error?.message || tr("context.failedToApplyChanges", "Failed to apply changes"));
        return;
      }
      await onRefreshContext();
      setEditingBrief(false);
    } finally {
      setSyncBusy(false);
    }
  };

  const handleSaveTask = async () => {
    if (!groupId || !taskDraft) return;
    const title = taskDraft.title.trim();
    if (!title) {
      setSyncError(tr("context.taskTitleRequired", "Task title is required."));
      return;
    }

    setSyncBusy(true);
    setSyncError("");
    try {
      if (taskEditorMode === "create") {
        const resp = await contextSync(groupId, [{
          op: "task.create",
          title,
          outcome: taskDraft.outcome,
          status: taskDraft.status || "planned",
          assignee: taskDraft.assignee.trim() || null,
          priority: taskDraft.priority.trim() || null,
          parent_id: taskDraft.parentId.trim() || null,
          blocked_by: parseLineList(taskDraft.blockedBy),
          waiting_on: taskDraft.waitingOn,
          handoff_to: taskDraft.handoffTo.trim() || null,
          notes: taskDraft.notes,
          checklist: parseChecklist(taskDraft.checklist, []),
        }]);
        if (!resp.ok) {
          setSyncError(resp.error?.message || tr("context.failedToApplyChanges", "Failed to apply changes"));
          return;
        }
        await onRefreshContext();
        setTaskEditorMode("none");
        setSelectedTaskId("");
        setTaskDraft(null);
        return;
      }

      if (!selectedTask) return;
      const resp = await updateCoordinationTask(groupId, {
        ...selectedTask,
        title,
        outcome: taskDraft.outcome,
        status: taskDraft.status,
        assignee: taskDraft.assignee.trim() || null,
        priority: taskDraft.priority.trim() || null,
        parent_id: taskDraft.parentId.trim() || null,
        blocked_by: parseLineList(taskDraft.blockedBy),
        waiting_on: taskDraft.waitingOn,
        handoff_to: taskDraft.handoffTo.trim() || null,
        notes: taskDraft.notes,
        checklist: parseChecklist(taskDraft.checklist, selectedTask.checklist),
      });
      if (!resp.ok) {
        setSyncError(resp.error?.message || tr("context.failedToApplyChanges", "Failed to apply changes"));
        return;
      }
      await onRefreshContext();
    } finally {
      setSyncBusy(false);
    }
  };

  const handleResetTask = () => {
    if (taskEditorMode === "create") {
      setTaskDraft(emptyTaskDraft("planned"));
      setSyncError("");
      return;
    }
    if (!selectedTask) return;
    setTaskDraft(taskToDraft(selectedTask));
    setSyncError("");
  };

  const handleOpenCreate = (status: BoardStatus = "planned") => {
    if (!confirmDiscardTaskChanges()) return;
    setTaskEditorMode("create");
    setSelectedTaskId("");
    setTaskDraft(emptyTaskDraft(status));
    setSyncError("");
    setActiveView("coordination");
  };

  const handleEditProject = async () => {
    openSteeringTab("project");
    const loaded = await loadProjectMd(true);
    setProjectText(String(loaded?.content || ""));
    setEditingProject(true);
  };

  const handleSaveProject = async () => {
    if (!groupId) return;
    setProjectBusy(true);
    setProjectError("");
    setProjectNotice("");
    setNotifyError("");
    try {
      const resp = await apiJson<ProjectMdInfo>(`/api/v1/groups/${encodeURIComponent(groupId)}/project_md`, {
        method: "PUT",
        body: JSON.stringify({ content: projectText, by: "user" }),
      });
      if (!resp.ok) {
        setProjectError(resp.error?.message || tr("context.failedToSaveProject", "Failed to save PROJECT.md"));
        return;
      }
      setProjectMd(resp.result);
      setEditingProject(false);
      await onRefreshContext();

      let notice = tr("context.projectSaved", "PROJECT.md saved.");
      if (notifyAgents) {
        const notifyResp = await apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/send`, {
          method: "POST",
          body: JSON.stringify({ text: notifyMessage, by: "user", to: ["@all"], path: "" }),
        });
        if (!notifyResp.ok) {
          setNotifyError(notifyResp.error?.message || tr("context.failedToNotify", "Failed to notify agents"));
          notice = tr("context.projectSavedNotifyFailed", "PROJECT.md saved, but chat notification failed.");
        } else {
          notice = tr("context.projectSavedAndNotified", "PROJECT.md saved and the team was notified in chat.");
        }
      }
      setProjectNotice(notice);
      setNotifyAgents(false);
    } finally {
      setProjectBusy(false);
    }
  };

  const handleAddCoordinationNote = async (kind: "decision" | "handoff") => {
    if (!groupId) return;
    const draft = kind === "decision" ? decisionDraft : handoffDraft;
    const summary = String(draft.summary || "").trim();
    if (!summary) {
      setActivityError(tr("context.noteSummaryRequired", "Summary is required."));
      return;
    }
    setActivityBusyKind(kind);
    setActivityError("");
    try {
      const resp = await addCoordinationNote(groupId, kind, summary, String(draft.taskId || "").trim() || null);
      if (!resp.ok) {
        setActivityError(resp.error?.message || tr("context.failedToApplyChanges", "Failed to apply changes"));
        return;
      }
      await onRefreshContext();
      if (kind === "decision") {
        setDecisionDraft(emptyNoteDraft());
      } else {
        setHandoffDraft(emptyNoteDraft());
      }
    } finally {
      setActivityBusyKind(null);
    }
  };

  const renderTaskEditorPanel = () => {
    if (!taskEditorVisible || !taskDraft) {
      return (
        <section className={classNames(surfaceClass, "hidden xl:flex xl:min-h-[520px] xl:flex-col xl:justify-center xl:p-6") }>
          <div className={classNames("text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{tr("context.taskEditorEmptyTitle", "Select a task")}</div>
          <div className={classNames("mt-2 text-sm", subtleTextClass)}>{tr("context.taskEditorEmptyHint", "Pick a card to edit, or create a new task from the button above.")}</div>
        </section>
      );
    }

    const isCreate = taskEditorMode === "create";
    return (
      <section className={classNames(surfaceClass, "p-4 xl:sticky xl:top-0 xl:max-h-[calc(94vh-8rem)] xl:overflow-y-auto")}>
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className={classNames("text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{isCreate ? tr("context.newTask", "New task") : tr("context.taskDetails", "Task editor")}</div>
            <div className={classNames("mt-1 text-xs", mutedTextClass)}>{isCreate ? tr("context.newTaskHint", "Create a new shared task. Keep it lean; fill advanced fields only when they help coordination.") : (selectedTask?.id || "")}</div>
          </div>
          <div className="flex items-center gap-2">
            {hasTaskUnsaved ? <span className={classNames("rounded-full px-2 py-0.5 text-[11px] font-medium", isDark ? "bg-amber-500/15 text-amber-200" : "bg-amber-50 text-amber-700")}>{tr("context.unsaved", "Unsaved")}</span> : null}
            <button type="button" onClick={handleResetTask} disabled={syncBusy} className={buttonSecondaryClass}>{isCreate ? tr("context.clear", "Clear") : tr("context.reset", "Reset")}</button>
            <button type="button" onClick={closeTaskEditor} className={buttonSecondaryClass}>{tr("context.close", "Close")}</button>
          </div>
        </div>

        <div className="mt-4 space-y-3">
          <div className="flex items-center justify-between gap-2">
            <div className={classNames("text-xs", mutedTextClass)}>
              {isCreate ? tr("context.editorInlineHint", "Editing stays inside the Tasks workspace so you can keep the board in view.") : (selectedTask?.updated_at ? `${tr("context.updated", "Updated {{time}}", { time: formatTime(selectedTask.updated_at) })}` : tr("context.notUpdatedYet", "Not updated yet"))}
            </div>
            <span className={classNames("rounded-full border px-2 py-0.5 text-[11px] font-medium", statusTone(taskDraft.status, isDark))}>{taskDraft.status}</span>
          </div>

          <label className="block text-sm">
            <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.titleField", "Title")}</span>
            <input value={taskDraft.title} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, title: event.target.value } : prev)} className={inputClass} />
          </label>

          <label className="block text-sm">
            <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.outcome", "Outcome")}</span>
            <textarea value={taskDraft.outcome} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, outcome: event.target.value } : prev)} className={textareaClass} />
          </label>

          <div className="grid gap-3 sm:grid-cols-2">
            <label className="block text-sm">
              <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.status", "Status")}</span>
              <select value={taskDraft.status} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, status: event.target.value } : prev)} className={inputClass}>
                <option value="planned">{tr("context.planned", "Planned")}</option>
                <option value="active">{tr("context.active", "Active")}</option>
                <option value="done">{tr("context.done", "Done")}</option>
                <option value="archived">{tr("context.archived", "Archived")}</option>
              </select>
            </label>
            <label className="block text-sm">
              <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.assignee", "Assignee")}</span>
              <input value={taskDraft.assignee} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, assignee: event.target.value } : prev)} className={inputClass} />
            </label>
          </div>

          <label className="block text-sm">
            <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.priority", "Priority")}</span>
            <input value={taskDraft.priority} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, priority: event.target.value } : prev)} className={inputClass} />
          </label>

          <details className={classNames("rounded-xl border px-3 py-3", isDark ? "border-slate-800 bg-slate-950/40" : "border-gray-200 bg-gray-50") }>
            <summary className={classNames("cursor-pointer text-sm font-medium", isDark ? "text-slate-100" : "text-gray-900")}>{tr("context.advancedTaskFields", "Advanced details")}</summary>
            <div className="mt-3 space-y-3">
              <div className="grid gap-3 sm:grid-cols-2">
                <label className="block text-sm">
                  <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.parentTask", "Parent task")}</span>
                  <input value={taskDraft.parentId} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, parentId: event.target.value } : prev)} className={inputClass} placeholder={tr("context.rootTask", "Root task")} />
                </label>
                <label className="block text-sm">
                  <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.handoffTo", "Handoff to")}</span>
                  <input value={taskDraft.handoffTo} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, handoffTo: event.target.value } : prev)} className={inputClass} />
                </label>
              </div>

              <div className="grid gap-3 sm:grid-cols-2">
                <label className="block text-sm">
                  <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.blockedBy", "Blocked by")}</span>
                  <textarea value={taskDraft.blockedBy} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, blockedBy: event.target.value } : prev)} className={classNames(textareaClass, "min-h-[90px]")} placeholder={tr("context.onePerLine", "One per line")} />
                </label>
                <label className="block text-sm">
                  <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.waitingOn", "Waiting on")}</span>
                  <select value={taskDraft.waitingOn} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, waitingOn: event.target.value as TaskWaitingOn } : prev)} className={inputClass}>
                    {WAITING_ON_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
                  </select>
                </label>
              </div>

              <label className="block text-sm">
                <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.notes", "Notes")}</span>
                <textarea value={taskDraft.notes} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, notes: event.target.value } : prev)} className={classNames(textareaClass, "min-h-[100px]")} />
              </label>

              <label className="block text-sm">
                <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.checklist", "Checklist")}</span>
                <textarea value={taskDraft.checklist} onChange={(event) => setTaskDraft((prev) => prev ? { ...prev, checklist: event.target.value } : prev)} className={classNames(textareaClass, "min-h-[120px]")} placeholder={tr("context.checklistPlaceholder", "Use [ ], [~], [x] prefixes if useful.")} />
              </label>
            </div>
          </details>

          {isCreate ? (
            <div className={classNames("rounded-xl border px-3 py-3 text-xs", isDark ? "border-slate-800 bg-slate-950/40 text-slate-400" : "border-gray-200 bg-gray-50 text-gray-500")}>
              {tr("context.newTaskHint", "Create a new shared task. Keep it lean; fill advanced fields only when they help coordination.")}
            </div>
          ) : selectedTask ? (
            <div className={classNames("grid gap-2 rounded-xl border px-3 py-3 text-xs sm:grid-cols-2", isDark ? "border-slate-800 bg-slate-950/40 text-slate-400" : "border-gray-200 bg-gray-50 text-gray-500")}>
              <div>{tr("context.createdAt", "Created")}: {selectedTask.created_at ? formatFullTime(selectedTask.created_at) : "-"}</div>
              <div>{tr("context.updatedAt", "Updated")}: {selectedTask.updated_at ? formatFullTime(selectedTask.updated_at) : "-"}</div>
              <div>{tr("context.archivedFrom", "Archived from")}: {selectedTask.archived_from || "-"}</div>
              <div>{tr("context.checklistItems", "Checklist items")}: {Array.isArray(selectedTask.checklist) ? selectedTask.checklist.length : 0}</div>
            </div>
          ) : null}

          <div className="flex items-center gap-2">
            <button type="button" onClick={() => void handleSaveTask()} disabled={syncBusy} className={buttonPrimaryClass}>{syncBusy ? tr("context.saving", "Saving…") : (isCreate ? tr("context.createTask", "Create task") : tr("context.saveTask", "Save task"))}</button>
          </div>
        </div>
      </section>
    );
  };

  const renderSteeringPanel = () => {
    const tabButtonClass = (active: boolean) => classNames(
      "rounded-lg px-3 py-2 text-sm font-medium transition-colors",
      active
        ? isDark
          ? "bg-blue-500/15 text-blue-200"
          : "bg-white text-blue-700 shadow-sm"
        : isDark
          ? "text-slate-300 hover:bg-slate-800"
          : "text-gray-600 hover:bg-white/80"
    );
    const notesCardClass = classNames("rounded-xl border p-3 text-sm", isDark ? "border-slate-800 bg-slate-950/70" : "border-gray-200 bg-gray-50");

    return (
      <section className={classNames(surfaceClass, "p-4")}>
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
            <div className="min-w-0 flex-1">
              <div className={classNames("text-lg font-semibold", isDark ? "text-slate-50" : "text-gray-900")}>{brief?.objective || tr("context.noObjective", "No objective set")}</div>
              <div className={classNames("mt-1 text-sm", subtleTextClass)}>{brief?.current_focus || tr("context.noCurrentFocus", "No current focus set")}</div>
              {(brief?.project_brief_stale || (Array.isArray(brief?.constraints) && brief.constraints.length > 0)) ? (
                <div className="mt-3 flex flex-wrap gap-1.5">
                  {brief?.project_brief_stale ? <button type="button" onClick={() => { openSteeringTab("summary"); setEditingBrief(true); }} className={classNames("rounded-full px-2 py-1 text-[11px] transition-colors", isDark ? "bg-amber-500/15 text-amber-200 hover:bg-amber-500/25" : "bg-amber-50 text-amber-700 hover:bg-amber-100")}>{tr("context.projectBriefNeedsRefresh", "Summary needs refresh")}</button> : null}
                  {(brief?.constraints || []).slice(0, 6).map((constraint, index) => <span key={`${constraint}-${index}`} className={classNames("rounded-full px-2 py-1 text-[11px]", isDark ? "bg-slate-800 text-slate-200" : "bg-gray-100 text-gray-700")}>{constraint}</span>)}
                </div>
              ) : null}
            </div>
            <div className="flex flex-wrap items-center gap-2 xl:max-w-[18rem] xl:justify-end">
              <span className={classNames("rounded-full px-2.5 py-1 text-xs", isDark ? "bg-blue-500/15 text-blue-200" : "bg-blue-50 text-blue-700")}>{tr("context.active", "Active")} · {Number(tasksSummary.active || 0)}</span>
              <span className={classNames("rounded-full px-2.5 py-1 text-xs", isDark ? "bg-rose-500/15 text-rose-200" : "bg-rose-50 text-rose-700")}>{tr("context.blocked", "Blocked")} · {attentionCounts.blocked}</span>
              <span className={classNames("rounded-full px-2.5 py-1 text-xs", isDark ? "bg-violet-500/15 text-violet-200" : "bg-violet-50 text-violet-700")}>{tr("context.waitingUser", "Waiting user")} · {attentionCounts.waitingUser}</span>
              <span className={classNames("rounded-full px-2.5 py-1 text-xs", isDark ? "bg-slate-800 text-slate-300" : "bg-gray-100 text-gray-700")}>{tr("context.unassigned", "Unassigned")} · {unassignedCount}</span>
            </div>
          </div>

          <div className={classNames("flex flex-col gap-3 border-t pt-4", isDark ? "border-slate-800" : "border-gray-200")}>
            <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
              <div>
                <div className={classNames("text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{tr("context.steering", "Project steering")}</div>
                <div className={classNames("mt-1 text-xs", mutedTextClass)}>{tr("context.projectSteeringHint", "Steer the project here. Keep PROJECT.md as the full repository reference, and keep the working summary hot and short.")}</div>
              </div>
            </div>

            <div className={classNames("inline-flex w-fit rounded-2xl border p-1", isDark ? "border-slate-800 bg-slate-950/70" : "border-gray-200 bg-gray-100/80")}>
              <button type="button" onClick={() => openSteeringTab("summary")} className={tabButtonClass(steeringTab === "summary")}>{tr("context.brief", "Summary")}</button>
              <button type="button" onClick={() => openSteeringTab("project")} className={tabButtonClass(steeringTab === "project")}>{t("context.projectMd", { defaultValue: "PROJECT.md" })}</button>
              <button type="button" onClick={() => openSteeringTab("log")} className={tabButtonClass(steeringTab === "log")}>{tr("context.activityLog", "Activity")}</button>
            </div>

            {steeringTab === "summary" ? (
              <section className={classNames("rounded-xl border p-4", isDark ? "border-slate-800 bg-slate-950/40" : "border-gray-200 bg-gray-50")}>
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <div className={classNames("text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{tr("context.brief", "Summary")}</div>
                    <div className={classNames("mt-1 text-xs", mutedTextClass)}>{brief?.updated_at ? `${tr("context.updated", "Updated {{time}}", { time: formatTime(brief.updated_at) })}` : tr("context.notUpdatedYet", "Not updated yet")}</div>
                  </div>
                  <div className="flex items-center gap-2">
                    {editingBrief ? <span className={classNames("rounded-full px-2 py-0.5 text-[11px] font-medium", isDark ? "bg-amber-500/15 text-amber-200" : "bg-amber-50 text-amber-700")}>{tr("context.unsaved", "Unsaved")}</span> : null}
                    {editingBrief ? (
                      <>
                        <button type="button" onClick={() => { setEditingBrief(false); setBriefDraft(briefToDraft(brief)); }} disabled={syncBusy} className={buttonSecondaryClass}>{tr("context.cancel", "Cancel")}</button>
                        <button type="button" onClick={() => void handleSaveBrief()} disabled={syncBusy} className={buttonPrimaryClass}>{syncBusy ? tr("context.saving", "Saving…") : tr("context.saveBrief", "Save summary")}</button>
                      </>
                    ) : (
                      <button type="button" onClick={() => { openSteeringTab("summary"); setEditingBrief(true); }} className={buttonPrimaryClass}>{t("context.editButton", { defaultValue: "Edit" })}</button>
                    )}
                  </div>
                </div>

                {editingBrief ? (
                  <div className="mt-4 grid gap-3 lg:grid-cols-2">
                    <label className="block text-sm lg:col-span-1">
                      <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.objective", "Objective")}</span>
                      <input value={briefDraft.objective} onChange={(event) => setBriefDraft((prev) => ({ ...prev, objective: event.target.value }))} className={inputClass} />
                    </label>
                    <label className="block text-sm lg:col-span-1">
                      <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.currentFocus", "Current focus")}</span>
                      <input value={briefDraft.currentFocus} onChange={(event) => setBriefDraft((prev) => ({ ...prev, currentFocus: event.target.value }))} className={inputClass} />
                    </label>
                    <label className="block text-sm lg:col-span-2">
                      <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.constraints", "Constraints")}</span>
                      <textarea value={briefDraft.constraints} onChange={(event) => setBriefDraft((prev) => ({ ...prev, constraints: event.target.value }))} className={classNames(textareaClass, "min-h-[110px]")} placeholder={tr("context.constraintsPlaceholder", "One per line")} />
                    </label>
                    <label className="block text-sm lg:col-span-2">
                      <span className={classNames("mb-1 block text-xs font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.projectBrief", "Working summary")}</span>
                      <textarea value={briefDraft.projectBrief} onChange={(event) => setBriefDraft((prev) => ({ ...prev, projectBrief: event.target.value }))} className={classNames(textareaClass, "min-h-[180px]")} />
                    </label>
                    <label className={classNames("flex items-center gap-2 rounded-lg border px-3 py-2 text-sm lg:col-span-2", isDark ? "border-slate-800 bg-slate-900 text-slate-200" : "border-gray-200 bg-white text-gray-800")}>
                      <input type="checkbox" checked={briefDraft.projectBriefStale} onChange={(event) => setBriefDraft((prev) => ({ ...prev, projectBriefStale: event.target.checked }))} />
                      {tr("context.projectBriefStale", "Mark working summary as stale")}
                    </label>
                  </div>
                ) : (
                  <div className="mt-4 space-y-4">
                    <div>
                      <div className={classNames("text-[11px] font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.projectBrief", "Working summary")}</div>
                      <div className={classNames("mt-1 whitespace-pre-wrap text-sm", subtleTextClass)}>{brief?.project_brief || tr("context.noProjectBrief", "No working summary set")}</div>
                    </div>
                    <div>
                      <div className={classNames("text-[11px] font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.constraints", "Constraints")}</div>
                      <div className="mt-2 flex flex-wrap gap-1.5">
                        {Array.isArray(brief?.constraints) && brief.constraints.length > 0 ? brief.constraints.map((constraint, index) => (
                          <span key={`${constraint}-${index}`} className={classNames("rounded-full px-2 py-1 text-[11px]", isDark ? "bg-slate-800 text-slate-200" : "bg-white text-gray-700")}>{constraint}</span>
                        )) : <span className={mutedTextClass}>{tr("context.noConstraints", "No constraints set")}</span>}
                      </div>
                    </div>
                  </div>
                )}
              </section>
            ) : null}

            {steeringTab === "project" ? (
              <section className={classNames("rounded-xl border p-4", isDark ? "border-slate-800 bg-slate-950/40" : "border-gray-200 bg-gray-50")}>
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <div className={classNames("text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{t("context.projectMd", { defaultValue: "PROJECT.md" })}</div>
                    <div className={classNames("mt-1 text-xs", mutedTextClass)}>{projectBusy ? t("common:loading", { defaultValue: "Loading…" }) : projectPathLabel}</div>
                  </div>
                  <div className="flex items-center gap-2">
                    {editingProject ? <button type="button" onClick={() => { setEditingProject(false); setProjectText(String(projectMd?.content || "")); setNotifyAgents(false); }} disabled={projectBusy} className={buttonSecondaryClass}>{tr("context.cancel", "Cancel")}</button> : null}
                    <button type="button" onClick={() => void handleEditProject()} className={buttonPrimaryClass}>{editingProject ? tr("context.editing", "Editing") : (projectMd?.found ? t("context.editButton", { defaultValue: "Edit" }) : t("context.createButton", { defaultValue: "Create" }))}</button>
                  </div>
                </div>
                {projectError ? <div className={classNames("mt-3 rounded-lg border px-3 py-2 text-sm", isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-200" : "border-rose-200 bg-rose-50 text-rose-700")}>{projectError}</div> : null}
                {notifyError ? <div className={classNames("mt-3 rounded-lg border px-3 py-2 text-sm", isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-200" : "border-rose-200 bg-rose-50 text-rose-700")}>{notifyError}</div> : null}
                {projectNotice ? <div className={classNames("mt-3 rounded-lg border px-3 py-2 text-sm", isDark ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-200" : "border-emerald-200 bg-emerald-50 text-emerald-700")}>{projectNotice}</div> : null}
                <div className="mt-4">
                  {editingProject ? (
                    <>
                      <textarea value={projectText} onChange={(event) => setProjectText(event.target.value)} className={classNames(textareaClass, "min-h-[320px]")} />
                      <label className={classNames("mt-3 flex items-center gap-2 rounded-lg border px-3 py-2 text-sm", isDark ? "border-slate-800 bg-slate-900 text-slate-200" : "border-gray-200 bg-white text-gray-800")}>
                        <input type="checkbox" checked={notifyAgents} onChange={(event) => setNotifyAgents(event.target.checked)} />
                        {tr("context.notifyAgents", "Notify the team in chat (@all) after save")}
                      </label>
                      <div className="mt-3 flex items-center gap-2">
                        <button type="button" onClick={() => void handleSaveProject()} disabled={projectBusy} className={buttonPrimaryClass}>{projectBusy ? tr("context.saving", "Saving…") : tr("context.saveProject", "Save PROJECT.md")}</button>
                      </div>
                    </>
                  ) : projectMd?.found && projectMd.content ? (
                    <div className={classNames("max-h-[36rem] overflow-y-auto rounded-xl border p-3", isDark ? "border-slate-800 bg-slate-900/70" : "border-gray-200 bg-white")}>
                      <MarkdownRenderer content={String(projectMd.content)} isDark={isDark} className={classNames("text-sm", subtleTextClass)} />
                    </div>
                  ) : (
                    <div className={classNames("rounded-xl border border-dashed px-3 py-4 text-sm", isDark ? "border-slate-800 text-slate-500" : "border-gray-200 text-gray-400")}>{t("context.noProjectMd", { defaultValue: "No PROJECT.md found" })}</div>
                  )}
                </div>
              </section>
            ) : null}

            {steeringTab === "log" ? (
              <div className="space-y-3">
                {activityError ? <div className={classNames("rounded-xl border px-3 py-2 text-sm", isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-200" : "border-rose-200 bg-rose-50 text-rose-700")}>{activityError}</div> : null}
                <div className="grid gap-3 xl:grid-cols-2">
                  <section className={classNames("rounded-xl border p-4", isDark ? "border-slate-800 bg-slate-950/40" : "border-gray-200 bg-gray-50")}>
                    <div className="flex items-start justify-between gap-3">
                      <div>
                        <div className={classNames("text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{tr("context.recentDecisions", "Recent decisions")}</div>
                        <div className={classNames("mt-1 text-xs", mutedTextClass)}>{tr("context.activityWriteHint", "Capture durable decisions and handoffs here when chat alone is too transient.")}</div>
                      </div>
                    </div>
                    <div className={classNames("mt-3 rounded-xl border p-3", isDark ? "border-slate-800 bg-slate-950/60" : "border-gray-200 bg-white")}>
                      <textarea value={decisionDraft.summary} onChange={(event) => setDecisionDraft((prev) => ({ ...prev, summary: event.target.value }))} className={classNames(textareaClass, "min-h-[96px]")} placeholder={tr("context.decisionPlaceholder", "Record a durable project decision...")} />
                      <div className="mt-2 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
                        <select value={decisionDraft.taskId} onChange={(event) => setDecisionDraft((prev) => ({ ...prev, taskId: event.target.value }))} className={inputClass}>
                          <option value="">{tr("context.noRelatedTask", "No related task")}</option>
                          {activeTaskOptions.map((task) => <option key={task.id} value={task.id}>{taskOptionLabel(task)}</option>)}
                        </select>
                        <button type="button" onClick={() => void handleAddCoordinationNote("decision")} disabled={activityBusyKind !== null || !String(decisionDraft.summary || "").trim()} className={buttonPrimaryClass}>{activityBusyKind === "decision" ? tr("context.saving", "Saving…") : tr("context.addDecision", "Add decision")}</button>
                      </div>
                    </div>
                    <div className="mt-3 space-y-2">
                      {recentDecisions.length > 0 ? recentDecisions.map((note, index) => (
                        <div key={`${note.summary}-${index}`} className={notesCardClass}>
                          <div className={classNames("font-medium", isDark ? "text-slate-100" : "text-gray-900")}>{note.summary}</div>
                          <div className={classNames("mt-1 text-xs", mutedTextClass)}>{[note.by || "", note.task_id || "", noteTimestamp(note)].filter(Boolean).join(" · ") || tr("context.noMetadata", "No metadata")}</div>
                        </div>
                      )) : <div className={classNames("text-sm", mutedTextClass)}>{tr("context.noRecentDecisions", "No recent decisions")}</div>}
                    </div>
                  </section>
                  <section className={classNames("rounded-xl border p-4", isDark ? "border-slate-800 bg-slate-950/40" : "border-gray-200 bg-gray-50")}>
                    <div className={classNames("text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{tr("context.recentHandoffs", "Recent handoffs")}</div>
                    <div className={classNames("mt-3 rounded-xl border p-3", isDark ? "border-slate-800 bg-slate-950/60" : "border-gray-200 bg-white")}>
                      <textarea value={handoffDraft.summary} onChange={(event) => setHandoffDraft((prev) => ({ ...prev, summary: event.target.value }))} className={classNames(textareaClass, "min-h-[96px]")} placeholder={tr("context.handoffPlaceholder", "Record a durable handoff or next-owner note...")} />
                      <div className="mt-2 grid gap-2 sm:grid-cols-[minmax(0,1fr)_auto]">
                        <select value={handoffDraft.taskId} onChange={(event) => setHandoffDraft((prev) => ({ ...prev, taskId: event.target.value }))} className={inputClass}>
                          <option value="">{tr("context.noRelatedTask", "No related task")}</option>
                          {activeTaskOptions.map((task) => <option key={task.id} value={task.id}>{taskOptionLabel(task)}</option>)}
                        </select>
                        <button type="button" onClick={() => void handleAddCoordinationNote("handoff")} disabled={activityBusyKind !== null || !String(handoffDraft.summary || "").trim()} className={buttonPrimaryClass}>{activityBusyKind === "handoff" ? tr("context.saving", "Saving…") : tr("context.addHandoff", "Add handoff")}</button>
                      </div>
                    </div>
                    <div className="mt-3 space-y-2">
                      {recentHandoffs.length > 0 ? recentHandoffs.map((note, index) => (
                        <div key={`${note.summary}-${index}`} className={notesCardClass}>
                          <div className={classNames("font-medium", isDark ? "text-slate-100" : "text-gray-900")}>{note.summary}</div>
                          <div className={classNames("mt-1 text-xs", mutedTextClass)}>{[note.by || "", note.task_id || "", noteTimestamp(note)].filter(Boolean).join(" · ") || tr("context.noMetadata", "No metadata")}</div>
                        </div>
                      )) : <div className={classNames("text-sm", mutedTextClass)}>{tr("context.noRecentHandoffs", "No recent handoffs")}</div>}
                    </div>
                  </section>
                </div>
              </div>
            ) : null}
          </div>
        </div>
      </section>
    );
  };

  const renderAgentsView = () => {
    const agentsWithBlockers = agents.filter((agent) => agentHot(agent).blockers.length > 0).length;
    const agentsWithActiveTask = agents.filter((agent) => !!agentHot(agent).activeTaskId).length;
    return (
      <section className={classNames(surfaceClass, "p-4")}>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <div className={classNames("text-lg font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{tr("context.agents", "Agents")}</div>
            <div className={classNames("mt-1 text-sm", subtleTextClass)}>{tr("context.agentsHint", "Use this view to recover each agent’s current execution state, not to steer the whole project.")}</div>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            <span className={classNames("rounded-full px-2.5 py-1 text-xs", isDark ? "bg-slate-800 text-slate-300" : "bg-gray-100 text-gray-700")}>{tr("context.totalAgents", "{{count}} agents", { count: agents.length })}</span>
            <span className={classNames("rounded-full px-2.5 py-1 text-xs", isDark ? "bg-blue-500/15 text-blue-200" : "bg-blue-50 text-blue-700")}>{tr("context.activeTasksCount", "{{count}} with active task", { count: agentsWithActiveTask })}</span>
            {agentsWithBlockers > 0 ? <span className={classNames("rounded-full px-2.5 py-1 text-xs", isDark ? "bg-rose-500/15 text-rose-200" : "bg-rose-50 text-rose-700")}>{tr("context.blockersCount", "{{count}} blockers", { count: agentsWithBlockers })}</span> : null}
          </div>
        </div>
        <div className="mt-4 grid gap-3 xl:grid-cols-2">
          {agents.length > 0 ? agents.map((agent) => {
            const hot = agentHot(agent);
            const warm = agentWarm(agent);
            return (
              <div key={agent.id} className={classNames("rounded-xl border p-4", isDark ? "border-slate-800 bg-slate-950/40" : "border-gray-200 bg-gray-50")}>
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <div className={classNames("text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{agent.id}</div>
                    <div className={classNames("mt-1 text-xs", mutedTextClass)}>{agent.updated_at ? `${tr("context.updated", "Updated {{time}}", { time: formatTime(agent.updated_at) })}` : tr("context.notUpdatedYet", "Not updated yet")}</div>
                  </div>
                  {hot.activeTaskId ? <span className={classNames("rounded-full px-2 py-0.5 text-[11px]", isDark ? "bg-blue-500/15 text-blue-200" : "bg-blue-50 text-blue-700")}>{hot.activeTaskId}</span> : null}
                </div>

                <div className="mt-3 grid gap-3 sm:grid-cols-2">
                  <div className={classNames("rounded-lg border p-3", isDark ? "border-slate-800 bg-slate-900/60" : "border-gray-200 bg-white")}>
                    <div className={classNames("text-[11px] font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.focus", "Focus")}</div>
                    <div className={classNames("mt-1 text-sm line-clamp-3", subtleTextClass)}>{hot.focus || tr("context.none", "None")}</div>
                  </div>
                  <div className={classNames("rounded-lg border p-3", isDark ? "border-slate-800 bg-slate-900/60" : "border-gray-200 bg-white")}>
                    <div className={classNames("text-[11px] font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.nextAction", "Next action")}</div>
                    <div className={classNames("mt-1 text-sm line-clamp-3", subtleTextClass)}>{hot.nextAction || tr("context.none", "None")}</div>
                  </div>
                  <div className={classNames("rounded-lg border p-3 sm:col-span-2", isDark ? "border-slate-800 bg-slate-900/60" : "border-gray-200 bg-white")}>
                    <div className={classNames("text-[11px] font-medium uppercase tracking-wide", mutedTextClass)}>{tr("context.activeTask", "Active task")}</div>
                    <div className={classNames("mt-1 text-sm", subtleTextClass)}>{hot.activeTaskId || tr("context.none", "None")}</div>
                  </div>
                  {hot.blockers.length > 0 ? (
                    <div className={classNames("rounded-lg border px-3 py-3 text-sm sm:col-span-2", isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-200" : "border-rose-200 bg-rose-50 text-rose-700")}>
                      <span className="font-medium">{tr("context.blockers", "Blockers")}: </span>{hot.blockers.join(" · ")}
                    </div>
                  ) : null}
                </div>

                {hasWarmState(agent) ? (
                  <details className={classNames("mt-3 rounded-lg border px-3 py-2", isDark ? "border-slate-800 bg-slate-900/40" : "border-gray-200 bg-white")} open={false}>
                    <summary className={classNames("cursor-pointer text-xs font-medium", isDark ? "text-slate-200" : "text-gray-700")}>{tr("context.warmState", "Warm state")}</summary>
                    <div className="mt-2 space-y-2 text-xs">
                      {warm.whatChanged ? <div className={subtleTextClass}><span className={mutedTextClass}>{tr("context.whatChanged", "What changed")}: </span>{warm.whatChanged}</div> : null}
                      {warm.openLoops.length > 0 ? <div className={subtleTextClass}><span className={mutedTextClass}>{tr("context.openLoops", "Open loops")}: </span>{warm.openLoops.join(" · ")}</div> : null}
                      {warm.commitments.length > 0 ? <div className={subtleTextClass}><span className={mutedTextClass}>{tr("context.commitments", "Commitments")}: </span>{warm.commitments.join(" · ")}</div> : null}
                      {warm.environmentSummary ? <div className={subtleTextClass}><span className={mutedTextClass}>{tr("context.environmentSummary", "Environment")}: </span>{warm.environmentSummary}</div> : null}
                      {warm.userModel ? <div className={subtleTextClass}><span className={mutedTextClass}>{tr("context.userModel", "User model")}: </span>{warm.userModel}</div> : null}
                      {warm.personaNotes ? <div className={subtleTextClass}><span className={mutedTextClass}>{tr("context.personaNotes", "Persona notes")}: </span>{warm.personaNotes}</div> : null}
                      {warm.resumeHint ? <div className={subtleTextClass}><span className={mutedTextClass}>{tr("context.resumeHint", "Resume hint")}: </span>{warm.resumeHint}</div> : null}
                    </div>
                  </details>
                ) : null}
              </div>
            );
          }) : <div className={classNames("rounded-xl border border-dashed px-3 py-4 text-sm", isDark ? "border-slate-800 text-slate-500" : "border-gray-200 text-gray-400")}>{tr("context.noAgents", "No agent state")}</div>}
        </div>
      </section>
    );
  };

  if (!isOpen) return null;

  const viewButtonClass = (active: boolean) => classNames(
    "rounded-xl px-3 py-2 text-sm font-medium transition-colors",
    active
      ? isDark
        ? "bg-blue-500/15 text-blue-200"
        : "bg-white text-blue-700 shadow-sm"
      : isDark
        ? "text-slate-300 hover:bg-slate-800"
        : "text-gray-600 hover:bg-white/80"
  );

  const renderCoordinationView = () => {
    return (
      <div className="space-y-4">
        {renderSteeringPanel()}

        <section className={classNames(surfaceClass, "p-4")}>
          <div className="flex flex-col gap-4">
            <div className="flex flex-col gap-3 xl:flex-row xl:items-start xl:justify-between">
              <div>
                <div className={classNames("text-lg font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>{tr("context.tasks", "Tasks")}</div>
                <div className={classNames("mt-1 text-sm", subtleTextClass)}>{tr("context.taskBoardHint", "Plan shared work here. Open a card only when you need blockers, handoffs, notes, or checklist detail.")}</div>
              </div>
              <div className="flex items-center gap-2">
                <button type="button" onClick={() => handleOpenCreate("planned")} className={buttonPrimaryClass}>{tr("context.newTask", "New task")}</button>
              </div>
            </div>

            <div className={classNames("flex flex-col gap-3 border-t pt-4", isDark ? "border-slate-800" : "border-gray-200")}>
              <div className="flex flex-col gap-3 xl:flex-row xl:items-center xl:justify-between">
                <div className="grid flex-1 gap-3 lg:grid-cols-[minmax(0,1fr)_auto_auto]">
                  <input
                    value={taskQuery}
                    onChange={(event) => setTaskQuery(event.target.value)}
                    className={inputClass}
                    placeholder={tr("context.searchTasks", "Search tasks by title, id, assignee, or outcome")}
                  />
                  <select value={assigneeFilter} onChange={(event) => setAssigneeFilter(event.target.value)} className={classNames(inputClass, "w-full lg:w-[14rem]")}>
                    <option value="__all__">{tr("context.allAssignees", "All assignees")}</option>
                    <option value="__unassigned__">{tr("context.unassignedOnly", "Unassigned only")}</option>
                    {assigneeOptions.map((assignee) => <option key={assignee} value={assignee}>{assignee}</option>)}
                  </select>
                  <button type="button" onClick={() => { setTaskQuery(""); setTaskFilter("all"); setAssigneeFilter("__all__"); }} className={buttonSecondaryClass}>{tr("context.clearFilters", "Clear filters")}</button>
                </div>
              </div>

              <div className="flex flex-wrap items-center gap-2">
                {[
                  ["all", tr("context.all", "All"), Number(tasksSummary.total || 0)],
                  ["blocked", tr("context.blocked", "Blocked"), attentionCounts.blocked],
                  ["waiting_user", tr("context.waitingUser", "Waiting user"), attentionCounts.waitingUser],
                  ["handoff", tr("context.pendingHandoffs", "Pending handoffs"), attentionCounts.pendingHandoffs],
                  ["unassigned", tr("context.unassigned", "Unassigned"), unassignedCount],
                ].map(([value, label, count]) => (
                  <button
                    key={String(value)}
                    type="button"
                    onClick={() => setTaskFilter(value as "all" | "blocked" | "waiting_user" | "handoff" | "unassigned")}
                    className={classNames(chipBaseClass, taskFilter === value ? (isDark ? "border-blue-500 text-blue-200" : "border-blue-300 text-blue-700") : "")}
                  >
                    {label} · {count}
                  </button>
                ))}
                <span className={classNames("text-xs", mutedTextClass)}>{tr("context.filteredTasks", "{{count}} visible", { count: filteredTaskTotal })}</span>
                {syncBusy ? <span className={classNames("text-xs italic", mutedTextClass)}>{tr("context.applyingChanges", "Applying changes…")}</span> : null}
              </div>
            </div>

            {filteredTaskTotal === 0 ? (
              <div className={classNames("rounded-xl border border-dashed px-4 py-6 text-sm", isDark ? "border-slate-800 bg-slate-950/40 text-slate-400" : "border-gray-200 bg-gray-50 text-gray-500")}>
                {tr("context.noMatchingTasks", "No tasks match the current filters")}
              </div>
            ) : null}

            <div className="grid gap-4 xl:grid-cols-[minmax(0,1.55fr)_minmax(340px,420px)]">
              <div className="min-w-0">
                <DndContext sensors={sensors} onDragStart={handleDragStart} onDragEnd={handleDragEnd} onDragCancel={() => setDragTaskId("")}>
                  <div className="grid gap-3 md:grid-cols-2 2xl:grid-cols-4">
                    <ColumnDropZone columnKey="planned" label={tr("context.planned", "Planned")} items={filteredBoard.planned} />
                    <ColumnDropZone columnKey="active" label={tr("context.active", "Active")} items={filteredBoard.active} />
                    <ColumnDropZone columnKey="done" label={tr("context.done", "Done")} items={filteredBoard.done} />
                    <ColumnDropZone columnKey="archived" label={tr("context.archived", "Archived")} items={filteredBoard.archived} />
                  </div>
                  <DragOverlay>{dragTaskId && taskMap.get(dragTaskId) ? <TaskGhostCard task={taskMap.get(dragTaskId)!} /> : null}</DragOverlay>
                </DndContext>
              </div>
              <div className="min-w-0">
                {renderTaskEditorPanel()}
              </div>
            </div>
          </div>
        </section>
      </div>
    );
  };

  return (
    <ModalFrame
      isDark={isDark}
      onClose={handleModalClose}
      titleId="context-modal-title"
      title={t("context.title", { defaultValue: "Project Context" })}
      closeAriaLabel={t("context.closeAria", { defaultValue: "Close context modal" })}
      panelClassName="h-full w-full overflow-hidden rounded-none sm:h-[94vh] sm:max-w-[96vw]"
      modalRef={modalRef}
    >
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="flex min-h-full flex-col gap-4 p-4 sm:p-5">
          {syncError ? <div className={classNames("rounded-xl border px-3 py-2 text-sm", isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-200" : "border-rose-200 bg-rose-50 text-rose-700")}>{syncError}</div> : null}

          <div className="flex items-center justify-end">
            <div className={classNames("inline-flex rounded-2xl border p-1", isDark ? "border-slate-800 bg-slate-950/70" : "border-gray-200 bg-gray-100/80")}>
              <button type="button" onClick={() => handleSwitchActiveView("coordination")} className={viewButtonClass(activeView === "coordination")}>{tr("context.coordination", "Coordination")}</button>
              <button type="button" onClick={() => handleSwitchActiveView("agents")} className={viewButtonClass(activeView === "agents")}>{tr("context.agents", "Agents")}</button>
            </div>
          </div>

          {activeView === "coordination" ? renderCoordinationView() : renderAgentsView()}
        </div>
      </div>
    </ModalFrame>
  );

}