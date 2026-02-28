import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiJson, contextSync, fetchTasks } from "../services/api";
import { GroupContext, ProjectMdInfo, Task } from "../types";
import { formatFullTime, formatTime } from "../utils/time";
import { classNames } from "../utils/classNames";
import { MarkdownRenderer } from "./MarkdownRenderer";
import { MermaidDiagram } from "./MermaidDiagram";
import { useModalA11y } from "../hooks/useModalA11y";
import { ModalFrame } from "./modals/ModalFrame";
import { ProjectSavedNotifyModal } from "./modals/context/ProjectSavedNotifyModal";

interface ContextModalProps {
  isOpen: boolean;
  onClose: () => void;
  groupId: string;
  context: GroupContext | null;
  onRefreshContext: () => Promise<void>;
  onUpdateVision: (vision: string) => Promise<void>;
  busy: boolean;
  isDark: boolean;
}

type ContextOp = { op: string } & Record<string, unknown>;
type ContextTabId = "strategy" | "execution" | "charter";

export function ContextModal({
  isOpen,
  onClose,
  groupId,
  context,
  onRefreshContext,
  onUpdateVision,
  busy,
  isDark,
}: ContextModalProps) {
  const { t } = useTranslation("modals");
  const tr = (key: string, defaultValue: string, vars?: Record<string, unknown>): string =>
    String(t(key as never, { defaultValue, ...(vars || {}) } as never));
  const { modalRef } = useModalA11y(isOpen, onClose);
  const [editingVision, setEditingVision] = useState(false);
  const [visionText, setVisionText] = useState("");
  const [editingOverview, setEditingOverview] = useState(false);
  const [overviewFocusText, setOverviewFocusText] = useState("");
  const [overviewRolesText, setOverviewRolesText] = useState("");
  const [overviewCollabText, setOverviewCollabText] = useState("");

  // PROJECT.md state (project constitution)
  const [projectMd, setProjectMd] = useState<ProjectMdInfo | null>(null);
  const [projectBusy, setProjectBusy] = useState(false);
  const [projectError, setProjectError] = useState("");
  const [editingProject, setEditingProject] = useState(false);
  const [projectText, setProjectText] = useState("");

  // Post-save notify modal
  const [showNotifyModal, setShowNotifyModal] = useState(false);
  const [notifyAgents, setNotifyAgents] = useState(false);
  const [notifyBusy, setNotifyBusy] = useState(false);
  const [notifyError, setNotifyError] = useState("");

  const [tasks, setTasks] = useState<Task[] | null>(null);
  const [tasksBusy, setTasksBusy] = useState(false);
  const [tasksError, setTasksError] = useState("");
  const [showMermaidModal, setShowMermaidModal] = useState(false);
  const [mermaidZoom, setMermaidZoom] = useState(1);

  const [syncBusy, setSyncBusy] = useState(false);
  const [syncError, setSyncError] = useState("");
  const [activeTab, setActiveTab] = useState<ContextTabId>("strategy");

  const tabStorageKey = useMemo(() => {
    const gid = String(groupId || "").trim();
    return gid ? `cccc.context.tab.${gid}` : "";
  }, [groupId]);

  useEffect(() => {
    if (!isOpen) return;
    let nextTab: ContextTabId = "strategy";
    if (tabStorageKey && typeof window !== "undefined") {
      try {
        const raw = String(window.sessionStorage.getItem(tabStorageKey) || "").trim();
        if (raw === "strategy" || raw === "execution" || raw === "charter") {
          nextTab = raw;
        }
      } catch {
        // ignore sessionStorage read failures
      }
    }
    setActiveTab(nextTab);
  }, [isOpen, tabStorageKey]);

  useEffect(() => {
    if (!isOpen || !tabStorageKey || typeof window === "undefined") return;
    try {
      window.sessionStorage.setItem(tabStorageKey, activeTab);
    } catch {
      // ignore sessionStorage write failures
    }
  }, [activeTab, isOpen, tabStorageKey]);

  const projectPathLabel = useMemo(() => {
    const p = projectMd?.path ? String(projectMd.path) : "";
    if (p) return p;
    return "PROJECT.md";
  }, [projectMd?.path]);

  const notifyMessage = useMemo(() => {
    return t("context.projectUpdatedNotify", { path: projectPathLabel });
  }, [projectPathLabel, t]);

  useEffect(() => {
    if (!isOpen) return;
    if (!groupId) return;
    let cancelled = false;

    setProjectBusy(true);
    setProjectError("");
    setEditingProject(false);
    setEditingOverview(false);
    setShowMermaidModal(false);
    setMermaidZoom(1);
    setNotifyError("");
    setShowNotifyModal(false);

    setTasksBusy(true);
    setTasksError("");

    setSyncBusy(false);
    setSyncError("");

    void (async () => {
      const resp = await apiJson<ProjectMdInfo>(`/api/v1/groups/${encodeURIComponent(groupId)}/project_md`);
      if (cancelled) return;
      if (!resp.ok) {
        setProjectMd(null);
        setProjectError(resp.error?.message || t("context.failedToLoadProject"));
        setProjectBusy(false);
      } else {
        setProjectMd(resp.result);
        setProjectBusy(false);
      }
    })();

    void (async () => {
      const resp = await fetchTasks(groupId);
      if (cancelled) return;
      if (!resp.ok) {
        setTasks(null);
        setTasksError(resp.error?.message || t("context.failedToLoadTasks"));
        setTasksBusy(false);
        return;
      }
      setTasks(Array.isArray(resp.result?.tasks) ? resp.result.tasks : []);
      setTasksBusy(false);
    })();

    return () => {
      cancelled = true;
    };
  }, [groupId, isOpen, t]);

  const tasksByStatus = useMemo(() => {
    const list = Array.isArray(tasks) ? tasks : [];
    const normalize = (s: unknown) => String(s || "planned").toLowerCase();
    const active: Task[] = [];
    const planned: Task[] = [];
    const done: Task[] = [];
    const archived: Task[] = [];
    const other: Task[] = [];
    for (const tk of list) {
      const st = normalize(tk.status);
      if (st === "active") active.push(tk);
      else if (st === "done") done.push(tk);
      else if (st === "archived") archived.push(tk);
      else if (st === "planned") planned.push(tk);
      else other.push(tk);
    }
    return { active, planned, done, archived, other };
  }, [tasks]);

  const runOps = async (ops: ContextOp[]): Promise<boolean> => {
    if (!groupId) return false;
    setSyncBusy(true);
    setSyncError("");
    try {
      const resp = await contextSync(groupId, ops);
      if (!resp.ok) {
        setSyncError(resp.error?.message || t("context.failedToApplyChanges"));
        return false;
      }
      await onRefreshContext();

      const needsTaskRefresh = ops.some((o) => String(o.op || "").startsWith("task."));
      if (needsTaskRefresh) {
        const tResp = await fetchTasks(groupId);
        if (tResp.ok) {
          setTasks(Array.isArray(tResp.result?.tasks) ? tResp.result.tasks : []);
        } else {
          setTasksError(tResp.error?.message || t("context.failedToLoadTasks"));
        }
      }
      return true;
    } catch (e) {
      setSyncError(e instanceof Error ? e.message : "Unknown error");
      return false;
    } finally {
      setSyncBusy(false);
    }
  };

  const handleArchiveTask = async (taskId: string) => {
    const prevTasks = tasks;
    setTasks((prev) => (Array.isArray(prev) ? prev.map((tk) => (tk.id === taskId ? { ...tk, status: "archived" } : tk)) : prev));
    const ok = await runOps([{ op: "task.status", task_id: taskId, status: "archived" }]);
    if (!ok) {
      setTasks(prevTasks);
    }
  };

  const handleRestoreTask = async (taskId: string) => {
    await runOps([{ op: "task.restore", task_id: taskId }]);
  };

  const handleEditVision = () => {
    setVisionText(context?.vision || "");
    setEditingVision(true);
  };

  const handleSaveVision = async () => {
    await onUpdateVision(visionText);
    setEditingVision(false);
  };

  const handleEditOverview = () => {
    const manual = context?.overview?.manual;
    setOverviewFocusText(String(manual?.current_focus || ""));
    setOverviewCollabText(String(manual?.collaboration_mode || ""));
    setOverviewRolesText(Array.isArray(manual?.roles) ? manual!.roles!.join(", ") : "");
    setEditingOverview(true);
  };

  const handleSaveOverview = async () => {
    const roles = String(overviewRolesText || "")
      .split(/[\n,]/)
      .map((x) => x.trim())
      .filter(Boolean);
    const ok = await runOps([
      {
        op: "overview.manual.update",
        current_focus: overviewFocusText,
        collaboration_mode: overviewCollabText,
        roles,
      },
    ]);
    if (ok) setEditingOverview(false);
  };

  const handleEditProject = () => {
    setProjectText(projectMd?.content ? String(projectMd.content) : "");
    setProjectError("");
    setEditingProject(true);
  };

  const handleSaveProject = async () => {
    if (!groupId) return;
    setProjectBusy(true);
    setProjectError("");
    setNotifyError("");
    try {
      const resp = await apiJson<ProjectMdInfo>(`/api/v1/groups/${encodeURIComponent(groupId)}/project_md`, {
        method: "PUT",
        body: JSON.stringify({ content: projectText, by: "user" }),
      });
      if (!resp.ok) {
        setProjectError(resp.error?.message || t("context.failedToSaveProject"));
        return;
      }
      setProjectMd(resp.result);
      setEditingProject(false);
      setNotifyAgents(false);
      setShowNotifyModal(true);
    } finally {
      setProjectBusy(false);
    }
  };

  const handleNotifyDone = async () => {
    if (!groupId) {
      setShowNotifyModal(false);
      return;
    }
    setNotifyBusy(true);
    setNotifyError("");
    try {
      if (notifyAgents) {
        const resp = await apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/send`, {
          method: "POST",
          body: JSON.stringify({ text: notifyMessage, by: "user", to: ["@all"], path: "" }),
        });
        if (!resp.ok) {
          setNotifyError(resp.error?.message || t("context.failedToNotify"));
          return;
        }
      }
      setShowNotifyModal(false);
    } finally {
      setNotifyBusy(false);
    }
  };

  const zoomInMermaid = () => setMermaidZoom((z) => Math.min(2.5, Math.round((z + 0.1) * 10) / 10));
  const zoomOutMermaid = () => setMermaidZoom((z) => Math.max(0.6, Math.round((z - 0.1) * 10) / 10));
  const resetMermaidZoom = () => setMermaidZoom(1);

  // Task card renderer (shared across status groups)
  const renderTaskCard = (tk: Task, showArchive: boolean) => (
    <div key={tk.id} className={`px-3 py-2 rounded-lg ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
      <div className="flex items-start gap-2">
        <span className={`text-[11px] px-1.5 py-0.5 rounded ${isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"}`}>
          {tk.id}
        </span>
        <div className="min-w-0 flex-1">
          <div className={`text-sm font-medium truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>{tk.name}</div>
          {tk.goal ? (
            <MarkdownRenderer
              content={String(tk.goal)}
              isDark={isDark}
              className={classNames("text-xs mt-0.5", isDark ? "text-slate-400" : "text-gray-600")}
            />
          ) : null}
          {(tk.parent_id || tk.assignee) ? (
            <div className={`text-[11px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
              {tk.parent_id ? `↑ ${tk.parent_id}` : ""}
              {tk.parent_id && tk.assignee ? " · " : ""}
              {tk.assignee ? t("context.assigneeLabel", { name: tk.assignee }) : ""}
            </div>
          ) : null}
        </div>
        {showArchive ? (
          <button
            type="button"
            disabled={syncBusy}
            onClick={() => void handleArchiveTask(tk.id)}
            className={classNames(
              "text-[11px] px-2 py-0.5 rounded flex-shrink-0 transition-colors disabled:opacity-50",
              isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
            )}
          >
            {t("context.archive")}
          </button>
        ) : (
          <button
            type="button"
            disabled={syncBusy}
            onClick={() => void handleRestoreTask(tk.id)}
            className={classNames(
              "text-[11px] px-2 py-0.5 rounded flex-shrink-0 transition-colors disabled:opacity-50",
              isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
            )}
          >
            {t("context.restore")}
          </button>
        )}
      </div>
    </div>
  );

  const agentStates = context?.presence?.agents || [];
  const agentCount = agentStates.length;
  const blockedAgentCount = agentStates.filter((a) => Array.isArray(a.blockers) && a.blockers.length > 0).length;
  const mermaidChart = String(context?.overview?.mermaid || "").trim();
  const sectionCardClass = classNames(
    "rounded-2xl border p-4 shadow-sm",
    isDark ? "border-slate-700/80 bg-slate-900/45" : "border-gray-200 bg-white/85"
  );
  const sectionTitleClass = classNames(
    "text-sm font-semibold tracking-wide",
    isDark ? "text-slate-200" : "text-gray-800"
  );
  const tabs: Array<{ id: ContextTabId; label: string }> = [
    {
      id: "strategy",
      label: tr("context.tabStrategy", "Strategy"),
    },
    {
      id: "execution",
      label: tr("context.tabExecution", "Execution"),
    },
    {
      id: "charter",
      label: tr("context.tabCharter", "Charter"),
    },
  ];

  if (!isOpen) return null;

  return (
    <>
      <ModalFrame
        isDark={isDark}
        onClose={onClose}
        titleId="context-modal-title"
        title={t("context.title")}
        closeAriaLabel={t("context.closeAria")}
        panelClassName="w-full h-full sm:h-auto sm:max-h-[92vh] sm:max-w-[96vw] 2xl:max-w-[1460px]"
        modalRef={modalRef}
      >
        {/* Content */}
        <div className="flex-1 overflow-y-auto px-5 pb-6 pt-4 space-y-5">
          <div
            className={classNames(
              "sticky top-0 z-10 -mx-1 rounded-xl px-1 py-1 backdrop-blur",
              isDark ? "bg-slate-950/85" : "bg-white/85"
            )}
          >
            <div
              role="tablist"
              aria-label={tr("context.tabsAria", "Context sections")}
              className={classNames(
                "flex items-center gap-1 rounded-xl border p-1",
                isDark ? "border-slate-700 bg-slate-900/70" : "border-gray-200 bg-gray-100/90"
              )}
            >
              {tabs.map((tab) => {
                const selected = activeTab === tab.id;
                return (
                  <button
                    key={tab.id}
                    type="button"
                    role="tab"
                    aria-selected={selected}
                    onClick={() => setActiveTab(tab.id)}
                    className={classNames(
                      "flex-1 rounded-lg px-2.5 py-1.5 text-center transition-colors",
                      selected
                        ? isDark
                          ? "bg-slate-200 text-slate-900"
                          : "bg-white text-gray-900 shadow-sm"
                        : isDark
                          ? "text-slate-300 hover:bg-slate-800"
                          : "text-gray-600 hover:bg-gray-200"
                    )}
                  >
                    <div className="text-xs font-semibold tracking-wide">{tab.label}</div>
                  </button>
                );
              })}
            </div>
          </div>

          {activeTab === "strategy" ? (
            <div className="space-y-4">
              <section className={sectionCardClass}>
                <div className={sectionTitleClass}>{t("context.vision")}</div>
                <div className="mt-2">
                  {!editingVision && (
                    <div className="flex justify-end mb-2">
                      <button
                        onClick={handleEditVision}
                        className={`text-xs min-h-[36px] px-2 rounded transition-colors ${isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-500 hover:text-gray-700"
                          }`}
                      >
                        ✏️ {t("context.editButton")}
                      </button>
                    </div>
                  )}
                  {editingVision ? (
                    <div className="space-y-2">
                      <textarea
                        value={visionText}
                        onChange={(e) => setVisionText(e.target.value)}
                        className={`w-full h-32 px-3 py-2 border rounded-lg text-sm resize-none transition-colors ${isDark
                          ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500"
                          : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                          }`}
                        placeholder={t("context.visionPlaceholder")}
                      />
                      <div className="flex gap-2">
                        <button
                          onClick={handleSaveVision}
                          disabled={busy}
                          className="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm rounded-lg disabled:opacity-50 min-h-[44px] transition-colors"
                        >
                          {busy ? t("common:loading") : t("common:save")}
                        </button>
                        <button
                          onClick={() => setEditingVision(false)}
                          className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors ${isDark
                            ? "bg-slate-700 hover:bg-slate-600 text-slate-200"
                            : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                            }`}
                        >
                          {t("common:cancel")}
                        </button>
                      </div>
                    </div>
                  ) : (
                    <div className={`px-3 py-2 rounded-lg text-sm min-h-[60px] ${isDark ? "bg-slate-800/50 text-slate-300" : "bg-gray-50 text-gray-700"
                      }`}>
                      {context?.vision ? (
                        <MarkdownRenderer content={context.vision} isDark={isDark} />
                      ) : (
                        <span className={isDark ? "text-slate-500 italic" : "text-gray-400 italic"}>{t("context.noVision")}</span>
                      )}
                    </div>
                  )}
                </div>
              </section>

              <section className={sectionCardClass}>
                <div className={sectionTitleClass}>{tr("context.overview", "Overview")}</div>
                <div className="mt-2">
                  {!editingOverview && (
                    <div className="flex justify-end mb-2">
                      <button
                        onClick={handleEditOverview}
                        className={`text-xs min-h-[36px] px-2 rounded transition-colors ${isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-500 hover:text-gray-700"}`}
                      >
                        ✏️ {t("context.editButton")}
                      </button>
                    </div>
                  )}

                  {editingOverview ? (
                    <div className="space-y-2">
                      <div>
                        <div className={`text-[11px] mb-1 font-medium uppercase tracking-wide ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                          {tr("context.currentFocus", "Current Focus")}
                        </div>
                        <textarea
                          value={overviewFocusText}
                          onChange={(e) => setOverviewFocusText(e.target.value)}
                          className={`w-full h-24 px-3 py-2 border rounded-lg text-sm resize-none transition-colors ${isDark
                            ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500"
                            : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                            }`}
                          placeholder={tr("context.currentFocusPlaceholder", "What is the team's current focus?")}
                        />
                      </div>

                      <div>
                        <div className={`text-[11px] mb-1 font-medium uppercase tracking-wide ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                          {tr("context.roles", "Roles")}
                        </div>
                        <input
                          value={overviewRolesText}
                          onChange={(e) => setOverviewRolesText(e.target.value)}
                          className={`w-full px-3 py-2 border rounded-lg text-sm transition-colors ${isDark
                            ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500"
                            : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                            }`}
                          placeholder={tr("context.rolesPlaceholder", "foreman, reviewer, impl")}
                        />
                      </div>

                      <div>
                        <div className={`text-[11px] mb-1 font-medium uppercase tracking-wide ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                          {tr("context.collaborationMode", "Collaboration")}
                        </div>
                        <input
                          value={overviewCollabText}
                          onChange={(e) => setOverviewCollabText(e.target.value)}
                          className={`w-full px-3 py-2 border rounded-lg text-sm transition-colors ${isDark
                            ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500"
                            : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                            }`}
                          placeholder={tr("context.collaborationPlaceholder", "How does the team collaborate?")}
                        />
                      </div>

                      <div className="flex gap-2">
                        <button
                          onClick={() => void handleSaveOverview()}
                          disabled={syncBusy}
                          className="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm rounded-lg disabled:opacity-50 min-h-[44px] transition-colors"
                        >
                          {syncBusy ? t("common:loading") : t("common:save")}
                        </button>
                        <button
                          onClick={() => setEditingOverview(false)}
                          disabled={syncBusy}
                          className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors disabled:opacity-50 ${isDark
                            ? "bg-slate-700 hover:bg-slate-600 text-slate-200"
                            : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                            }`}
                        >
                          {t("common:cancel")}
                        </button>
                      </div>
                    </div>
                  ) : (
                    <>
                      <div className={`px-3 py-2 rounded-lg space-y-2 ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                        {context?.overview?.manual?.current_focus && (
                          <div>
                            <div className={`text-[11px] font-medium uppercase tracking-wide ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                              {tr("context.currentFocus", "Current Focus")}
                            </div>
                            <MarkdownRenderer
                              content={String(context.overview.manual.current_focus)}
                              isDark={isDark}
                              className={classNames("text-sm mt-0.5", isDark ? "text-slate-300" : "text-gray-700")}
                            />
                          </div>
                        )}
                        {context?.overview?.manual?.roles && context.overview.manual.roles.length > 0 && (
                          <div>
                            <div className={`text-[11px] font-medium uppercase tracking-wide ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                              {tr("context.roles", "Roles")}
                            </div>
                            <div className="flex flex-wrap gap-1 mt-0.5">
                              {context.overview.manual.roles.map((role, i) => (
                                <span key={i} className={classNames("text-xs px-2 py-0.5 rounded", isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700")}>
                                  {role}
                                </span>
                              ))}
                            </div>
                          </div>
                        )}
                        {context?.overview?.manual?.collaboration_mode && (
                          <div>
                            <div className={`text-[11px] font-medium uppercase tracking-wide ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                              {tr("context.collaborationMode", "Collaboration")}
                            </div>
                            <div className={`text-sm mt-0.5 ${isDark ? "text-slate-300" : "text-gray-700"}`}>
                              {context.overview.manual.collaboration_mode}
                            </div>
                          </div>
                        )}
                        {(!context?.overview?.manual?.current_focus &&
                          !context?.overview?.manual?.collaboration_mode &&
                          (!context?.overview?.manual?.roles || context.overview.manual.roles.length === 0)) && (
                            <span className={isDark ? "text-slate-500 italic text-sm" : "text-gray-400 italic text-sm"}>
                              {tr("context.noOverview", "No overview set")}
                            </span>
                          )}
                      </div>

                      {context?.overview?.mermaid ? (
                        <div className="mt-3">
                          <div className={`mb-1 text-[11px] font-medium uppercase tracking-wide ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                            {tr("context.overviewMermaid", "Project Panorama")}
                          </div>
                          <div className={classNames("mb-2 text-[11px]", isDark ? "text-slate-500" : "text-gray-500")}>
                            {tr("context.diagramHint", "Click diagram to open full-screen and zoom")}
                          </div>
                          <button
                            type="button"
                            onClick={() => {
                              setMermaidZoom(1);
                              setShowMermaidModal(true);
                            }}
                            className={classNames(
                              "w-full cursor-zoom-in text-left rounded-xl border p-2 overflow-hidden transition-colors",
                              isDark ? "border-slate-700 bg-slate-950/40 hover:bg-slate-900/60" : "border-gray-200 bg-white hover:bg-gray-50"
                            )}
                          >
                            <MermaidDiagram
                              chart={String(context.overview.mermaid)}
                              isDark={isDark}
                              fitMode="contain"
                              className="max-h-[520px] min-h-[280px]"
                            />
                          </button>
                        </div>
                      ) : null}
                    </>
                  )}
                </div>
              </section>
            </div>
          ) : null}

          {activeTab === "execution" ? (
            <div className="space-y-4">
              <section className={sectionCardClass}>
                <div className={sectionTitleClass}>{t("context.tasks")}</div>
                <div className="mt-2">
                  {context?.tasks_summary ? (
                    <div className="space-y-2">
                      <div className={`px-3 py-2 rounded-lg text-xs ${isDark ? "bg-slate-800/50 text-slate-300" : "bg-gray-50 text-gray-700"}`}>
                        {t("context.tasksSummary", { total: context.tasks_summary.total, active: context.tasks_summary.active, planned: context.tasks_summary.planned, done: context.tasks_summary.done })}
                      </div>
                      <div className={`px-3 py-2 rounded-lg text-xs ${isDark ? "bg-slate-900/50 text-slate-400" : "bg-gray-50 text-gray-600"}`}>
                        {tr("context.agentSummaryInline", "agents {{agents}} · blocked {{blocked}}", {
                          agents: agentCount,
                          blocked: blockedAgentCount,
                        })}
                      </div>

                      {syncError && (
                        <div className={`text-xs rounded-lg border px-3 py-2 ${isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-300 bg-rose-50 text-rose-700"}`}>
                          {syncError}
                        </div>
                      )}

                      {tasksError ? (
                        <div className={`px-3 py-2 rounded-lg text-sm ${isDark ? "bg-rose-500/10 text-rose-300 border border-rose-500/30" : "bg-rose-50 text-rose-700 border border-rose-300"}`}>
                          {tasksError}
                        </div>
                      ) : tasksBusy ? (
                        <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                          {t("context.loadingTasks")}
                        </div>
                      ) : (
                        <div className="space-y-2">
                          {(tasksByStatus.active.length + tasksByStatus.planned.length + tasksByStatus.done.length + tasksByStatus.archived.length + tasksByStatus.other.length) === 0 ? (
                            <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                              {t("context.noTasks")}
                            </div>
                          ) : (
                            <>
                              <details open>
                                <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                                  {t("context.statusActive")} ({tasksByStatus.active.length})
                                </summary>
                                <div className="mt-2 space-y-2">
                                  {tasksByStatus.active.length === 0 ? (
                                    <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                                      {t("context.noActiveTask")}
                                    </div>
                                  ) : tasksByStatus.active.map((tk) => renderTaskCard(tk, true))}
                                </div>
                              </details>

                              <details open>
                                <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                                  {t("context.statusPlanned")} ({tasksByStatus.planned.length})
                                </summary>
                                <div className="mt-2 space-y-2">
                                  {tasksByStatus.planned.length === 0 ? (
                                    <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                                      {t("context.noPlannedTasks")}
                                    </div>
                                  ) : tasksByStatus.planned.map((tk) => renderTaskCard(tk, true))}
                                </div>
                              </details>

                              <details>
                                <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                                  {t("context.statusDone")} ({tasksByStatus.done.length})
                                </summary>
                                <div className="mt-2 space-y-2">
                                  {tasksByStatus.done.length === 0 ? (
                                    <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                                      {t("context.noDoneTasks")}
                                    </div>
                                  ) : tasksByStatus.done.map((tk) => renderTaskCard(tk, true))}
                                </div>
                              </details>

                              <details>
                                <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                                  {t("context.statusArchived")} ({tasksByStatus.archived.length})
                                </summary>
                                <div className="mt-2 space-y-2">
                                  {tasksByStatus.archived.length === 0 ? (
                                    <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                                      {t("context.noArchivedTasks")}
                                    </div>
                                  ) : tasksByStatus.archived.map((tk) => renderTaskCard(tk, false))}
                                </div>
                              </details>
                            </>
                          )}
                        </div>
                      )}
                    </div>
                  ) : (
                    <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"
                      }`}>
                      {t("context.noTaskSummary")}
                    </div>
                  )}
                </div>
              </section>

              <section className={sectionCardClass}>
                <div className={sectionTitleClass}>{tr("context.agents", "Agent State")}</div>
                <div className="mt-2">
                  {context?.presence?.agents && context.presence.agents.length > 0 ? (
                    <div className="space-y-2">
                      {context.presence.agents.map((a) => (
                        <div key={a.id} className={`px-3 py-2 rounded-lg ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                          <div className="flex items-center gap-2">
                            <span className={`text-sm font-medium ${isDark ? "text-slate-200" : "text-gray-800"}`}>{a.id}</span>
                            {a.updated_at ? (
                              <span
                                className={classNames(
                                  "ml-auto text-xs tabular-nums",
                                  isDark ? "text-slate-400" : "text-gray-500"
                                )}
                                title={formatFullTime(a.updated_at || "")}
                              >
                                {t("context.updated", { time: formatTime(a.updated_at || "") })}
                              </span>
                            ) : null}
                          </div>
                          {a.focus ? (
                            <MarkdownRenderer
                              content={String(a.focus)}
                              isDark={isDark}
                              className={classNames("text-xs mt-1", isDark ? "text-slate-300" : "text-gray-700")}
                            />
                          ) : (
                            <div className={`text-xs mt-1 italic ${isDark ? "text-slate-500" : "text-gray-500"}`}>{tr("context.noAgentStateYet", "No agent update yet")}</div>
                          )}
                          <div className={classNames("mt-2 pl-3 border-l-2 space-y-1", isDark ? "border-slate-700" : "border-gray-200")}>
                            {a.active_task_id && (
                              <div className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                                <span className="font-medium">{tr("context.fieldTask", "Task")}:</span> {a.active_task_id}
                              </div>
                            )}
                            {a.blockers && a.blockers.length > 0 && (
                              <div className={`text-xs ${isDark ? "text-rose-400/80" : "text-rose-600"}`}>
                                <span className="font-medium">{tr("context.fieldBlockers", "Blockers")}:</span> {a.blockers.join(", ")}
                              </div>
                            )}
                            {a.next_action && (
                              <div className={`text-xs ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                                <span className="font-medium">{tr("context.fieldNextAction", "Next")}:</span> {a.next_action}
                              </div>
                            )}
                            {a.what_changed && (
                              <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                <span className="font-medium">{tr("context.fieldWhatChanged", "Changed")}:</span> {a.what_changed}
                              </div>
                            )}
                            {a.decision_delta && (
                              <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                <span className="font-medium">{tr("context.fieldDecisionDelta", "Decision")}:</span> {a.decision_delta}
                              </div>
                            )}
                            {a.environment && (
                              <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                <span className="font-medium">{tr("context.fieldEnvironment", "Env")}:</span> {a.environment}
                              </div>
                            )}
                            {a.user_profile && (
                              <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                <span className="font-medium">{tr("context.fieldUserProfile", "User")}:</span> {a.user_profile}
                              </div>
                            )}
                            {a.notes && (
                              <div className={`text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                <span className="font-medium">{tr("context.fieldNotes", "Notes")}:</span> {a.notes}
                              </div>
                            )}
                          </div>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                      {tr("context.noAgents", "No agent state")}
                    </div>
                  )}
                </div>
              </section>
            </div>
          ) : null}

          {activeTab === "charter" ? (
            <div className="space-y-4">
              <section className={sectionCardClass}>
                <div className={sectionTitleClass}>{t("context.projectMd")}</div>
                <div className="mt-2">
                  <div className="flex items-center justify-between mb-2 gap-2">
                    <div className="min-w-0">
                      <div className={`text-[11px] truncate ${isDark ? "text-slate-500" : "text-gray-500"}`} title={projectPathLabel}>
                        {projectBusy ? t("common:loading") : projectMd?.found ? projectPathLabel : projectMd?.path ? t("context.missingPath", { path: projectMd.path }) : t("context.missingLabel")}
                      </div>
                    </div>
                    {!editingProject && (
                      <button
                        onClick={handleEditProject}
                        disabled={projectBusy || !groupId}
                        className={`text-xs min-h-[36px] px-2 rounded transition-colors disabled:opacity-50 ${isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-500 hover:text-gray-700"
                          }`}
                      >
                        {projectMd?.found ? `✏️ ${t("context.editButton")}` : `＋ ${t("context.createButton")}`}
                      </button>
                    )}
                  </div>
                  {projectError && (
                    <div className={`mb-2 text-xs rounded-lg border px-3 py-2 ${isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-300 bg-rose-50 text-rose-700"
                      }`}>
                      {projectError}
                    </div>
                  )}
                  {editingProject ? (
                    <div className="space-y-2">
                      <textarea
                        value={projectText}
                        onChange={(e) => setProjectText(e.target.value)}
                        className={`w-full h-72 px-3 py-2 border rounded-lg text-sm resize-none font-mono transition-colors ${isDark
                          ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500"
                          : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                          }`}
                        placeholder={t("context.writePlaceholder")}
                      />
                      <div className="flex gap-2">
                        <button
                          onClick={handleSaveProject}
                          disabled={projectBusy || !groupId}
                          className="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm rounded-lg disabled:opacity-50 min-h-[44px] transition-colors"
                        >
                          {projectBusy ? t("common:loading") : t("common:save")}
                        </button>
                        <button
                          onClick={() => setEditingProject(false)}
                          disabled={projectBusy}
                          className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors disabled:opacity-50 ${isDark
                            ? "bg-slate-700 hover:bg-slate-600 text-slate-200"
                            : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                            }`}
                        >
                          {t("common:cancel")}
                        </button>
                      </div>
                    </div>
                  ) : (
                    <div className={`px-3 py-2 rounded-lg text-sm min-h-[80px] max-h-[64vh] overflow-auto ${isDark ? "bg-slate-800/50 text-slate-300" : "bg-gray-50 text-gray-700"
                      }`}>
                      {projectMd?.found && projectMd.content ? (
                        <MarkdownRenderer
                          content={String(projectMd.content)}
                          isDark={isDark}
                        />
                      ) : (
                        <span className={isDark ? "text-slate-500 italic" : "text-gray-400 italic"}>{t("context.noProjectMd")}</span>
                      )}
                    </div>
                  )}
                </div>
              </section>
            </div>
          ) : null}

          {syncBusy && (
            <div className={classNames(
              "text-[11px] italic",
              isDark ? "text-slate-500" : "text-gray-500"
            )}>
              {t("context.applyingChanges")}
            </div>
          )}
        </div>
      </ModalFrame>

      {showMermaidModal && mermaidChart ? (
        <div className="fixed inset-0 z-overlay flex items-center justify-center p-2 sm:p-4">
          <div
            className={isDark ? "absolute inset-0 bg-black/75" : "absolute inset-0 bg-black/60"}
            onPointerDown={(e) => {
              if (e.target !== e.currentTarget) return;
              setShowMermaidModal(false);
            }}
            aria-hidden="true"
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-label={tr("context.overviewMermaid", "Project Panorama")}
            className={classNames(
              "relative w-full max-w-[98vw] rounded-xl border shadow-2xl p-3 sm:p-4",
              isDark ? "bg-slate-950 border-slate-700" : "bg-white border-gray-200"
            )}
          >
            <div className="flex flex-wrap items-center justify-between gap-2 mb-3">
              <div>
                <div className={classNames("text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>
                  {tr("context.overviewMermaid", "Project Panorama")}
                </div>
                <div className={classNames("text-[11px]", isDark ? "text-slate-500" : "text-gray-500")}>
                  {tr("context.zoomLevel", "Zoom {{percent}}%", { percent: Math.round(mermaidZoom * 100) })}
                </div>
              </div>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={zoomOutMermaid}
                  className={classNames(
                    "px-2 py-1 rounded text-xs border min-h-[32px]",
                    isDark ? "border-slate-600 text-slate-200 hover:bg-slate-800" : "border-gray-300 text-gray-700 hover:bg-gray-100"
                  )}
                >
                  {tr("context.zoomOut", "Zoom -")}
                </button>
                <button
                  type="button"
                  onClick={resetMermaidZoom}
                  className={classNames(
                    "px-2 py-1 rounded text-xs border min-h-[32px]",
                    isDark ? "border-slate-600 text-slate-200 hover:bg-slate-800" : "border-gray-300 text-gray-700 hover:bg-gray-100"
                  )}
                >
                  {tr("context.zoomReset", "Reset")}
                </button>
                <button
                  type="button"
                  onClick={zoomInMermaid}
                  className={classNames(
                    "px-2 py-1 rounded text-xs border min-h-[32px]",
                    isDark ? "border-slate-600 text-slate-200 hover:bg-slate-800" : "border-gray-300 text-gray-700 hover:bg-gray-100"
                  )}
                >
                  {tr("context.zoomIn", "Zoom +")}
                </button>
                <button
                  type="button"
                  onClick={() => setShowMermaidModal(false)}
                  className={classNames(
                    "px-2.5 py-1 rounded text-xs border min-h-[32px]",
                    isDark ? "border-slate-600 text-slate-200 hover:bg-slate-800" : "border-gray-300 text-gray-700 hover:bg-gray-100"
                  )}
                >
                  {tr("context.close", "Close")}
                </button>
              </div>
            </div>

            <div className={classNames("rounded-lg border overflow-auto h-[82vh] p-3", isDark ? "border-slate-700 bg-slate-900/40" : "border-gray-200 bg-gray-50")}>
              <div
                style={{
                  transform: `scale(${mermaidZoom})`,
                  transformOrigin: "top left",
                  width: `${100 / mermaidZoom}%`,
                }}
              >
                <MermaidDiagram chart={mermaidChart} isDark={isDark} fitMode="natural" className="min-h-[76vh]" />
              </div>
            </div>
          </div>
        </div>
      ) : null}

      <ProjectSavedNotifyModal
        isOpen={showNotifyModal}
        onClose={() => setShowNotifyModal(false)}
        onDone={() => {
          void handleNotifyDone();
        }}
        isDark={isDark}
        projectPathLabel={projectPathLabel}
        notifyMessage={notifyMessage}
        notifyAgents={notifyAgents}
        onChangeNotifyAgents={setNotifyAgents}
        notifyBusy={notifyBusy}
        notifyError={notifyError}
      />
    </>
  );
}
