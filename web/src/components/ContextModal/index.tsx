import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  type DragEndEvent,
  type DragStartEvent,
  KeyboardSensor,
  PointerSensor,
  TouchSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import { useTranslation } from "react-i18next";
import {
  addCoordinationNote,
  apiJson,
  contextSync,
  deleteCoordinationTaskConfirmed,
  isTaskRevision,
  moveCoordinationTask,
  restoreCoordinationTask,
  updateCoordinationBrief,
  updateCoordinationTask,
  type ApiResponse,
} from "../../services/api";
import { reloadContextAfterWrite } from "../../features/contextModal/contextWriteback";
import { useTaskBoardData } from "../../features/contextModal/useTaskBoardData";
import type { GroupContext, AgentsMdInfo, Task } from "../../types";
import { evaluateTaskWorkflow, getTaskTypeDefinition } from "../../utils/taskWorkflow";
import { classNames } from "../../utils/classNames";
import { useModalA11y } from "../../hooks/useModalA11y";
import { setWorkspaceDirty } from "../../stores/workspaceNavigation";
import { ModalFrame } from "../modals/ModalFrame";
import { settingsDialogBodyClass, settingsDialogPanelClass } from "../modals/settings/types";
import { AgentsView } from "./agents/AgentsView";
import { AgentsMdPanel } from "./coordination/AgentsMdPanel";
import { WorkingSummaryPanel } from "./coordination/WorkingSummaryPanel";
import { CoordinationLogPanel } from "./coordination/CoordinationLogPanel";
import { TaskBoard } from "./coordination/TaskBoard";
import { TaskEditorPanel } from "./coordination/TaskEditorPanel";
import { CapabilitiesTab } from "../modals/settings/CapabilitiesTab";
import {
  briefDraftMatches,
  briefToDraft,
  directSubtasks,
  emptyNoteDraft,
  checklistToSave,
  emptyTaskDraft,
  parseLineList,
  isVisibleContextAgent,
  subtaskProgressByParent,
  taskDraftChanged,
  taskDraftPatch,
  taskHoldsDraftSave,
  taskMatchesDraftBase,
  taskStatus,
  taskToDraft,
  type BoardStatus,
  type BriefDraft,
  type ContextModalView,
  type NoteDraft,
  type TaskDraft,
  type TaskDeleteInfo,
  type TaskFilterValue,
  type TaskStale,
} from "./model";
import { createContextModalUi } from "./ui";
import { newClientId } from "../../utils/clientId";

/** Errors that mean the answer to a write was lost, or did not read as an answer. */
const ANSWER_UNCONFIRMED = new Set([
  "NETWORK_ERROR",
  "EMPTY_RESPONSE",
  "HTTP_ERROR",
  "PARSE_ERROR",
  "daemon_unavailable",
  "task_receipt_unconfirmed",
]);

/** Errors for a write refused before it was made. Any other may follow the write. */
const REFUSED_BEFORE_WRITE = new Set([
  "invalid_args",
  "permission_denied",
  "group_not_found",
  "task_not_found",
  "task_conflict",
  "task_changed",
  "version_conflict",
  "task_revision_unavailable",
  "task_delete_check_unavailable",
]);

interface ContextModalProps {
  isOpen: boolean;
  onClose: () => void;
  groupId: string;
  groupTitle?: string;
  context: GroupContext | null;
  initialTaskId?: string | null;
  onInitialTaskHandled?: () => void;
  onOpenContext: () => Promise<void>;
  onSyncContext: () => Promise<void>;
  isDark: boolean;
}

