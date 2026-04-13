import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
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
  fetchGroupLearning,
  fetchGroupPrompts,
  updateCoordinationBrief,
  updateCoordinationTask,
  updateGroupPrompt,
  type GroupPromptInfo,
  type ApiResponse,
} from "../../services/api";
import { reloadContextAfterWrite } from "../../features/contextModal/contextWriteback";
import type {
  GroupContext,
  GroupLearningSnapshot,
  GroupSettings,
  ProjectMdInfo,
  Task,
} from "../../types";
import {
  evaluateTaskWorkflow,
  getTaskDoneTransitionBlockers,
  getTaskTypeDefinition,
} from "../../utils/taskWorkflow";
import { classNames } from "../../utils/classNames";
import { useModalA11y } from "../../hooks/useModalA11y";
import { useLatestRequest } from "../../hooks/useLatestRequest";
import { ModalFrame } from "../modals/ModalFrame";
import { settingsDialogBodyClass, settingsDialogPanelClass } from "../modals/settings/types";
import { parseHelpMarkdown, updatePetHelpNote } from "../../utils/helpMarkdown";
import { AgentsView } from "./agents/AgentsView";
import { ProjectPanel } from "./coordination/ProjectPanel";
import { SteeringPanel } from "./coordination/SteeringPanel";
import { TaskBoard } from "./coordination/TaskBoard";
import { TaskEditorPanel } from "./coordination/TaskEditorPanel";
import { DesktopPetView } from "./desktopPet/DesktopPetView";
import {
  briefDraftMatches,
  briefToDraft,
  buildBoard,
  countLike,
  emptyNoteDraft,
  emptyTaskDraft,
  getTaskDeleteInfo,
  parseChecklist,
  parseLineList,
  resolvePetPersonaDraft,
  isVisibleContextAgent,
  petPersonaDraftDirty,
  taskDisplaySummary,
  taskDraftDirty,
  taskDraftMatches,
  taskStatus,
  taskToDraft,
  type BoardStatus,
  type BriefDraft,
  type ContextModalView,
  type NoteDraft,
  type SteeringTab,
  type TaskDraft,
  type TaskFilterValue,
} from "./model";
import { createContextModalUi } from "./ui";

const LearningPanel = lazy(() => import("./learning/LearningPanel").then((module) => ({ default: module.LearningPanel })));

interface ContextModalProps {
  isOpen: boolean;
  onClose: () => void;
  groupId: string;
  context: GroupContext | null;
  onOpenContext: () => Promise<void>;
  onSyncContext: () => Promise<void>;
  isDark: boolean;
  settings?: GroupSettings | null;
  onUpdateSettings?: (settings: Partial<GroupSettings>) => Promise<boolean | void>;
}

