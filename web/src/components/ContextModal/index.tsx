import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  type DragEndEvent,
  type DragStartEvent,
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
  deleteCoordinationTask,
  updateCoordinationBrief,
  updateCoordinationTask,
  type ApiResponse,
} from "../../services/api";
import { reloadContextAfterWrite } from "../../features/contextModal/contextWriteback";
import { useTaskBoardData } from "../../features/contextModal/useTaskBoardData";
import type { GroupContext, AgentsMdInfo, Task } from "../../types";
import {
  evaluateTaskWorkflow,
  getTaskDoneTransitionBlockers,
  getTaskTypeDefinition,
} from "../../utils/taskWorkflow";
import { classNames } from "../../utils/classNames";
import { useModalA11y } from "../../hooks/useModalA11y";
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
  emptyTaskDraft,
  parseChecklist,
  parseLineList,
  isVisibleContextAgent,
  subtaskProgressByParent,
  taskDraftDirty,
  taskDraftMatches,
  taskStatus,
  taskToDraft,
  type BoardStatus,
  type BriefDraft,
  type ContextModalView,
  type NoteDraft,
  type TaskDraft,
  type TaskDeleteInfo,
  type TaskFilterValue,
} from "./model";
import { createContextModalUi } from "./ui";

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
  const [pendingTaskReadback, setPendingTaskReadback] = useState<{
    taskId: string;
    previousUpdatedAt: string;
  } | null>(null);
  const [syncBusy, setSyncBusy] = useState(false);
  const [syncError, setSyncError] = useState("");

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
  const hasTaskUnsaved = useMemo(() => {
    if (!taskDraft || taskEditorMode === "none") return false;
    if (taskEditorMode === "create") return taskDraftDirty(taskDraft);
    return !!selectedTask && !taskDraftMatches(selectedTask, taskDraft);
  }, [selectedTask, taskDraft, taskEditorMode]);
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
    setPendingTaskReadback(null);
    setTaskEditorMode("none");
    setSelectedTaskId("");
    setTaskDraft(null);
    setSyncBusy(false);
    setSyncError("");
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
      setSelectedTaskId("");
      setTaskDraft(null);
      setTaskEditorMode("none");
      return;
    }
    if (!taskDraft) {
      const nextDraft = taskToDraft(selectedTask);
      setTaskDraft(nextDraft);
    }
  }, [selectedTaskId, selectedTask, taskDraft, taskEditorMode]);

  useEffect(() => {
    if (!pendingTaskReadback) return;
    if (taskEditorMode !== "edit" || !selectedTaskId) {
      setPendingTaskReadback(null);
      return;
    }
    if (!selectedTask || selectedTask.id !== pendingTaskReadback.taskId) return;
    if (String(selectedTask.updated_at || "") === pendingTaskReadback.previousUpdatedAt) return;
    setTaskDraft(taskToDraft(selectedTask));
    setPendingTaskReadback(null);
  }, [pendingTaskReadback, selectedTask, selectedTaskId, taskEditorMode]);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 180, tolerance: 6 } }),
  );

  const formatDoneTransitionGuardMessage = useCallback(
    (missing: string[]) =>
      tr("context.taskDoneGuard", "Complete closeout before marking this task done: {{items}}.", {
        items: missing.join(", "),
      }),
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
      "Only tasks that never moved past planned can be deleted.",
    );
  }, [selectedTask, selectedTaskDeleteInfo, tr]);

  const confirmDiscardTaskChanges = useCallback(() => {
    if (!hasTaskUnsaved || typeof window === "undefined") return true;
    return window.confirm(
      tr("context.unsavedTaskConfirm", "You have unsaved task edits. Discard them and continue?"),
    );
  }, [hasTaskUnsaved, tr]);

  const showTask = useCallback((task: Task, options?: { draft?: TaskDraft; error?: string }) => {
    const nextDraft = options?.draft ?? taskToDraft(task);
    setPendingTaskReadback(null);
    setSelectedTaskId(task.id);
    setTaskDraft(nextDraft);
    setTaskEditorMode("edit");
    setSyncError(options?.error || "");
    setActiveView("tasks");
  }, []);

  const moveTaskToStatus = useCallback(
    async (task: Task, nextStatus: BoardStatus) => {
      if (!groupId) return;
      if (taskStatus(task) === nextStatus) return;
      const visit = modalVisitRef.current;
      const isCurrent = () => modalVisitRef.current === visit;
      let transitionTask = task;
      if (nextStatus === "done") {
        const request = ++taskOpenSeqRef.current;
        const selection = selectedTaskIdRef.current;
        const draft = taskDraftRef.current;
        const unsavedDraft =
          selectedTaskId === task.id && taskEditorMode === "edit" && hasTaskUnsaved
            ? taskDraft
            : null;
        let guardSource = unsavedDraft;
        if (!guardSource) {
          if (!confirmDiscardTaskChanges()) return;
          const current = await loadTaskDetail(task.id);
          if (!current || !isCurrent()) return;
          transitionTask = current;
          if (taskStatus(current) === nextStatus) return;
          guardSource = taskToDraft(current);
        }
        const blockers = getTaskDoneTransitionBlockers({
          parentId: guardSource.parentId,
          taskType: guardSource.taskType,
          assignee: guardSource.assignee,
          outcome: guardSource.outcome,
          notes: guardSource.notes,
          checklist: guardSource.checklist,
        });
        // Existing edits must be saved through the editor, rather than relying on
        // unsaved closeout fields to authorize a status-only write.
        if (blockers.length > 0 || unsavedDraft) {
          if (
            taskOpenSeqRef.current !== request ||
            selectedTaskIdRef.current !== selection ||
            taskDraftRef.current !== draft
          ) {
            return;
          }
          showTask(transitionTask, {
            draft: { ...guardSource, status: "done" },
            error: blockers.length > 0 ? formatDoneTransitionGuardMessage(blockers) : "",
          });
          return;
        }
      }

      setSyncBusy(true);
      setSyncError("");
      try {
        const resp = await contextSync(groupId, [
          { op: "task.move", task_id: transitionTask.id, status: nextStatus },
        ]);
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
        if (!isCurrent()) return;
        if (selectedTaskId === transitionTask.id && taskEditorMode === "edit") {
          setPendingTaskReadback({
            taskId: transitionTask.id,
            previousUpdatedAt: String(transitionTask.updated_at || ""),
          });
        }
        await refreshTasks();
        if (isCurrent() && selectedTaskId === transitionTask.id)
          await loadTaskDetail(transitionTask.id);
      } finally {
        if (isCurrent()) setSyncBusy(false);
      }
    },
    [
      applyContextWriteback,
      confirmDiscardTaskChanges,
      formatDoneTransitionGuardMessage,
      groupId,
      hasTaskUnsaved,
      loadTaskDetail,
      refreshTasks,
      selectedTaskId,
      showTask,
      taskDraft,
      taskEditorMode,
      tr,
    ],
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
    setPendingTaskReadback(null);
    setTaskEditorMode("none");
    setSelectedTaskId("");
    setTaskDraft(null);
    setSyncError("");
  }, [confirmDiscardTaskChanges]);

  const handleSwitchActiveView = useCallback(
    (next: ContextModalView) => {
      if (next !== "tasks") {
        if (!confirmDiscardTaskChanges()) return;
        setPendingTaskReadback(null);
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

  const { modalRef } = useModalA11y(isOpen, handleModalClose);
  const closeAgentsMdExpanded = useCallback(() => setAgentsMdExpanded(false), []);
  const { modalRef: agentsMdExpandedRef } = useModalA11y(agentsMdExpanded, closeAgentsMdExpanded);

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
    if (
      String(taskDraft.status || "")
        .trim()
        .toLowerCase() === "done"
    ) {
      const blockers = getTaskDoneTransitionBlockers({
        parentId: taskDraft.parentId,
        taskType: taskDraft.taskType,
        assignee: taskDraft.assignee,
        outcome: taskDraft.outcome,
        notes: taskDraft.notes,
        checklist: taskDraft.checklist,
      });
      if (blockers.length > 0) {
        setSyncError(formatDoneTransitionGuardMessage(blockers));
        return;
      }
    }

    const visit = modalVisitRef.current;
    const isCurrent = () => modalVisitRef.current === visit;
    setSyncBusy(true);
    setSyncError("");
    try {
      if (taskEditorMode === "create") {
        const resp = await contextSync(groupId, [
          {
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
            checklist: parseChecklist(taskDraft.checklist, []),
          },
        ]);
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
        if (!isCurrent()) return;
        await refreshTasks();
        if (!isCurrent()) return;
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
        task_type: taskDraft.taskType,
        notes: taskDraft.notes,
        checklist: parseChecklist(taskDraft.checklist, selectedTask.checklist),
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
      if (!isCurrent()) return;
      setPendingTaskReadback({
        taskId: selectedTask.id,
        previousUpdatedAt: String(selectedTask.updated_at || ""),
      });
      await refreshTasks();
      if (isCurrent()) await loadTaskDetail(selectedTask.id);
    } finally {
      if (isCurrent()) setSyncBusy(false);
    }
  }, [
    applyContextWriteback,
    formatDoneTransitionGuardMessage,
    groupId,
    loadTaskDetail,
    refreshTasks,
    selectedTask,
    taskDraft,
    taskEditorMode,
    tr,
  ]);

  const handleDeleteTask = useCallback(
    async (task: Task) => {
      if (!groupId) return;
      const deleteInfo = deleteInfoById[task.id];
      if (!deleteInfo?.allowed) return;
      if (!confirmDiscardTaskChanges()) return;
      if (typeof window !== "undefined") {
        const confirmed = window.confirm(
          deleteInfo.total > 1
            ? tr(
                "context.deleteTaskCascadeConfirm",
                'Delete task "{{title}}" and its {{count}} unexecuted tasks?',
                { title: taskTitle(task) || task.id, count: deleteInfo.total },
              )
            : tr("context.deleteTaskConfirm", 'Delete task "{{title}}" permanently?', {
                title: taskTitle(task) || task.id,
              }),
        );
        if (!confirmed) return;
      }

      const visit = modalVisitRef.current;
      const isCurrent = () => modalVisitRef.current === visit;
      setSyncBusy(true);
      setSyncError("");
      try {
        const resp = await deleteCoordinationTask(groupId, task.id);
        const nextResp = await applyContextWriteback(resp);
        if (!nextResp.ok) {
          if (isCurrent()) {
            setSyncError(
              nextResp.error?.message ||
                resp.error?.message ||
                tr("context.failedToDeleteTask", "Failed to delete task"),
            );
          }
          return;
        }
        if (!isCurrent()) return;
        if (selectedTaskId === task.id) {
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
      refreshTasks,
      selectedTaskId,
      tr,
    ],
  );

  const handleResetTask = useCallback(() => {
    if (taskEditorMode === "create") {
      setTaskDraft(emptyTaskDraft("planned"));
      setSyncError("");
      return;
    }
    if (!selectedTask) return;
    const nextDraft = taskToDraft(selectedTask);
    setTaskDraft(nextDraft);
    setSyncError("");
  }, [selectedTask, taskEditorMode]);

  const handleOpenCreate = useCallback(
    (status: BoardStatus = "planned") => {
      if (!confirmDiscardTaskChanges()) return;
      setPendingTaskReadback(null);
      setTaskEditorMode("create");
      setSelectedTaskId("");
      setTaskDraft(emptyTaskDraft(status));
      setSyncError("");
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
            {syncError ? (
              <div
                className={classNames(
                  "rounded-xl border px-3 py-2 text-sm",
                  "border-rose-500/30 bg-rose-500/15 text-rose-600 dark:text-rose-400",
                )}
              >
                {syncError}
              </div>
            ) : null}

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
                    <div className="min-h-0 flex-1 overflow-x-hidden overflow-y-auto">
                      <TaskEditorPanel
                        tr={tr}
                        ui={ui}
                        taskEditorMode={taskEditorMode}
                        taskDraft={taskDraft}
                        hasTaskUnsaved={hasTaskUnsaved}
                        syncBusy={syncBusy}
                        selectedTask={selectedTask}
                        subtasks={selectedSubtasks}
                        selectedTaskDeleteInfo={selectedTaskDeleteInfo}
                        selectedTaskDeleteHint={selectedTaskDeleteHint}
                        taskWorkflowCoverage={taskWorkflowCoverage}
                        taskTypeId={taskDraft?.taskType || "standard"}
                        selectedTaskType={selectedTaskType}
                        setTaskDraft={setTaskDraft}
                        onTaskTypeChange={(nextTaskTypeId) =>
                          setTaskDraft((prev) =>
                            prev ? { ...prev, taskType: nextTaskTypeId } : prev,
                          )
                        }
                        onResetTask={handleResetTask}
                        onClose={closeTaskEditor}
                        onDeleteSelectedTask={() => {
                          if (selectedTask) void handleDeleteTask(selectedTask);
                        }}
                        onSaveTask={() => void handleSaveTask()}
                        onOpenTask={openSubtask}
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