export function ContextModal({
  isOpen,
  onClose,
  groupId,
  groupTitle,
  context,
  initialTaskId,
  onInitialTaskHandled,
  onOpenContext,
  onSyncContext,
  isDark,
}: ContextModalProps) {
  const { t } = useTranslation("modals");
  const tr = useCallback(
    (key: string, fallback: string, vars?: Record<string, unknown>) =>
      String(t(key as never, { defaultValue: fallback, ...(vars || {}) } as never)),
    [t],
  );

  const ui = useMemo(() => createContextModalUi(isDark), [isDark]);

  const [activeView, setActiveView] = useState<ContextModalView>("tasks");
  const navigationId = useId();
  const navigationRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const [taskFilter, setTaskFilter] = useState<TaskFilterValue>("all");
  const [assigneeFilter, setAssigneeFilter] = useState<string>("__all__");
  const [taskQuery, setTaskQuery] = useState("");
  const [dragTaskId, setDragTaskId] = useState("");
  const [archivedExpanded, setArchivedExpanded] = useState(false);
  const [editingBrief, setEditingBrief] = useState(false);
  const [briefDraft, setBriefDraft] = useState<BriefDraft>(briefToDraft(null));
  const [taskEditorMode, setTaskEditorMode] = useState<"none" | "create" | "edit">("none");
  const [selectedTaskId, setSelectedTaskId] = useState("");
  const selectedTaskIdRef = useRef(selectedTaskId);
  selectedTaskIdRef.current = selectedTaskId;
  const [taskDraft, setTaskDraft] = useState<TaskDraft | null>(null);
  const taskDraftRef = useRef(taskDraft);
  taskDraftRef.current = taskDraft;
  const taskOpenSeqRef = useRef(0);
  // Which drawer is open: it changes whenever the drawer closes or shows
  // another task, so a save or delete answers only the drawer that made it.
  const editorSessionRef = useRef(0);
  // The last failed try to create the task open in the drawer, by what it sent.
  const lastCreateRef = useRef<{
    session: number;
    sent: string;
    requestId: string;
    mayExist: boolean;
  } | null>(null);
  // A save or delete was refused because the task changed, or a saved task
  // could not be read back: the draft no longer matches what is stored.
  const [taskStale, setTaskStale] = useState<TaskStale>("");
  const taskTitleRef = useRef<HTMLInputElement>(null);
  const [syncBusy, setSyncBusy] = useState(false);
  const [syncError, setSyncError] = useState("");
  const [boardError, setBoardError] = useState("");

  const [agentsMd, setAgentsMd] = useState<AgentsMdInfo | null>(null);
  const [agentsMdBusy, setAgentsMdBusy] = useState(false);
  const [agentsMdError, setAgentsMdError] = useState("");
  const [agentsMdNotice, setAgentsMdNotice] = useState("");
  const [editingAgentsMd, setEditingAgentsMd] = useState(false);
  const [agentsMdText, setAgentsMdText] = useState("");
  const [agentsMdExpanded, setAgentsMdExpanded] = useState(false);
  const [notifyAgents, setNotifyAgents] = useState(false);
  const [notifyError, setNotifyError] = useState("");

  const [decisionDraft, setDecisionDraft] = useState<NoteDraft>(emptyNoteDraft());
  const [handoffDraft, setHandoffDraft] = useState<NoteDraft>(emptyNoteDraft());
  const [activityBusyKind, setActivityBusyKind] = useState<"decision" | "handoff" | null>(null);
  const [activityError, setActivityError] = useState("");
  const lastOpenedGroupRef = useRef("");
  const modalVisitRef = useRef({ groupId, isOpen });
  const agentsMdReadSeqRef = useRef(0);

  const {
    columns: taskColumnPages,
    board,
    taskIndex,
    taskMap,
    tasksSummary,
    facets: taskFacets,
    error: taskLoadError,
    loading: taskLoading,
    deleteInfoById,
    refresh: refreshTasks,
    loadMore: loadMoreTasks,
    loadTaskDetail,
    forgetTask,
  } = useTaskBoardData({
    groupId,
    isOpen,
    query: taskQuery,
    assignee: assigneeFilter,
    filter: taskFilter,
    includeArchived: archivedExpanded,
    contextTasksVersion: context?.tasks_version,
    selectedTaskId,
  });

  const brief = context?.coordination?.brief || null;
  const agents = useMemo(
    () =>
      Array.isArray(context?.agent_states)
        ? context.agent_states.filter((agent) => isVisibleContextAgent(agent))
        : [],
    [context],
  );

  const selectedTask = selectedTaskId ? taskMap.get(selectedTaskId) || null : null;
  const taskWorkflowCoverage = useMemo(
    () =>
      evaluateTaskWorkflow({
        parentId: taskDraft?.parentId,
        taskType: taskDraft?.taskType,
        status: taskDraft?.status,
        assignee: taskDraft?.assignee,
        outcome: taskDraft?.outcome,
        notes: taskDraft?.notes,
        checklist: taskDraft?.checklist,
      }),
    [taskDraft],
  );
  const selectedTaskType = useMemo(
    () => getTaskTypeDefinition(taskDraft?.taskType || "standard"),
    [taskDraft?.taskType],
  );

  const attentionCounts = useMemo(
    () => ({
      blocked: taskFacets.blocked,
      waitingUser: taskFacets.waitingUser,
      pendingHandoffs: taskFacets.pendingHandoffs,
    }),
    [taskFacets],
  );

  const recentDecisions = useMemo(
    () =>
      Array.isArray(context?.coordination?.recent_decisions)
        ? context.coordination.recent_decisions
        : [],
    [context],
  );
  const recentHandoffs = useMemo(
    () =>
      Array.isArray(context?.coordination?.recent_handoffs)
        ? context.coordination.recent_handoffs
        : [],
    [context],
  );
  const agentsMdPathLabel = useMemo(() => {
    const path = String(agentsMd?.path || "").trim();
    return path || "AGENTS.md";
  }, [agentsMd?.path]);
  const notifyMessage = useMemo(
    () =>
      tr(
        "context.agentsMdUpdatedNotify",
        "Repository instructions updated at {{path}}. Re-read this file if it applies to your work.",
        { path: agentsMdPathLabel },
      ),
    [agentsMdPathLabel, tr],
  );

  const assigneeOptions = taskFacets.assignees;
  // Who a task can go to: the Group's agents, and anyone tasks already name.
  const assigneeChoices = useMemo(
    () =>
      [...new Set([...agents.map((agent) => String(agent.id || "")), ...assigneeOptions])]
        .filter((id) => id && id !== "__all__")
        .sort(),
    [agents, assigneeOptions],
  );
  const unassignedCount = taskFacets.unassigned;
  const activeTaskOptions = useMemo(
    () => taskIndex.filter((task) => taskStatus(task) !== "archived"),
    [taskIndex],
  );
  // The index covers every live task regardless of board paging and filters.
  const subtaskProgress = useMemo(() => subtaskProgressByParent(taskIndex), [taskIndex]);
  const selectedSubtasks = useMemo(
    () => directSubtasks(selectedTaskId, taskIndex),
    [selectedTaskId, taskIndex],
  );
  const hasBriefUnsaved = useMemo(
    () => editingBrief && !briefDraftMatches(brief, briefDraft),
    [brief, briefDraft, editingBrief],
  );
  const hasAgentsMdUnsaved = useMemo(
    () => editingAgentsMd && agentsMdText !== String(agentsMd?.content || ""),
    [editingAgentsMd, agentsMd?.content, agentsMdText],
  );
  const hasSteeringUnsaved = hasBriefUnsaved || hasAgentsMdUnsaved;
  const hasTaskUnsaved = taskEditorMode !== "none" && taskDraftChanged(taskDraft);
  // Deleted while it was open: the draft stays, so nothing typed is lost.
  const taskGone = taskEditorMode === "edit" && !!taskDraft && !!selectedTaskId && !selectedTask;
  const taskChangedElsewhere =
    taskEditorMode === "edit" &&
    !!taskDraft &&
    !!selectedTask &&
    !taskMatchesDraftBase(selectedTask, taskDraft);
  const selectedTaskDeleteInfo = useMemo<TaskDeleteInfo>(() => {
    const info = selectedTask ? deleteInfoById[selectedTask.id] : null;
    const reason = info?.reason;
    return {
      allowed: Boolean(info?.allowed),
      total: Number(info?.total || 0),
      reason: reason === "self_history" || reason === "subtree_history" ? reason : "",
    };
  }, [deleteInfoById, selectedTask]);
  const taskEditorVisible = taskEditorMode !== "none" && !!taskDraft;

  const loadAgentsMd = useCallback(
    async (force: boolean = false): Promise<AgentsMdInfo | null> => {
      if (!groupId) return null;
      if (!force && agentsMd !== null) return agentsMd;
      const visit = modalVisitRef.current;
      const seq = ++agentsMdReadSeqRef.current;
      const isCurrent = () => modalVisitRef.current === visit && agentsMdReadSeqRef.current === seq;
      setAgentsMdBusy(true);
      setAgentsMdError("");
      try {
        const resp = await apiJson<AgentsMdInfo>(
          `/api/v1/groups/${encodeURIComponent(groupId)}/agents_md`,
        );
        if (!isCurrent()) return null;
        if (!resp.ok) {
          setAgentsMd(null);
          setAgentsMdError(
            resp.error?.message || tr("context.failedToLoadAgentsMd", "Failed to load AGENTS.md"),
          );
          return null;
        }
        setAgentsMd(resp.result);
        return resp.result ?? null;
      } catch {
        if (isCurrent()) {
          setAgentsMd(null);
          setAgentsMdError(tr("context.failedToLoadAgentsMd", "Failed to load AGENTS.md"));
        }
        return null;
      } finally {
        if (isCurrent()) setAgentsMdBusy(false);
      }
    },
    [groupId, agentsMd, tr],
  );

  useEffect(() => {
    modalVisitRef.current = { groupId, isOpen };
    agentsMdReadSeqRef.current += 1;
    if (!isOpen || !groupId) return;

    setActiveView("tasks");
    setTaskFilter("all");
    setAssigneeFilter("__all__");
    setTaskQuery("");
    setArchivedExpanded(false);
    setDragTaskId("");
    setTaskStale("");
    editorSessionRef.current += 1;
    setTaskEditorMode("none");
    setSelectedTaskId("");
    setTaskDraft(null);
    setSyncBusy(false);
    setSyncError("");
    setBoardError("");
    setEditingBrief(false);
    setAgentsMd(null);
    setAgentsMdBusy(false);
    setAgentsMdError("");
    setAgentsMdNotice("");
    setEditingAgentsMd(false);
    setAgentsMdText("");
    setAgentsMdExpanded(false);
    setNotifyError("");
    setNotifyAgents(false);
    setDecisionDraft(emptyNoteDraft());
    setHandoffDraft(emptyNoteDraft());
    setActivityBusyKind(null);
    setActivityError("");
    return () => {
      modalVisitRef.current = { groupId, isOpen };
      agentsMdReadSeqRef.current += 1;
    };
  }, [groupId, isOpen]);

  useEffect(() => {
    if (!isOpen || !groupId) {
      lastOpenedGroupRef.current = "";
      return;
    }
    if (lastOpenedGroupRef.current === groupId) return;
    lastOpenedGroupRef.current = groupId;
    void onOpenContext();
  }, [groupId, isOpen, onOpenContext]);

  const applyContextWriteback = useCallback(
    async <T,>(response: ApiResponse<T>) => reloadContextAfterWrite(response, onSyncContext),
    [onSyncContext],
  );

  // A task as the daemon has it now. A detail read is dropped while the board
  // is behind the task list, so the board catches up first.
  const readTaskNow = useCallback(
    async (taskId: string) => {
      await refreshTasks();
      return loadTaskDetail(taskId);
    },
    [loadTaskDetail, refreshTasks],
  );

  useEffect(() => {
    if (!isOpen || !groupId) return;
    if (
      (activeView === "instructions" || editingAgentsMd) &&
      agentsMd === null &&
      !agentsMdBusy &&
      !agentsMdError
    ) {
      void loadAgentsMd();
    }
  }, [
    editingAgentsMd,
    groupId,
    isOpen,
    loadAgentsMd,
    agentsMdBusy,
    agentsMdError,
    agentsMd,
    activeView,
  ]);

  useEffect(() => {
    if (isOpen) {
      if (contentRef.current) contentRef.current.scrollTop = 0;
      navigationRef.current
        ?.querySelector('[aria-selected="true"]')
        ?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    }
  }, [activeView, isOpen]);

  const filteredBoard = board;
  const hasTaskFilters =
    taskFilter !== "all" || assigneeFilter !== "__all__" || taskQuery.trim().length > 0;
  const hiddenArchivedMatches =
    archivedExpanded || hasTaskFilters ? 0 : Number(tasksSummary.archived || 0);
  const visibleTaskTotal = useMemo(
    () =>
      filteredBoard.planned.length +
      filteredBoard.active.length +
      filteredBoard.done.length +
      (archivedExpanded ? filteredBoard.archived.length : 0),
    [archivedExpanded, filteredBoard],
  );
  const hasVisibleTasks = visibleTaskTotal > 0 || taskLoading;
  const archivedToggleCount = hasTaskFilters
    ? taskColumnPages.archived.totalCount
    : Number(tasksSummary.archived || 0);
  const hasArchivedTasks =
    archivedExpanded || archivedToggleCount > 0 || Number(tasksSummary.archived || 0) > 0;

  useEffect(() => {
    if (!isOpen) return;
    if (selectedTask && taskStatus(selectedTask) === "archived") {
      setArchivedExpanded(true);
    }
  }, [isOpen, selectedTask]);

  useEffect(() => {
    if (!isOpen || !selectedTaskId) return;
    void loadTaskDetail(selectedTaskId);
  }, [isOpen, loadTaskDetail, selectedTaskId]);

  useEffect(() => {
    if (taskEditorMode !== "edit") return;
    if (!selectedTaskId) {
      setTaskEditorMode("none");
      if (taskDraft) setTaskDraft(null);
      return;
    }
    if (!selectedTask) {
      // A task deleted while open keeps its draft until the editor is closed.
      if (!taskDraft) {
        setSelectedTaskId("");
        setTaskEditorMode("none");
      }
      return;
    }
    if (!taskDraft) setTaskDraft(taskToDraft(selectedTask));
  }, [selectedTaskId, selectedTask, taskDraft, taskEditorMode]);

  // A draft without edits follows the task as it changes. One with edits
  // stays as it is, and saving it is refused until it is reloaded, unless the
  // task now holds exactly those edits, as after a save whose answer was lost:
  // then they are saved, and the draft is the task again.
  useEffect(() => {
    if (taskEditorMode !== "edit" || !taskDraft || !selectedTask || syncBusy) return;
    if (taskMatchesDraftBase(selectedTask, taskDraft)) return;
    // Only forward: a list read before the last save can arrive after it.
    if ((selectedTask.rev ?? 0) < (taskDraft.base.rev ?? 0)) return;
    if (taskDraftChanged(taskDraft)) {
      if (!isTaskRevision(selectedTask.rev) || !taskHoldsDraftSave(selectedTask, taskDraft)) return;
      setSyncError("");
    } else if (taskStale) {
      return;
    }
    setTaskStale("");
    setTaskDraft(taskToDraft(selectedTask));
  }, [selectedTask, syncBusy, taskDraft, taskEditorMode, taskStale]);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 180, tolerance: 6 } }),
    useSensor(KeyboardSensor),
  );

  /** Why a task write was refused, in the user's terms. */
  const taskWriteError = useCallback(
    (error: { code?: string; message?: string } | undefined, fallback: string) =>
      error?.code === "task_revision_unavailable"
        ? tr(
            "context.taskRevisionUnavailable",
            "This task's version could not be read, so nothing was saved. Reload it and try again.",
          )
        : error?.code === "task_delete_check_unavailable"
          ? tr(
              "context.taskDeleteCheckUnavailable",
              "The task's current state could not be read, so nothing was deleted. Check your connection and try again.",
            )
          : ANSWER_UNCONFIRMED.has(String(error?.code || ""))
            ? tr(
                "context.taskWriteUnconfirmed",
                "It could not be confirmed whether this went through. Check the task's current state before trying again.",
              )
            : error?.message || fallback,
    [tr],
  );
  const selectedTaskDeleteHint = useMemo(() => {
    if (!selectedTask || selectedTaskDeleteInfo.allowed || !selectedTaskDeleteInfo.reason)
      return "";
    if (selectedTaskDeleteInfo.reason === "subtree_history") {
      return tr(
        "context.deleteTaskBlockedSubtree",
        "This task has child tasks with execution history, so delete is blocked.",
      );
    }
    return tr(
      "context.deleteTaskBlockedSelf",
      "Only tasks that never left Planned can be deleted. Move it to Archived instead.",
    );
  }, [selectedTask, selectedTaskDeleteInfo, tr]);

  const confirmDiscardTaskChanges = useCallback(() => {
    if (!hasTaskUnsaved || typeof window === "undefined") return true;
    return window.confirm(
      tr("context.unsavedTaskConfirm", "You have unsaved task edits. Discard them and continue?"),
    );
  }, [hasTaskUnsaved, tr]);

  const showTask = useCallback((task: Task) => {
    editorSessionRef.current += 1;
    setTaskStale("");
    setSelectedTaskId(task.id);
    setTaskDraft(taskToDraft(task));
    setTaskEditorMode("edit");
    setSyncError("");
    setBoardError("");
    setActiveView("tasks");
  }, []);

  // A move or a restore from the board is fenced on the revision the board
  // showed: a task changed since is left as it is, and the board is read
  // again instead. What it reports stays on the board, as an editor opened
  // while it ran is about another task.
  const writeFromBoard = useCallback(
    async (task: Task, write: (groupId: string) => Promise<ApiResponse<unknown>>) => {
      if (!groupId) return;
      const visit = modalVisitRef.current;
      const isCurrent = () => modalVisitRef.current === visit;
      setSyncBusy(true);
      setBoardError("");
      try {
        const resp = await write(groupId);
        if (!resp.ok) {
          const error = resp.error;
          if (isCurrent()) {
            setBoardError(
              error?.code === "task_conflict"
                ? tr(
                    "context.taskMoveConflict",
                    "{{id}} has changed since the board showed it, so it was not moved. Check its latest state and try again.",
                    { id: task.id },
                  )
                : taskWriteError(
                    error,
                    tr("context.failedToApplyChanges", "Failed to apply changes"),
                  ),
            );
          }
          // What became of the task, whatever became of the write.
          if (isCurrent()) await refreshTasks();
          return;
        }
        await applyContextWriteback(resp);
        if (!isCurrent()) return;
        await refreshTasks();
      } finally {
        if (isCurrent()) setSyncBusy(false);
      }
    },
    [applyContextWriteback, groupId, refreshTasks, taskWriteError, tr],
  );

  const moveTaskToStatus = useCallback(
    async (task: Task, nextStatus: BoardStatus) => {
      if (taskStatus(task) === nextStatus) return;
      await writeFromBoard(task, (id) => moveCoordinationTask(id, task.id, nextStatus, task.rev));
    },
    [writeFromBoard],
  );

  // Back to where it was before it was archived, which a move cannot know.
  const restoreTask = useCallback(
    (task: Task) => writeFromBoard(task, (id) => restoreCoordinationTask(id, task.id, task.rev)),
    [writeFromBoard],
  );

  const handleDragStart = useCallback((event: DragStartEvent) => {
    const id = String(event.active.id || "");
    if (id.startsWith("task:")) setDragTaskId(id.slice(5));
  }, []);

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      setDragTaskId("");
      const activeId = String(event.active.id || "");
      if (!activeId.startsWith("task:")) return;
      const boardTasks = Object.values(board).flat();
      const task = boardTasks.find((candidate) => candidate.id === activeId.slice(5));
      if (!task || !event.over) return;
      const overId = String(event.over.id || "");
      let nextStatus: BoardStatus | null = null;
      if (overId.startsWith("column:")) {
        nextStatus = overId.slice(7) as BoardStatus;
      } else if (overId.startsWith("task:")) {
        const overTask = boardTasks.find((candidate) => candidate.id === overId.slice(5));
        if (overTask) nextStatus = taskStatus(overTask) as BoardStatus;
      }
      if (nextStatus && nextStatus !== taskStatus(task)) {
        void moveTaskToStatus(task, nextStatus);
      }
    },
    [board, moveTaskToStatus],
  );

  const selectTask = useCallback(
    (task: Task) => {
      if (selectedTaskId === task.id && taskEditorMode === "edit") return;
      if (!confirmDiscardTaskChanges()) return;
      showTask(task);
    },
    [confirmDiscardTaskChanges, selectedTaskId, showTask, taskEditorMode],
  );

  // Read fresh full details before drafting, since cached subtasks may have changed elsewhere.
  // Unsaved edits are confirmed at click time; only the newest click applies, and a later edit,
  // selection or modal visit cancels a pending one instead of being overwritten by it.
  const openSubtask = useCallback(
    (taskId: string) => {
      const request = ++taskOpenSeqRef.current;
      if (!confirmDiscardTaskChanges()) return;
      const visit = modalVisitRef.current;
      const parentId = selectedTaskIdRef.current;
      const draft = taskDraftRef.current;
      void loadTaskDetail(taskId).then((task) => {
        if (
          task &&
          taskOpenSeqRef.current === request &&
          modalVisitRef.current === visit &&
          selectedTaskIdRef.current === parentId &&
          taskDraftRef.current === draft
        ) {
          showTask(task);
        }
      });
    },
    [confirmDiscardTaskChanges, loadTaskDetail, showTask],
  );

  useEffect(() => {
    if (!isOpen) return;
    const targetTaskId = String(initialTaskId || "").trim();
    if (!targetTaskId) return;
    if (selectedTaskId === targetTaskId && taskEditorMode === "edit") return;
    setTaskFilter("all");
    setAssigneeFilter("__all__");
    setTaskQuery("");
    let cancelled = false;
    void (async () => {
      const task = taskMap.get(targetTaskId) || (await loadTaskDetail(targetTaskId));
      if (cancelled || !task) return;
      if (taskStatus(task) === "archived") setArchivedExpanded(true);
      selectTask(task);
      onInitialTaskHandled?.();
    })();
    return () => {
      cancelled = true;
    };
  }, [
    initialTaskId,
    isOpen,
    loadTaskDetail,
    onInitialTaskHandled,
    selectTask,
    selectedTaskId,
    taskEditorMode,
    taskMap,
  ]);

  const closeTaskEditor = useCallback(() => {
    if (!confirmDiscardTaskChanges()) return;
    editorSessionRef.current += 1;
    setTaskStale("");
    setTaskEditorMode("none");
    setSelectedTaskId("");
    setTaskDraft(null);
    setSyncError("");
  }, [confirmDiscardTaskChanges]);

  const handleSwitchActiveView = useCallback(
    (next: ContextModalView) => {
      if (next !== "tasks") {
        if (!confirmDiscardTaskChanges()) return;
        editorSessionRef.current += 1;
        setTaskStale("");
        setTaskEditorMode("none");
        setSelectedTaskId("");
        setTaskDraft(null);
      }
      setActiveView(next);
      if (next === "instructions" && !agentsMdBusy && !editingAgentsMd) void loadAgentsMd();
    },
    [confirmDiscardTaskChanges, agentsMdBusy, editingAgentsMd, loadAgentsMd],
  );

  const handleModalClose = useCallback(() => {
    if (typeof window !== "undefined") {
      if (hasSteeringUnsaved && hasTaskUnsaved) {
        const ok = window.confirm(
          tr(
            "context.unsavedCloseConfirm",
            "You have unsaved project and task changes. Discard them and close?",
          ),
        );
        if (!ok) return;
      } else if (hasSteeringUnsaved) {
        const ok = window.confirm(
          tr(
            "context.unsavedChangesConfirm",
            "You have unsaved changes in project steering. Discard them and close?",
          ),
        );
        if (!ok) return;
      } else if (hasTaskUnsaved) {
        const ok = window.confirm(
          tr(
            "context.unsavedTaskConfirm",
            "You have unsaved task edits. Discard them and continue?",
          ),
        );
        if (!ok) return;
      }
    }
    onClose();
  }, [hasSteeringUnsaved, hasTaskUnsaved, onClose, tr]);

  // Escape during a keyboard drag cancels the drag alone. The board's drag
  // handling hears it after this modal does, whatever the modal does with it.
  const handleModalEscape = useCallback(() => {
    if (!dragTaskId) handleModalClose();
  }, [dragTaskId, handleModalClose]);
  const { modalRef } = useModalA11y(isOpen, handleModalEscape);

  // Navigation that replaces this Group's context, such as opening a
  // notification for another Group, asks before unsaved edits are dropped.
  const dirtyOwner = useRef(Symbol("project context"));
  useEffect(() => {
    setWorkspaceDirty(dirtyOwner.current, isOpen && (hasTaskUnsaved || hasSteeringUnsaved));
  }, [hasSteeringUnsaved, hasTaskUnsaved, isOpen]);
  useEffect(() => {
    const owner = dirtyOwner.current;
    return () => setWorkspaceDirty(owner, false);
  }, []);
  const closeAgentsMdExpanded = useCallback(() => setAgentsMdExpanded(false), []);
  const { modalRef: agentsMdExpandedRef } = useModalA11y(agentsMdExpanded, closeAgentsMdExpanded);
  // The drawer is a dialog over the Context modal: Escape closes it alone, and
  // focus returns to the card that opened it.
  const { modalRef: taskDrawerRef } = useModalA11y(taskEditorVisible, closeTaskEditor, {
    initialFocusRef: taskEditorMode === "create" ? taskTitleRef : undefined,
  });

  const handleSaveBrief = useCallback(async () => {
    if (!groupId) return;
    const visit = modalVisitRef.current;
    const isCurrent = () => modalVisitRef.current === visit;
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
      const nextResp = await applyContextWriteback(resp);
      if (!nextResp.ok) {
        if (isCurrent()) {
          setSyncError(
            nextResp.error?.message ||
              resp.error?.message ||
              tr("context.failedToApplyChanges", "Failed to apply changes"),
          );
        }
        return;
      }
      if (isCurrent()) setEditingBrief(false);
    } finally {
      if (isCurrent()) setSyncBusy(false);
    }
  }, [applyContextWriteback, briefDraft, groupId, tr]);

  const handleSaveTask = useCallback(async () => {
    if (!groupId || !taskDraft) return;
    const title = taskDraft.title.trim();
    if (!title) {
      setSyncError(tr("context.taskTitleRequired", "Task title is required."));
      return;
    }

    const visit = modalVisitRef.current;
    const session = editorSessionRef.current;
    const isCurrent = () => modalVisitRef.current === visit;
    // The board is read again for any drawer; the draft, its errors and the
    // drawer itself only while it is the one that saved.
    const inEditor = () => isCurrent() && editorSessionRef.current === session;

    // A new task is sent with a request id the daemon knows it by. A try that
    // fails keeps its id for the task as sent: when it was written after all,
    // as when the answer was lost, sending the same again finds that task
    // instead of making another.
    let create: {
      op: Record<string, unknown>;
      sent: string;
      requestId: string;
      /** An earlier try under this id may have been written. */
      mayExist: boolean;
    } | null = null;
    if (taskEditorMode === "create") {
      const op = {
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
        task_type: taskDraft.taskType,
        notes: taskDraft.notes,
        checklist: checklistToSave(taskDraft.checklist),
      };
      const sent = JSON.stringify(op);
      const last = lastCreateRef.current?.session === session ? lastCreateRef.current : null;
      // Changed since: the old id names what was sent then, so a new one is
      // used, which makes a second task if that try went through. That is
      // asked first.
      if (
        last?.mayExist &&
        last.sent !== sent &&
        typeof window !== "undefined" &&
        !window.confirm(
          tr(
            "context.createMayExist",
            "Your last try may have created this task already. Create it with your changes anyway? That may make a second task.",
          ),
        )
      ) {
        return;
      }
      const again = last?.sent === sent ? last : null;
      create = {
        op,
        sent,
        requestId: again?.requestId || newClientId(),
        mayExist: again?.mayExist ?? false,
      };
    }

    // The editor holds still while saving, so the draft is what gets saved.
    setSyncBusy(true);
    setSyncError("");
    try {
      if (create) {
        const { op, sent, requestId, mayExist } = create;
        const resp = await contextSync(groupId, [{ ...op, client_request_id: requestId }]);
        if (!resp.ok) {
          if (!inEditor()) return;
          const code = String(resp.error?.code || "");
          const lost = ANSWER_UNCONFIRMED.has(code);
          // Only a refusal before the write shows nothing was made, and only
          // by this try: one refused under the same id says nothing about an
          // earlier one.
          lastCreateRef.current = {
            session,
            sent,
            requestId,
            mayExist: mayExist || !REFUSED_BEFORE_WRITE.has(code),
          };
          setSyncError(
            lost
              ? tr(
                  "context.createUnconfirmed",
                  "Creating the task could not be confirmed, so it may already exist. Create it again as it is to finish without making a second one.",
                )
              : resp.error?.message ||
                  tr("context.failedToApplyChanges", "Failed to apply changes"),
          );
          return;
        }
        // It exists now, made by this try or by an earlier one sent the same.
        lastCreateRef.current = null;
        await applyContextWriteback(resp);
        if (!isCurrent()) return;
        await refreshTasks();
        if (!inEditor()) return;
        editorSessionRef.current += 1;
        setTaskEditorMode("none");
        setSelectedTaskId("");
        setTaskDraft(null);
        return;
      }

      if (!selectedTask) {
        setSyncError(
          tr("context.taskGoneSave", "This task no longer exists, so it cannot be saved."),
        );
        return;
      }
      const taskId = selectedTask.id;
      // The task as the daemon has it now. It is read once the board has caught
      // up with the write, as a read begun before then is dropped, and twice.
      const readBack = async () => {
        if (!isCurrent()) return null;
        let latest = await readTaskNow(taskId);
        if (!latest && inEditor()) latest = await loadTaskDetail(taskId);
        return inEditor() ? latest : null;
      };
      // Only what the draft changed is sent, fenced on the revision the draft
      // was made from: a task changed since is not overwritten.
      const resp = await updateCoordinationTask(
        groupId,
        taskId,
        taskDraftPatch(taskDraft),
        taskDraft.base.rev,
      );
      if (!resp.ok) {
        if (!inEditor()) return;
        const code = String(resp.error?.code || "");
        if (code === "task_conflict") {
          setTaskStale("changed");
          void readTaskNow(taskId);
          return;
        }
        // Gone, by the daemon's own word: the editor shows so at once,
        // keeping the draft, even if the board cannot be read again.
        if (code === "task_not_found") {
          forgetTask(taskId);
          void refreshTasks();
          return;
        }
        if (REFUSED_BEFORE_WRITE.has(code)) {
          setSyncError(
            taskWriteError(
              resp.error,
              tr("context.failedToApplyChanges", "Failed to apply changes"),
            ),
          );
          return;
        }
        // Whether it was saved is not known; the task as it is now tells, if
        // it is read with its revision. Saving again is safe either way: it is
        // fenced on the same revision.
        const latest = await readBack();
        if (!inEditor()) return;
        const read = latest && isTaskRevision(latest.rev) ? latest : null;
        if (read && taskHoldsDraftSave(read, taskDraft)) {
          setTaskDraft(taskToDraft(read));
        } else if (read && read.rev === taskDraft.base.rev) {
          setSyncError(tr("context.taskNotSaved", "Your changes were not saved. Try again."));
        } else if (read) {
          setTaskStale("changed");
        } else {
          setSyncError(
            tr(
              "context.taskSaveUnconfirmed",
              "It could not be confirmed whether your changes were saved. Saving again will not apply them twice.",
            ),
          );
        }
        return;
      }
      // Saved. Reading the context again can fail without undoing that.
      await applyContextWriteback(resp);
      const saved = await readBack();
      if (!inEditor()) return;
      // Saved, but not read with its revision: the draft keeps what was saved,
      // and saving again waits for a reload, as the next save is fenced on it.
      if (saved && isTaskRevision(saved.rev)) setTaskDraft(taskToDraft(saved));
      else setTaskStale("unconfirmed");
    } finally {
      if (isCurrent()) setSyncBusy(false);
    }
  }, [
    applyContextWriteback,
    forgetTask,
    groupId,
    loadTaskDetail,
    readTaskNow,
    refreshTasks,
    selectedTask,
    taskDraft,
    taskEditorMode,
    taskWriteError,
    tr,
  ]);

  // A deletion removes the task's whole subtree, so it is fenced on what the
  // user confirmed: the task and its subtree as one read showed them.
  const handleDeleteTask = useCallback(
    async (task: Task) => {
      if (!groupId) return;
      const deleteInfo = deleteInfoById[task.id];
      if (!deleteInfo?.allowed) return;
      if (!confirmDiscardTaskChanges()) return;
      if (typeof window !== "undefined") {
        // The subtree's total counts the task itself; the user is told of the ones below it.
        const confirmed = window.confirm(
          deleteInfo.total > 1
            ? tr(
                "context.deleteTaskCascadeConfirm",
                'Delete task "{{title}}" and its {{count}} unexecuted tasks?',
                { title: taskTitle(task) || task.id, count: deleteInfo.total - 1 },
              )
            : tr("context.deleteTaskConfirm", 'Delete task "{{title}}" permanently?', {
                title: taskTitle(task) || task.id,
              }),
        );
        if (!confirmed) return;
      }

      const visit = modalVisitRef.current;
      const session = editorSessionRef.current;
      const isCurrent = () => modalVisitRef.current === visit;
      const inEditor = () => isCurrent() && editorSessionRef.current === session;
      setSyncBusy(true);
      setSyncError("");
      try {
        const resp = await deleteCoordinationTaskConfirmed(groupId, task.id, {
          rev: deleteInfo.rev,
          total: deleteInfo.total,
          tasksVersion: deleteInfo.tasksVersion,
        });
        if (!resp.ok) {
          const error = resp.error;
          if (!inEditor()) return;
          if (error?.code === "task_changed" || error?.code === "task_conflict") {
            setSyncError(
              tr(
                "context.taskDeleteChanged",
                "Something changed after you confirmed, so nothing was deleted. Check the task and confirm again.",
              ),
            );
            void readTaskNow(task.id);
          } else if (error?.code === "task_not_found") {
            // Already gone, by the daemon's own word: the editor shows so.
            forgetTask(task.id);
            void refreshTasks();
          } else {
            setSyncError(
              taskWriteError(error, tr("context.failedToDeleteTask", "Failed to delete task")),
            );
            // Unless refused outright, it may be gone: reading it shows which.
            if (!REFUSED_BEFORE_WRITE.has(String(error?.code || ""))) void readTaskNow(task.id);
          }
          return;
        }
        await applyContextWriteback(resp);
        if (!isCurrent()) return;
        if (inEditor()) {
          editorSessionRef.current += 1;
          setTaskEditorMode("none");
          setSelectedTaskId("");
          setTaskDraft(null);
        }
        forgetTask(task.id);
        await refreshTasks();
      } finally {
        if (isCurrent()) setSyncBusy(false);
      }
    },
    [
      applyContextWriteback,
      confirmDiscardTaskChanges,
      deleteInfoById,
      forgetTask,
      groupId,
      readTaskNow,
      refreshTasks,
      taskWriteError,
      tr,
    ],
  );

  // Drop the edits. An existing task loads its latest version, which is also
  // the way out of a draft that went stale.
  const handleResetTask = useCallback(async () => {
    if (!taskDraft) return;
    setSyncError("");
    if (taskEditorMode === "create") {
      setTaskDraft({ ...taskDraft.base.fields, base: taskDraft.base });
      return;
    }
    if (!selectedTaskId) return;
    const visit = modalVisitRef.current;
    const draft = taskDraft;
    const latest = await readTaskNow(selectedTaskId);
    // A later edit, selection or visit wins over this reload.
    if (modalVisitRef.current !== visit || taskDraftRef.current !== draft) return;
    // Only a task just read, with its revision, replaces the draft: what is
    // cached may be what went stale, and a deleted task shows as such.
    if (!latest || !isTaskRevision(latest.rev)) {
      setSyncError(
        tr(
          "context.taskReloadFailed",
          "The latest version could not be loaded, so your edits are kept. Try again.",
        ),
      );
      return;
    }
    setTaskStale("");
    setTaskDraft(taskToDraft(latest));
  }, [readTaskNow, selectedTaskId, taskDraft, taskEditorMode, tr]);

  const handleOpenCreate = useCallback(
    (status: BoardStatus = "planned", parentId = "") => {
      if (!confirmDiscardTaskChanges()) return;
      editorSessionRef.current += 1;
      setTaskStale("");
      setTaskEditorMode("create");
      setSelectedTaskId("");
      setTaskDraft(emptyTaskDraft(status, parentId));
      setSyncError("");
      setBoardError("");
      setActiveView("tasks");
    },
    [confirmDiscardTaskChanges],
  );

  const handleEditAgentsMd = useCallback(async () => {
    const visit = modalVisitRef.current;
    const loaded = await loadAgentsMd(true);
    if (modalVisitRef.current !== visit || !loaded) return;
    setAgentsMdText(String(loaded.content || ""));
    setEditingAgentsMd(true);
  }, [loadAgentsMd]);

  const handleSaveAgentsMd = useCallback(async () => {
    if (!groupId || !agentsMd?.path) return;
    const visit = modalVisitRef.current;
    const isCurrent = () => modalVisitRef.current === visit;
    agentsMdReadSeqRef.current += 1;
    setAgentsMdBusy(true);
    setAgentsMdError("");
    setAgentsMdNotice("");
    setNotifyError("");
    try {
      const resp = await apiJson<AgentsMdInfo>(
        `/api/v1/groups/${encodeURIComponent(groupId)}/agents_md`,
        {
          method: "PUT",
          body: JSON.stringify({ content: agentsMdText, expected_path: agentsMd.path }),
        },
      );
      if (!resp.ok) {
        if (isCurrent()) {
          setAgentsMdError(
            resp.error?.message || tr("context.failedToSaveAgentsMd", "Failed to save AGENTS.md"),
          );
        }
        return;
      }
      if (isCurrent()) {
        setAgentsMd(resp.result);
        setEditingAgentsMd(false);
      }

      let notice = tr("context.agentsMdSaved", "AGENTS.md saved.");
      if (notifyAgents) {
        const notifyResp = await apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/send`, {
          method: "POST",
          body: JSON.stringify({
            text: notifyMessage,
            by: "user",
            to: ["@all"],
            message_mode: "send",
            path: "",
          }),
        });
        if (!notifyResp.ok) {
          if (isCurrent()) {
            setNotifyError(
              notifyResp.error?.message || tr("context.failedToNotify", "Failed to notify agents"),
            );
          }
          notice = tr(
            "context.agentsMdSavedNotifyFailed",
            "AGENTS.md saved, but chat notification failed.",
          );
        } else {
          notice = tr(
            "context.agentsMdSavedAndNotified",
            "AGENTS.md saved and the team was notified in chat.",
          );
        }
      }
      if (isCurrent()) {
        setAgentsMdNotice(notice);
        setNotifyAgents(false);
      }
    } finally {
      if (isCurrent()) setAgentsMdBusy(false);
    }
  }, [agentsMd?.path, groupId, notifyAgents, notifyMessage, agentsMdText, tr]);

  const handleAddCoordinationNote = useCallback(
    async (kind: "decision" | "handoff") => {
      if (!groupId) return;
      const draft = kind === "decision" ? decisionDraft : handoffDraft;
      const summary = String(draft.summary || "").trim();
      if (!summary) {
        setActivityError(tr("context.noteSummaryRequired", "Summary is required."));
        return;
      }
      const visit = modalVisitRef.current;
      const isCurrent = () => modalVisitRef.current === visit;
      setActivityBusyKind(kind);
      setActivityError("");
      try {
        const resp = await addCoordinationNote(
          groupId,
          kind,
          summary,
          String(draft.taskId || "").trim() || null,
        );
        const nextResp = await applyContextWriteback(resp);
        if (!nextResp.ok) {
          if (isCurrent()) {
            setActivityError(
              nextResp.error?.message ||
                resp.error?.message ||
                tr("context.failedToApplyChanges", "Failed to apply changes"),
            );
          }
          return;
        }
        if (!isCurrent()) return;
        if (kind === "decision") {
          setDecisionDraft(emptyNoteDraft());
        } else {
          setHandoffDraft(emptyNoteDraft());
        }
      } finally {
        if (isCurrent()) setActivityBusyKind(null);
      }
    },
    [applyContextWriteback, decisionDraft, groupId, handoffDraft, tr],
  );

  const startBriefEdit = useCallback(() => {
    if (!editingBrief) setBriefDraft(briefToDraft(brief));
    setEditingBrief(true);
  }, [brief, editingBrief]);

  const cancelBriefEdit = useCallback(() => {
    setEditingBrief(false);
    setBriefDraft(briefToDraft(brief));
  }, [brief]);

  const agentsMdPanel = (
    <AgentsMdPanel
      expanded={false}
      isDark={isDark}
      tr={tr}
      ui={ui}
      agentsMdBusy={agentsMdBusy}
      agentsMdError={agentsMdError}
      notifyError={notifyError}
      agentsMdNotice={agentsMdNotice}
      agentsMdPathLabel={agentsMdPathLabel}
      editingAgentsMd={editingAgentsMd}
      agentsMd={agentsMd}
      agentsMdText={agentsMdText}
      notifyAgents={notifyAgents}
      onExpand={() => setAgentsMdExpanded(true)}
      onCancelEdit={() => {
        setEditingAgentsMd(false);
        setAgentsMdText(String(agentsMd?.content || ""));
        setNotifyAgents(false);
      }}
      onEditAgentsMd={() => void handleEditAgentsMd()}
      onAgentsMdTextChange={setAgentsMdText}
      onNotifyAgentsChange={setNotifyAgents}
      onSaveAgentsMd={() => void handleSaveAgentsMd()}
    />
  );

  if (!isOpen) return null;

  const views: { value: ContextModalView; label: string }[] = [
    { value: "tasks", label: tr("context.tasks", "Tasks") },
    { value: "instructions", label: tr("context.agentsMd", "AGENTS.md") },
    { value: "agents", label: tr("context.agents", "Agent State") },
    { value: "log", label: tr("context.activityLog", "Coordination log") },
    { value: "self_evolving_skills", label: tr("context.skillsTab", "Skills") },
  ];

  return (
    <>
      <ModalFrame
        isDark={isDark}
        onClose={handleModalClose}
        titleId="context-modal-title"
        title={
          <span className="flex min-w-0 items-baseline gap-x-3">
            <span className="shrink-0 font-semibold">{tr("context.title", "Project Context")}</span>
            <span
              title={groupTitle || groupId}
              className="min-w-0 truncate text-sm font-normal text-[var(--color-text-secondary)]"
            >
              {groupTitle || groupId}
            </span>
          </span>
        }
        closeAriaLabel={tr("context.closeAria", "Close context modal")}
        panelClassName="h-full w-full overflow-hidden rounded-none sm:h-[94vh] sm:max-w-[96vw]"
        headerClassName="!px-4 !pb-2 sm:!px-5"
        modalRef={modalRef}
      >
        <div
          ref={navigationRef}
          role="tablist"
          aria-label={tr("context.tabsAria", "Context sections")}
          className="flex shrink-0 gap-5 overflow-x-auto scroll-px-4 sm:scroll-px-5 border-b border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-4 sm:px-5"
          onKeyDown={(event) => {
            const tabs = Array.from(
              event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]'),
            );
            const current = tabs.indexOf(document.activeElement as HTMLButtonElement);
            if (current < 0) return;
            const next =
              event.key === "ArrowRight"
                ? (current + 1) % tabs.length
                : event.key === "ArrowLeft"
                  ? (current + tabs.length - 1) % tabs.length
                  : event.key === "Home"
                    ? 0
                    : event.key === "End"
                      ? tabs.length - 1
                      : -1;
            if (next < 0) return;
            event.preventDefault();
            tabs[next].focus();
            tabs[next].scrollIntoView?.({ block: "nearest", inline: "nearest" });
          }}
        >
          {views.map(({ value, label }) => (
            <button
              key={value}
              type="button"
              role="tab"
              id={`${navigationId}-tab-${value}`}
              aria-controls={`${navigationId}-panel-${value}`}
              aria-selected={activeView === value}
              tabIndex={activeView === value ? 0 : -1}
              onClick={() => handleSwitchActiveView(value)}
              className={classNames(
                "min-h-11 shrink-0 whitespace-nowrap border-b-2 py-2 text-sm font-medium transition-colors focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] focus-visible:outline-offset-[-2px]",
                activeView === value
                  ? "border-[var(--color-text-primary)] text-[var(--color-text-primary)]"
                  : "border-transparent text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]",
              )}
            >
              {label}
            </button>
          ))}
        </div>
        <div
          ref={contentRef}
          className="min-h-0 flex-1 overflow-y-auto bg-[var(--color-bg-primary)]"
        >
          <div className="flex min-h-full flex-col gap-4 p-4 sm:p-5">
            {[boardError, taskEditorVisible ? "" : syncError].map((message, index) =>
              message ? (
                <div
                  key={index}
                  role="alert"
                  className={classNames(
                    "rounded-xl border px-3 py-2 text-sm",
                    "border-rose-500/30 bg-rose-500/15 text-rose-600 dark:text-rose-400",
                  )}
                >
                  {message}
                </div>
              ) : null,
            )}

            {views.map(({ value }) => (
              <div
                key={value}
                role="tabpanel"
                id={`${navigationId}-panel-${value}`}
                aria-labelledby={`${navigationId}-tab-${value}`}
                hidden={activeView !== value}
                tabIndex={0}
                className="min-w-0 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
              >
                {activeView === value &&
                  (value === "tasks" ? (
                    <div className="space-y-4">
                      <WorkingSummaryPanel
                        tr={tr}
                        ui={ui}
                        brief={brief}
                        tasksSummary={tasksSummary}
                        attentionCounts={attentionCounts}
                        unassignedCount={unassignedCount}
                        editingBrief={editingBrief}
                        briefDraft={briefDraft}
                        syncBusy={syncBusy}
                        onStartBriefEdit={startBriefEdit}
                        onCancelBriefEdit={cancelBriefEdit}
                        onSaveBrief={() => void handleSaveBrief()}
                        onBriefDraftChange={(updater) => setBriefDraft((prev) => updater(prev))}
                      />

                      <TaskBoard
                        tr={tr}
                        ui={ui}
                        syncBusy={syncBusy}
                        taskQuery={taskQuery}
                        assigneeFilter={assigneeFilter}
                        assigneeOptions={assigneeOptions}
                        taskFilter={taskFilter}
                        tasksSummary={tasksSummary}
                        attentionCounts={attentionCounts}
                        unassignedCount={unassignedCount}
                        hasArchivedTasks={hasArchivedTasks}
                        archivedExpanded={archivedExpanded}
                        hasVisibleTasks={hasVisibleTasks}
                        hiddenArchivedMatches={hiddenArchivedMatches}
                        filteredBoard={filteredBoard}
                        columnPages={taskColumnPages}
                        taskLoading={taskLoading}
                        taskLoadError={taskLoadError}
                        taskMap={taskMap}
                        subtaskProgress={subtaskProgress}
                        selectedTaskId={selectedTaskId}
                        dragTaskId={dragTaskId}
                        sensors={sensors}
                        onTaskQueryChange={setTaskQuery}
                        onAssigneeFilterChange={setAssigneeFilter}
                        onTaskFilterChange={setTaskFilter}
                        onClearFilters={() => {
                          setTaskQuery("");
                          setTaskFilter("all");
                          setAssigneeFilter("__all__");
                        }}
                        onArchivedExpandedChange={setArchivedExpanded}
                        onOpenCreate={handleOpenCreate}
                        onDragStart={handleDragStart}
                        onDragEnd={handleDragEnd}
                        onDragCancel={() => setDragTaskId("")}
                        onSelectTask={selectTask}
                        onMoveTaskToStatus={(task, nextStatus) =>
                          void moveTaskToStatus(task, nextStatus)
                        }
                        onRestoreTask={(task) => void restoreTask(task)}
                        onLoadMore={(status) => void loadMoreTasks(status)}
                        onRetryLoad={() => void refreshTasks()}
                      />
                    </div>
                  ) : value === "instructions" ? (
                    agentsMdPanel
                  ) : value === "log" ? (
                    <CoordinationLogPanel
                      tr={tr}
                      ui={ui}
                      activityBusyKind={activityBusyKind}
                      activityError={activityError}
                      recentDecisions={recentDecisions}
                      recentHandoffs={recentHandoffs}
                      decisionDraft={decisionDraft}
                      handoffDraft={handoffDraft}
                      activeTaskOptions={activeTaskOptions}
                      onDecisionDraftChange={setDecisionDraft}
                      onHandoffDraftChange={setHandoffDraft}
                      onAddCoordinationNote={(kind) => void handleAddCoordinationNote(kind)}
                    />
                  ) : value === "agents" ? (
                    <AgentsView agents={agents} tr={tr} ui={ui} />
                  ) : (
                    <CapabilitiesTab
                      isDark={isDark}
                      isActive={isOpen && activeView === "self_evolving_skills"}
                      groupId={groupId}
                      surface="selfEvolving"
                    />
                  ))}
              </div>
            ))}
          </div>
        </div>
      </ModalFrame>

      {taskEditorVisible && typeof document !== "undefined"
        ? createPortal(
            <div
              ref={taskDrawerRef}
              className="fixed inset-0 z-[65] animate-fade-in"
              role="dialog"
              aria-modal="true"
              aria-labelledby="context-task-drawer-title"
            >
              <div className="absolute inset-0 glass-overlay" onPointerDown={closeTaskEditor} />
              <div className="absolute inset-y-0 right-0 flex w-full justify-end">
                <div className="h-full w-full sm:w-[min(860px,calc(100vw-1.5rem))]">
                  <div className="flex h-full flex-col border-l border-[var(--glass-border-subtle)] shadow-2xl glass-modal">
                    <div className="sr-only" id="context-task-drawer-title">
                      {taskEditorMode === "create"
                        ? tr("context.newTask", "New task")
                        : tr("context.taskDetails", "Task editor")}
                    </div>
                    <div className="min-h-0 flex-1 overflow-hidden">
                      <TaskEditorPanel
                        tr={tr}
                        ui={ui}
                        taskEditorMode={taskEditorMode}
                        taskDraft={taskDraft}
                        hasTaskUnsaved={hasTaskUnsaved}
                        syncBusy={syncBusy}
                        error={syncError}
                        taskGone={taskGone}
                        taskStale={
                          taskStale ||
                          (!syncBusy && hasTaskUnsaved && taskChangedElsewhere ? "changed" : "")
                        }
                        selectedTask={selectedTask}
                        subtasks={selectedSubtasks}
                        assigneeChoices={assigneeChoices}
                        parentChoices={activeTaskOptions}
                        selectedTaskDeleteInfo={selectedTaskDeleteInfo}
                        selectedTaskDeleteHint={selectedTaskDeleteHint}
                        taskWorkflowCoverage={taskWorkflowCoverage}
                        taskTypeId={taskDraft?.taskType || "standard"}
                        selectedTaskType={selectedTaskType}
                        titleRef={taskTitleRef}
                        setTaskDraft={setTaskDraft}
                        onTaskTypeChange={(nextTaskTypeId) =>
                          setTaskDraft((prev) =>
                            prev ? { ...prev, taskType: nextTaskTypeId } : prev,
                          )
                        }
                        onResetTask={() => void handleResetTask()}
                        onClose={closeTaskEditor}
                        onDeleteSelectedTask={() => {
                          if (selectedTask) void handleDeleteTask(selectedTask);
                        }}
                        onSaveTask={() => void handleSaveTask()}
                        onOpenTask={openSubtask}
                        onAddSubtask={() => {
                          if (selectedTask) handleOpenCreate("planned", selectedTask.id);
                        }}
                      />
                    </div>
                  </div>
                </div>
              </div>
            </div>,
            document.body,
          )
        : null}

      {agentsMdExpanded && typeof document !== "undefined"
        ? createPortal(
            <div
              className="fixed inset-0 z-[70] animate-fade-in"
              role="dialog"
              aria-modal="true"
              aria-label={tr("context.agentsMd", "AGENTS.md")}
              onPointerDown={(event) => {
                if (event.target === event.currentTarget) setAgentsMdExpanded(false);
              }}
            >
              <div className="absolute inset-0 glass-overlay" />
              <div ref={agentsMdExpandedRef} className={settingsDialogPanelClass("xl")}>
                <div className="flex shrink-0 justify-end border-b border-[var(--glass-border-subtle)] px-3 py-2 sm:px-4 sm:py-3">
                  <button
                    type="button"
                    className={ui.buttonSecondaryClass}
                    onClick={() => setAgentsMdExpanded(false)}
                  >
                    {tr("common:close", "Close")}
                  </button>
                </div>
                <div className={settingsDialogBodyClass}>
                  <AgentsMdPanel
                    expanded
                    isDark={isDark}
                    tr={tr}
                    ui={ui}
                    agentsMdBusy={agentsMdBusy}
                    agentsMdError={agentsMdError}
                    notifyError={notifyError}
                    agentsMdNotice={agentsMdNotice}
                    agentsMdPathLabel={agentsMdPathLabel}
                    editingAgentsMd={editingAgentsMd}
                    agentsMd={agentsMd}
                    agentsMdText={agentsMdText}
                    notifyAgents={notifyAgents}
                    onExpand={() => {}}
                    onCancelEdit={() => {
                      setEditingAgentsMd(false);
                      setAgentsMdText(String(agentsMd?.content || ""));
                      setNotifyAgents(false);
                    }}
                    onEditAgentsMd={() => void handleEditAgentsMd()}
                    onAgentsMdTextChange={setAgentsMdText}
                    onNotifyAgentsChange={setNotifyAgents}
                    onSaveAgentsMd={() => void handleSaveAgentsMd()}
                  />
                </div>
              </div>
            </div>,
            document.body,
          )
        : null}
    </>
  );
}

function taskTitle(task: Task | null | undefined): string {
  if (!task) return "";
  return String(task.title || task.id || "").trim();
}