export function ContextModal({
  isOpen,
  onClose,
  groupId,
  context,
  onOpenContext,
  onSyncContext,
  isDark,
  settings,
  onUpdateSettings,
}: ContextModalProps) {
  const { t } = useTranslation("modals");
  const tr = useCallback((key: string, fallback: string, vars?: Record<string, unknown>) =>
    String(t(key as never, { defaultValue: fallback, ...(vars || {}) } as never)), [t]);

  const ui = useMemo(() => createContextModalUi(isDark), [isDark]);

  const [activeView, setActiveView] = useState<ContextModalView>("coordination");
  const [steeringTab, setSteeringTab] = useState<SteeringTab>("summary");
  const [taskFilter, setTaskFilter] = useState<TaskFilterValue>("all");
  const [assigneeFilter, setAssigneeFilter] = useState<string>("__all__");
  const [taskQuery, setTaskQuery] = useState("");
  const [dragTaskId, setDragTaskId] = useState("");
  const [archivedExpanded, setArchivedExpanded] = useState(false);
  const [editingBrief, setEditingBrief] = useState(false);
  const [briefDraft, setBriefDraft] = useState<BriefDraft>(briefToDraft(null));
  const [taskEditorMode, setTaskEditorMode] = useState<"none" | "create" | "edit">("none");
  const [selectedTaskId, setSelectedTaskId] = useState("");
  const [taskDraft, setTaskDraft] = useState<TaskDraft | null>(null);
  const [pendingTaskReadback, setPendingTaskReadback] = useState<{ taskId: string; previousUpdatedAt: string } | null>(null);
  const [syncBusy, setSyncBusy] = useState(false);
  const [syncError, setSyncError] = useState("");
  const [viewBusy, setViewBusy] = useState(false);

  const [projectMd, setProjectMd] = useState<ProjectMdInfo | null>(null);
  const [projectBusy, setProjectBusy] = useState(false);
  const [projectError, setProjectError] = useState("");
  const [projectNotice, setProjectNotice] = useState("");
  const [editingProject, setEditingProject] = useState(false);
  const [projectText, setProjectText] = useState("");
  const [projectExpanded, setProjectExpanded] = useState(false);
  const [notifyAgents, setNotifyAgents] = useState(false);
  const [notifyError, setNotifyError] = useState("");

  const [petHelpPrompt, setPetHelpPrompt] = useState<GroupPromptInfo | null>(null);
  const [petPersonaDraft, setPetPersonaDraft] = useState("");
  const [petPersonaBusy, setPetPersonaBusy] = useState(false);
  const [petPersonaError, setPetPersonaError] = useState("");
  const [petPersonaNotice, setPetPersonaNotice] = useState("");

  const [decisionDraft, setDecisionDraft] = useState<NoteDraft>(emptyNoteDraft());
  const [handoffDraft, setHandoffDraft] = useState<NoteDraft>(emptyNoteDraft());
  const [activityBusyKind, setActivityBusyKind] = useState<"decision" | "handoff" | null>(null);
  const [activityError, setActivityError] = useState("");
  const [learningSnapshot, setLearningSnapshot] = useState<GroupLearningSnapshot | null>(null);
  const [learningBusy, setLearningBusy] = useState(false);
  const [learningError, setLearningError] = useState("");
  const [learningLoadedGroupId, setLearningLoadedGroupId] = useState("");
  const lastOpenedGroupRef = useRef("");
  const projectRequest = useLatestRequest();
  const learningRequest = useLatestRequest();
  const petPersonaRequest = useLatestRequest();

  const brief = context?.coordination?.brief || null;
  const desktopPetEnabled = Boolean(settings?.desktop_pet_enabled);
  const tasks = useMemo(() => (Array.isArray(context?.coordination?.tasks) ? context.coordination.tasks : []), [context]);
  const agents = useMemo(
    () => (Array.isArray(context?.agent_states) ? context.agent_states.filter((agent) => isVisibleContextAgent(agent)) : []),
    [context]
  );
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
  const taskWorkflowCoverage = useMemo(
    () => evaluateTaskWorkflow({
      parentId: taskDraft?.parentId,
      taskType: taskDraft?.taskType,
      status: taskDraft?.status,
      assignee: taskDraft?.assignee,
      outcome: taskDraft?.outcome,
      notes: taskDraft?.notes,
      checklist: taskDraft?.checklist,
    }),
    [taskDraft]
  );
  const selectedTaskType = useMemo(
    () => getTaskTypeDefinition(taskDraft?.taskType || "standard"),
    [taskDraft?.taskType]
  );

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
    () => tr("context.projectUpdatedNotify", "PROJECT.md updated. Please re-read and realign. ({{path}})", { path: projectPathLabel }),
    [projectPathLabel, tr]
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
  const savedPetPersona = useMemo(
    () => (petHelpPrompt ? parseHelpMarkdown(String(petHelpPrompt.content || "")).pet : ""),
    [petHelpPrompt]
  );
  const hasPetPersonaUnsaved = useMemo(
    () => petPersonaDraftDirty(savedPetPersona, petPersonaDraft, { loaded: petHelpPrompt !== null }),
    [petHelpPrompt, petPersonaDraft, savedPetPersona]
  );
  const hasSteeringUnsaved = hasBriefUnsaved || hasProjectUnsaved || hasPetPersonaUnsaved;
  const hasTaskUnsaved = useMemo(() => {
    if (!taskDraft || taskEditorMode === "none") return false;
    if (taskEditorMode === "create") return taskDraftDirty(taskDraft);
    return !!selectedTask && !taskDraftMatches(selectedTask, taskDraft);
  }, [selectedTask, taskDraft, taskEditorMode]);
  const selectedTaskDeleteInfo = useMemo(() => getTaskDeleteInfo(selectedTask, tasks), [selectedTask, tasks]);
  const taskEditorVisible = taskEditorMode !== "none" && !!taskDraft;

  const loadProjectMd = useCallback(async (force: boolean = false): Promise<ProjectMdInfo | null> => {
    if (!groupId) return null;
    if (!force && projectMd !== null) return projectMd;
    setProjectBusy(true);
    setProjectError("");
    let nextProject: ProjectMdInfo | null = null;
    await projectRequest.runLatest({
      run: (signal) => apiJson<ProjectMdInfo>(`/api/v1/groups/${encodeURIComponent(groupId)}/project_md`, { signal }),
      onSuccess: (resp) => {
        if (!resp.ok) {
          setProjectMd(null);
          setProjectError(resp.error?.message || tr("context.failedToLoadProject", "Failed to load PROJECT.md"));
          return;
        }
        nextProject = resp.result ?? null;
        setProjectMd(resp.result);
      },
      onSettled: () => setProjectBusy(false),
    });
    return nextProject;
  }, [groupId, projectMd, projectRequest, tr]);

  const loadLearningSnapshot = useCallback(async (force: boolean = false): Promise<GroupLearningSnapshot | null> => {
    if (!groupId) return null;
    if (!force && learningSnapshot !== null && learningLoadedGroupId === groupId) {
      return learningSnapshot;
    }
    setLearningBusy(true);
    setLearningError("");
    let nextSnapshot: GroupLearningSnapshot | null = null;
    await learningRequest.runLatest({
      run: (signal) => fetchGroupLearning(groupId, { signal }),
      onSuccess: (resp) => {
        if (!resp.ok) {
          setLearningError(resp.error?.message || tr("context.failedToLoadLearning", "Failed to load learning board"));
          return;
        }
        nextSnapshot = resp.result ?? null;
        setLearningSnapshot(resp.result);
        setLearningLoadedGroupId(groupId);
      },
      onSettled: () => setLearningBusy(false),
    });
    return nextSnapshot;
  }, [groupId, learningLoadedGroupId, learningRequest, learningSnapshot, tr]);

  const loadPetPersona = useCallback(async (force: boolean = false): Promise<GroupPromptInfo | null> => {
    if (!groupId) return null;
    if (!force && petHelpPrompt !== null) return petHelpPrompt;
    setPetPersonaBusy(true);
    setPetPersonaError("");
    let nextHelpPrompt: GroupPromptInfo | null = null;
    await petPersonaRequest.runLatest({
      run: (signal) => fetchGroupPrompts(groupId, { signal, noCache: true }),
      onSuccess: (resp) => {
        if (!resp.ok) {
          setPetHelpPrompt(null);
          setPetPersonaError(resp.error?.message || tr("context.failedToLoadPetPersona", "Failed to load pet persona"));
          return;
        }
        const nextHelp = resp.result?.help ?? null;
        if (!nextHelp) {
          setPetHelpPrompt(null);
          setPetPersonaError(tr("context.failedToLoadPetPersona", "Failed to load pet persona"));
          return;
        }
        nextHelpPrompt = nextHelp;
        setPetHelpPrompt(nextHelp);
        const parsedPet = parseHelpMarkdown(String(nextHelp.content || "")).pet;
        setPetPersonaDraft(resolvePetPersonaDraft(parsedPet));
      },
      onSettled: () => setPetPersonaBusy(false),
    });
    return nextHelpPrompt;
  }, [groupId, petHelpPrompt, petPersonaRequest, tr]);

  useEffect(() => {
    if (!isOpen || !groupId) return;

    setActiveView("coordination");
    setSteeringTab("summary");
    setTaskFilter("all");
    setAssigneeFilter("__all__");
    setTaskQuery("");
    setArchivedExpanded(false);
    setDragTaskId("");
    setPendingTaskReadback(null);
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
    setProjectExpanded(false);
    setPetHelpPrompt(null);
    setPetPersonaDraft("");
    setPetPersonaBusy(false);
    setPetPersonaError("");
    setPetPersonaNotice("");
    setNotifyError("");
    setNotifyAgents(false);
    setDecisionDraft(emptyNoteDraft());
    setHandoffDraft(emptyNoteDraft());
    setActivityBusyKind(null);
    setActivityError("");
    setLearningSnapshot(null);
    setLearningBusy(false);
    setLearningError("");
    setLearningLoadedGroupId("");
    projectRequest.cancelLatest();
    learningRequest.cancelLatest();
    petPersonaRequest.cancelLatest();
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
    async <T,>(response: ApiResponse<T>) =>
      reloadContextAfterWrite(response, onSyncContext),
    [onSyncContext],
  );

  useEffect(() => {
    if (!isOpen || !groupId) return;
    if ((steeringTab === "project" || editingProject) && projectMd === null && !projectBusy) {
      void loadProjectMd();
    }
  }, [editingProject, groupId, isOpen, loadProjectMd, projectBusy, projectMd, steeringTab]);

  useEffect(() => {
    if (!isOpen || !groupId || activeView !== "coordination" || steeringTab !== "learning") return;
    const missingSkillsField =
      learningSnapshot !== null && !Array.isArray((learningSnapshot as GroupLearningSnapshot & { skills?: unknown }).skills);
    if ((learningSnapshot === null || missingSkillsField) && !learningBusy) {
      void loadLearningSnapshot(true);
    }
  }, [activeView, groupId, isOpen, learningBusy, learningSnapshot, loadLearningSnapshot, steeringTab]);

  useEffect(() => {
    if (!isOpen || !groupId || activeView !== "desktop_pet") return;
    if (petHelpPrompt === null && !petPersonaBusy) {
      void loadPetPersona();
    }
  }, [activeView, groupId, isOpen, loadPetPersona, petHelpPrompt, petPersonaBusy]);

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
          taskDisplaySummary(task),
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

  const hasTaskFilters = taskFilter !== "all" || assigneeFilter !== "__all__" || taskQuery.trim().length > 0;
  const hiddenArchivedMatches = archivedExpanded ? 0 : filteredBoard.archived.length;
  const visibleTaskTotal = useMemo(
    () => filteredBoard.planned.length + filteredBoard.active.length + filteredBoard.done.length + (archivedExpanded ? filteredBoard.archived.length : 0),
    [archivedExpanded, filteredBoard]
  );
  const hasVisibleTasks = visibleTaskTotal > 0;
  const archivedToggleCount = hasTaskFilters ? filteredBoard.archived.length : Number(tasksSummary.archived || 0);
  const hasArchivedTasks = archivedExpanded || archivedToggleCount > 0 || Number(tasksSummary.archived || 0) > 0;

  useEffect(() => {
    if (!isOpen) return;
    if (selectedTask && taskStatus(selectedTask) === "archived") {
      setArchivedExpanded(true);
    }
  }, [isOpen, selectedTask]);

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
    useSensor(TouchSensor, { activationConstraint: { delay: 180, tolerance: 6 } })
  );

  const formatDoneTransitionGuardMessage = useCallback(
    (missing: string[]) => tr(
      "context.taskDoneGuard",
      "Complete closeout before marking this task done: {{items}}.",
      { items: missing.join(", ") }
    ),
    [tr]
  );
  const selectedTaskDeleteHint = useMemo(() => {
    if (!selectedTask || selectedTaskDeleteInfo.allowed || !selectedTaskDeleteInfo.reason) return "";
    if (selectedTaskDeleteInfo.reason === "subtree_history") {
      return tr("context.deleteTaskBlockedSubtree", "This task has child tasks with execution history, so delete is blocked.");
    }
    return tr("context.deleteTaskBlockedSelf", "Only tasks that never moved past planned can be deleted.");
  }, [selectedTask, selectedTaskDeleteInfo, tr]);

  const confirmDiscardTaskChanges = useCallback(() => {
    if (!hasTaskUnsaved || typeof window === "undefined") return true;
    return window.confirm(tr("context.unsavedTaskConfirm", "You have unsaved task edits. Discard them and continue?"));
  }, [hasTaskUnsaved, tr]);

  const openTaskEditorForTask = useCallback((task: Task, options?: { draft?: TaskDraft; error?: string }): boolean => {
    const sameTask = selectedTaskId === task.id && taskEditorMode === "edit";
    if (!sameTask && !confirmDiscardTaskChanges()) return false;
    const nextDraft = options?.draft ?? taskToDraft(task);
    if (!sameTask) {
      setPendingTaskReadback(null);
    }
    setSelectedTaskId(task.id);
    setTaskDraft(nextDraft);
    setTaskEditorMode("edit");
    setSyncError(options?.error || "");
    setActiveView("coordination");
    return true;
  }, [confirmDiscardTaskChanges, selectedTaskId, taskEditorMode]);

  const moveTaskToStatus = useCallback(async (task: Task, nextStatus: BoardStatus) => {
    if (!groupId) return;
    if (taskStatus(task) === nextStatus) return;
    if (nextStatus === "done") {
      const guardSource = selectedTaskId === task.id && taskEditorMode === "edit" && taskDraft
        ? taskDraft
        : taskToDraft(task);
      const blockers = getTaskDoneTransitionBlockers({
        parentId: guardSource.parentId,
        taskType: guardSource.taskType,
        assignee: guardSource.assignee,
        outcome: guardSource.outcome,
        notes: guardSource.notes,
        checklist: guardSource.checklist,
      });
      if (blockers.length > 0) {
        openTaskEditorForTask(task, {
          draft: { ...guardSource, status: "done" },
          error: formatDoneTransitionGuardMessage(blockers),
        });
        return;
      }
    }

    setSyncBusy(true);
    setSyncError("");
    try {
      const resp = await updateCoordinationTask(groupId, {
        ...task,
        status: nextStatus,
      });
      const nextResp = await applyContextWriteback(resp);
      if (!nextResp.ok) {
        setSyncError(nextResp.error?.message || resp.error?.message || tr("context.failedToApplyChanges", "Failed to apply changes"));
        return;
      }
      if (selectedTaskId === task.id && taskEditorMode === "edit") {
        setPendingTaskReadback({
          taskId: task.id,
          previousUpdatedAt: String(task.updated_at || ""),
        });
      }
    } finally {
      setSyncBusy(false);
    }
  }, [applyContextWriteback, formatDoneTransitionGuardMessage, groupId, openTaskEditorForTask, selectedTaskId, taskDraft, taskEditorMode, tr]);

  const handleDragStart = useCallback((event: DragStartEvent) => {
    const id = String(event.active.id || "");
    if (id.startsWith("task:")) setDragTaskId(id.slice(5));
  }, []);

  const handleDragEnd = useCallback((event: DragEndEvent) => {
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
  }, [moveTaskToStatus, taskMap]);

  const selectTask = useCallback((task: Task) => {
    if (selectedTaskId === task.id && taskEditorMode === "edit") return;
    if (!confirmDiscardTaskChanges()) return;
    const nextDraft = taskToDraft(task);
    setPendingTaskReadback(null);
    setSelectedTaskId(task.id);
    setTaskDraft(nextDraft);
    setTaskEditorMode("edit");
    setSyncError("");
    setActiveView("coordination");
  }, [confirmDiscardTaskChanges, selectedTaskId, taskEditorMode]);

  const closeTaskEditor = useCallback(() => {
    if (!confirmDiscardTaskChanges()) return;
    setPendingTaskReadback(null);
    setTaskEditorMode("none");
    setSelectedTaskId("");
    setTaskDraft(null);
    setSyncError("");
  }, [confirmDiscardTaskChanges]);

  const openSteeringTab = useCallback((tab: SteeringTab) => {
    if (!confirmDiscardTaskChanges()) return;
    setPendingTaskReadback(null);
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
    if (tab === "learning") {
      void loadLearningSnapshot();
    }
  }, [confirmDiscardTaskChanges, loadLearningSnapshot, loadProjectMd]);

  const handleSwitchActiveView = useCallback((next: ContextModalView) => {
    if (next !== "coordination") {
      if (!confirmDiscardTaskChanges()) return;
      setPendingTaskReadback(null);
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
  const closeProjectExpanded = useCallback(() => setProjectExpanded(false), []);
  const { modalRef: projectExpandedRef } = useModalA11y(projectExpanded, closeProjectExpanded);

  const handleToggleDesktopPet = useCallback(async (enabled: boolean) => {
    if (!onUpdateSettings) return;
    setViewBusy(true);
    try {
      await onUpdateSettings({ desktop_pet_enabled: enabled });
    } finally {
      setViewBusy(false);
    }
  }, [onUpdateSettings]);

  const handleSavePetPersona = useCallback(async () => {
    if (!groupId) return;
    setPetPersonaBusy(true);
    setPetPersonaError("");
    setPetPersonaNotice("");
    try {
      const currentHelp = petHelpPrompt ?? await loadPetPersona();
      if (!currentHelp) return;
      const actorOrder = Object.keys(parseHelpMarkdown(String(currentHelp.content || "")).actorNotes);
      const nextContent = updatePetHelpNote(String(currentHelp.content || ""), petPersonaDraft, actorOrder);
      const resp = await updateGroupPrompt(groupId, "help", nextContent, {
        editorMode: "structured",
        changedBlocks: ["pet"],
      });
      if (!resp.ok) {
        setPetPersonaError(resp.error?.message || tr("context.failedToSavePetPersona", "Failed to save pet persona"));
        return;
      }
      setPetHelpPrompt(resp.result);
      setPetPersonaDraft(resolvePetPersonaDraft(parseHelpMarkdown(String(resp.result.content || "")).pet));
      setPetPersonaNotice(tr("context.petPersonaSaved", "Pet persona saved."));
    } finally {
      setPetPersonaBusy(false);
    }
  }, [groupId, loadPetPersona, petHelpPrompt, petPersonaDraft, tr]);

  const handleDiscardPetPersona = useCallback(() => {
    setPetPersonaDraft(resolvePetPersonaDraft(savedPetPersona));
    setPetPersonaError("");
    setPetPersonaNotice("");
  }, [savedPetPersona]);

  const handleSaveBrief = useCallback(async () => {
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
      const nextResp = await applyContextWriteback(resp);
      if (!nextResp.ok) {
        setSyncError(nextResp.error?.message || resp.error?.message || tr("context.failedToApplyChanges", "Failed to apply changes"));
        return;
      }
      setEditingBrief(false);
    } finally {
      setSyncBusy(false);
    }
  }, [applyContextWriteback, briefDraft, groupId, tr]);

  const handleSaveTask = useCallback(async () => {
    if (!groupId || !taskDraft) return;
    const title = taskDraft.title.trim();
    if (!title) {
      setSyncError(tr("context.taskTitleRequired", "Task title is required."));
      return;
    }
    if (String(taskDraft.status || "").trim().toLowerCase() === "done") {
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
          task_type: taskDraft.taskType,
          notes: taskDraft.notes,
          checklist: parseChecklist(taskDraft.checklist, []),
        }]);
        const nextResp = await applyContextWriteback(resp);
        if (!nextResp.ok) {
          setSyncError(nextResp.error?.message || resp.error?.message || tr("context.failedToApplyChanges", "Failed to apply changes"));
          return;
        }
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
        setSyncError(nextResp.error?.message || resp.error?.message || tr("context.failedToApplyChanges", "Failed to apply changes"));
        return;
      }
      setPendingTaskReadback({
        taskId: selectedTask.id,
        previousUpdatedAt: String(selectedTask.updated_at || ""),
      });
    } finally {
      setSyncBusy(false);
    }
  }, [applyContextWriteback, formatDoneTransitionGuardMessage, groupId, selectedTask, taskDraft, taskEditorMode, tr]);

  const handleDeleteTask = useCallback(async (task: Task) => {
    if (!groupId) return;
    const deleteInfo = getTaskDeleteInfo(task, tasks);
    if (!deleteInfo.allowed) return;
    if (!confirmDiscardTaskChanges()) return;
    if (typeof window !== "undefined") {
      const confirmed = window.confirm(
        deleteInfo.total > 1
          ? tr("context.deleteTaskCascadeConfirm", "Delete task \"{{title}}\" and its {{count}} unexecuted tasks?", {
              title: taskTitle(task) || task.id,
              count: deleteInfo.total,
            })
          : tr("context.deleteTaskConfirm", "Delete task \"{{title}}\" permanently?", {
              title: taskTitle(task) || task.id,
            })
      );
      if (!confirmed) return;
    }

    setSyncBusy(true);
    setSyncError("");
    try {
      const resp = await deleteCoordinationTask(groupId, task.id);
      const nextResp = await applyContextWriteback(resp);
      if (!nextResp.ok) {
        setSyncError(nextResp.error?.message || resp.error?.message || tr("context.failedToDeleteTask", "Failed to delete task"));
        return;
      }
      if (selectedTaskId === task.id) {
        setTaskEditorMode("none");
        setSelectedTaskId("");
        setTaskDraft(null);
      }
    } finally {
      setSyncBusy(false);
    }
  }, [applyContextWriteback, confirmDiscardTaskChanges, groupId, selectedTaskId, tasks, tr]);

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

  const handleOpenCreate = useCallback((status: BoardStatus = "planned") => {
    if (!confirmDiscardTaskChanges()) return;
    setPendingTaskReadback(null);
    setTaskEditorMode("create");
    setSelectedTaskId("");
    setTaskDraft(emptyTaskDraft(status));
    setSyncError("");
    setActiveView("coordination");
  }, [confirmDiscardTaskChanges]);

  const handleEditProject = useCallback(async () => {
    openSteeringTab("project");
    const loaded = await loadProjectMd(true);
    setProjectText(String(loaded?.content || ""));
    setEditingProject(true);
  }, [loadProjectMd, openSteeringTab]);

  const handleSaveProject = useCallback(async () => {
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
      const nextResp = await applyContextWriteback(resp);
      if (!nextResp.ok) {
        setProjectError(nextResp.error?.message || resp.error?.message || tr("context.failedToSaveProject", "Failed to save PROJECT.md"));
        return;
      }
      setProjectMd(nextResp.result);
      setEditingProject(false);

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
  }, [applyContextWriteback, groupId, notifyAgents, notifyMessage, projectText, tr]);

  const handleAddCoordinationNote = useCallback(async (kind: "decision" | "handoff") => {
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
      const nextResp = await applyContextWriteback(resp);
      if (!nextResp.ok) {
        setActivityError(nextResp.error?.message || resp.error?.message || tr("context.failedToApplyChanges", "Failed to apply changes"));
        return;
      }
      if (kind === "decision") {
        setDecisionDraft(emptyNoteDraft());
      } else {
        setHandoffDraft(emptyNoteDraft());
      }
    } finally {
      setActivityBusyKind(null);
    }
  }, [applyContextWriteback, decisionDraft, groupId, handoffDraft, tr]);

  const handlePetPersonaChange = useCallback((value: string) => {
    setPetPersonaDraft(value);
    setPetPersonaError("");
    setPetPersonaNotice("");
  }, []);

  const startBriefEdit = useCallback(() => {
    openSteeringTab("summary");
    setEditingBrief(true);
  }, [openSteeringTab]);

  const cancelBriefEdit = useCallback(() => {
    setEditingBrief(false);
    setBriefDraft(briefToDraft(brief));
  }, [brief]);

  const projectPanel = (
    <ProjectPanel
      expanded={false}
      isDark={isDark}
      tr={tr}
      ui={ui}
      projectBusy={projectBusy}
      projectError={projectError}
      notifyError={notifyError}
      projectNotice={projectNotice}
      projectPathLabel={projectPathLabel}
      editingProject={editingProject}
      projectMd={projectMd}
      projectText={projectText}
      notifyAgents={notifyAgents}
      onExpand={() => setProjectExpanded(true)}
      onCancelEdit={() => {
        setEditingProject(false);
        setProjectText(String(projectMd?.content || ""));
        setNotifyAgents(false);
      }}
      onEditProject={() => void handleEditProject()}
      onProjectTextChange={setProjectText}
      onNotifyAgentsChange={setNotifyAgents}
      onSaveProject={() => void handleSaveProject()}
    />
  );

  const learningPanel = (
    <Suspense fallback={<section className={classNames(ui.surfaceClass, "rounded-xl border p-4 text-sm", "glass-card text-[var(--color-text-secondary)]")}>{tr("context.loading", "Loading…")}</section>}>
      <LearningPanel
        groupId={groupId}
        tr={tr}
        ui={ui}
        data={learningSnapshot}
        loading={learningBusy}
        error={learningError}
        onRefresh={() => void loadLearningSnapshot(true)}
      />
    </Suspense>
  );

  if (!isOpen) return null;

  const viewButtonClass = (active: boolean) => classNames(
    "rounded-xl px-3 py-2 text-sm font-medium transition-colors",
    active
      ? "bg-[var(--glass-accent-bg)] text-[var(--color-accent-primary)]"
      : "text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]"
  );

  return (
    <>
      <ModalFrame
        isDark={isDark}
        onClose={handleModalClose}
        titleId="context-modal-title"
        title={tr("context.title", "Project Context")}
        closeAriaLabel={tr("context.closeAria", "Close context modal")}
        panelClassName="h-full w-full overflow-hidden rounded-none sm:h-[94vh] sm:max-w-[96vw]"
        modalRef={modalRef}
      >
        <div className="min-h-0 flex-1 overflow-y-auto">
          <div className="flex min-h-full flex-col gap-4 p-4 sm:p-5">
            {syncError ? <div className={classNames("rounded-xl border px-3 py-2 text-sm", "border-rose-500/30 bg-rose-500/15 text-rose-600 dark:text-rose-400")}>{syncError}</div> : null}

            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div className={classNames("inline-flex w-fit rounded-2xl border p-1", isDark ? "border-slate-800 bg-slate-950/70" : "border-gray-200 bg-gray-100/80")}>
                <button type="button" onClick={() => handleSwitchActiveView("coordination")} className={viewButtonClass(activeView === "coordination")}>{tr("context.coordination", "Coordination")}</button>
                <button type="button" onClick={() => handleSwitchActiveView("agents")} className={viewButtonClass(activeView === "agents")}>{tr("context.agents", "Agents")}</button>
                <button type="button" onClick={() => handleSwitchActiveView("desktop_pet")} className={viewButtonClass(activeView === "desktop_pet")}>{tr("context.desktopPetTab", "Web Pet")}<span className="ml-1.5 rounded-md bg-cyan-500/15 px-1.5 py-0.5 text-[10px] font-semibold leading-none text-cyan-400">Beta</span></button>
              </div>
            </div>

            {activeView === "coordination" ? (
              <div className="space-y-4">
                <SteeringPanel
                  tr={tr}
                  ui={ui}
                  brief={brief}
                  tasksSummary={tasksSummary}
                  attentionCounts={attentionCounts}
                  unassignedCount={unassignedCount}
                  steeringTab={steeringTab}
                  editingBrief={editingBrief}
                  briefDraft={briefDraft}
                  syncBusy={syncBusy}
                  activityBusyKind={activityBusyKind}
                  activityError={activityError}
                  recentDecisions={recentDecisions}
                  recentHandoffs={recentHandoffs}
                  decisionDraft={decisionDraft}
                  handoffDraft={handoffDraft}
                  activeTaskOptions={activeTaskOptions}
                  projectPanel={projectPanel}
                  learningPanel={learningPanel}
                  onOpenSteeringTab={openSteeringTab}
                  onStartBriefEdit={startBriefEdit}
                  onCancelBriefEdit={cancelBriefEdit}
                  onSaveBrief={() => void handleSaveBrief()}
                  onBriefDraftChange={(updater) => setBriefDraft((prev) => updater(prev))}
                  onDecisionDraftChange={(updater) => setDecisionDraft((prev) => updater(prev))}
                  onHandoffDraftChange={(updater) => setHandoffDraft((prev) => updater(prev))}
                  onAddCoordinationNote={(kind) => void handleAddCoordinationNote(kind)}
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
                  taskMap={taskMap}
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
                  onMoveTaskToStatus={(task, nextStatus) => void moveTaskToStatus(task, nextStatus)}
                />
              </div>
            ) : activeView === "agents" ? (
              <AgentsView agents={agents} tr={tr} ui={ui} />
            ) : (
              <DesktopPetView
                tr={tr}
                ui={ui}
                onUpdateSettings={onUpdateSettings}
                desktopPetEnabled={desktopPetEnabled}
                viewBusy={viewBusy}
                petHelpPrompt={petHelpPrompt}
                petPersonaBusy={petPersonaBusy}
                petPersonaError={petPersonaError}
                petPersonaNotice={petPersonaNotice}
                petPersonaDraft={petPersonaDraft}
                hasPetPersonaUnsaved={hasPetPersonaUnsaved}
                onToggleDesktopPet={(enabled) => void handleToggleDesktopPet(enabled)}
                onLoadPetPersona={loadPetPersona}
                onSavePetPersona={() => void handleSavePetPersona()}
                onDiscardPetPersona={handleDiscardPetPersona}
                onPetPersonaChange={handlePetPersonaChange}
              />
            )}
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
                  <div className="h-full overflow-y-auto border-l border-[var(--glass-border-subtle)] shadow-2xl glass-modal">
                    <div className="sr-only" id="context-task-drawer-title">
                      {taskEditorMode === "create" ? tr("context.newTask", "New task") : tr("context.taskDetails", "Task editor")}
                    </div>
                    <TaskEditorPanel
                      tr={tr}
                      ui={ui}
                      taskEditorMode={taskEditorMode}
                      taskDraft={taskDraft}
                      hasTaskUnsaved={hasTaskUnsaved}
                      syncBusy={syncBusy}
                      selectedTask={selectedTask}
                      selectedTaskDeleteInfo={selectedTaskDeleteInfo}
                      selectedTaskDeleteHint={selectedTaskDeleteHint}
                      taskWorkflowCoverage={taskWorkflowCoverage}
                      taskTypeId={taskDraft?.taskType || "standard"}
                      selectedTaskType={selectedTaskType}
                      setTaskDraft={setTaskDraft}
                      onTaskTypeChange={(nextTaskTypeId) => setTaskDraft((prev) => (prev ? {
                        ...prev,
                        taskType: nextTaskTypeId,
                      } : prev))}
                      onResetTask={handleResetTask}
                      onClose={closeTaskEditor}
                      onDeleteSelectedTask={() => {
                        if (selectedTask) void handleDeleteTask(selectedTask);
                      }}
                      onSaveTask={() => void handleSaveTask()}
                    />
                  </div>
                </div>
              </div>
            </div>,
            document.body
          )
        : null}

      {projectExpanded && typeof document !== "undefined"
        ? createPortal(
            <div
              className="fixed inset-0 z-[70] animate-fade-in"
              role="dialog"
              aria-modal="true"
              onPointerDown={(event) => {
                if (event.target === event.currentTarget) setProjectExpanded(false);
              }}
            >
              <div className="absolute inset-0 glass-overlay" />
              <div ref={projectExpandedRef} className={settingsDialogPanelClass("xl")}>
                <div className="flex shrink-0 justify-end border-b border-[var(--glass-border-subtle)] px-3 py-2 sm:px-4 sm:py-3">
                  <button type="button" className={ui.buttonSecondaryClass} onClick={() => setProjectExpanded(false)}>
                    {tr("common:close", "Close")}
                  </button>
                </div>
                <div className={settingsDialogBodyClass}>
                  <ProjectPanel
                    expanded
                    isDark={isDark}
                    tr={tr}
                    ui={ui}
                    projectBusy={projectBusy}
                    projectError={projectError}
                    notifyError={notifyError}
                    projectNotice={projectNotice}
                    projectPathLabel={projectPathLabel}
                    editingProject={editingProject}
                    projectMd={projectMd}
                    projectText={projectText}
                    notifyAgents={notifyAgents}
                    onExpand={() => {}}
                    onCancelEdit={() => {
                      setEditingProject(false);
                      setProjectText(String(projectMd?.content || ""));
                      setNotifyAgents(false);
                    }}
                    onEditProject={() => void handleEditProject()}
                    onProjectTextChange={setProjectText}
                    onNotifyAgentsChange={setNotifyAgents}
                    onSaveProject={() => void handleSaveProject()}
                  />
                </div>
              </div>
            </div>,
            document.body
          )
        : null}
    </>
  );
}

function taskTitle(task: Task | null | undefined): string {
  if (!task) return "";
  return String(task.title || task.id || "").trim();
}
