import { useEffect, useMemo, useState } from "react";
import { apiJson, contextSync, fetchTasks } from "../services/api";
import { GroupContext, ProjectMdInfo, Task } from "../types";
import { formatFullTime, formatTime } from "../utils/time";
import { classNames } from "../utils/classNames";
import { MarkdownRenderer } from "./MarkdownRenderer";
import { useModalA11y } from "../hooks/useModalA11y";

interface ContextModalProps {
  isOpen: boolean;
  onClose: () => void;
  groupId: string;
  context: GroupContext | null;
  onRefreshContext: () => Promise<void>;
  onUpdateVision: (vision: string) => Promise<void>;
  onUpdateSketch: (sketch: string) => Promise<void>;
  busy: boolean;
  isDark: boolean;
}

type ContextOp = { op: string } & Record<string, unknown>;

export function ContextModal({
  isOpen,
  onClose,
  groupId,
  context,
  onRefreshContext,
  onUpdateVision,
  onUpdateSketch,
  busy,
  isDark,
}: ContextModalProps) {
  const { modalRef } = useModalA11y(isOpen, onClose);
  const [editingVision, setEditingVision] = useState(false);
  const [editingSketch, setEditingSketch] = useState(false);
  const [visionText, setVisionText] = useState("");
  const [sketchText, setSketchText] = useState("");

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

  const [syncBusy, setSyncBusy] = useState(false);
  const [syncError, setSyncError] = useState("");

  const [addingNote, setAddingNote] = useState(false);
  const [newNoteContent, setNewNoteContent] = useState("");
  const [editingNoteId, setEditingNoteId] = useState<string | null>(null);
  const [editNoteContent, setEditNoteContent] = useState("");

  const [addingRef, setAddingRef] = useState(false);
  const [newRefUrl, setNewRefUrl] = useState("");
  const [newRefNote, setNewRefNote] = useState("");
  const [editingRefId, setEditingRefId] = useState<string | null>(null);
  const [editRefUrl, setEditRefUrl] = useState("");
  const [editRefNote, setEditRefNote] = useState("");

  const projectPathLabel = useMemo(() => {
    const p = projectMd?.path ? String(projectMd.path) : "";
    if (p) return p;
    return "PROJECT.md";
  }, [projectMd?.path]);

  const notifyMessage = useMemo(() => {
    return `PROJECT.md updated. Please re-read and align. (${projectPathLabel})`;
  }, [projectPathLabel]);

  useEffect(() => {
    if (!isOpen) return;
    if (!groupId) return;
    let cancelled = false;

    setProjectBusy(true);
    setProjectError("");
    setEditingProject(false);
    setNotifyError("");
    setShowNotifyModal(false);

    setTasksBusy(true);
    setTasksError("");

    setSyncBusy(false);
    setSyncError("");
    setAddingNote(false);
    setEditingNoteId(null);
    setAddingRef(false);
    setEditingRefId(null);

    void (async () => {
      const resp = await apiJson<ProjectMdInfo>(`/api/v1/groups/${encodeURIComponent(groupId)}/project_md`);
      if (cancelled) return;
      if (!resp.ok) {
        setProjectMd(null);
        setProjectError(resp.error?.message || "Failed to load PROJECT.md");
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
        setTasksError(resp.error?.message || "Failed to load tasks");
        setTasksBusy(false);
        return;
      }
      setTasks(Array.isArray(resp.result?.tasks) ? resp.result.tasks : []);
      setTasksBusy(false);
    })();

    return () => {
      cancelled = true;
    };
  }, [groupId, isOpen]);

  const tasksByStatus = useMemo(() => {
    const list = Array.isArray(tasks) ? tasks : [];
    const normalize = (s: unknown) => String(s || "planned").toLowerCase();
    const active: Task[] = [];
    const planned: Task[] = [];
    const done: Task[] = [];
    const archived: Task[] = [];
    const other: Task[] = [];
    for (const t of list) {
      const st = normalize(t.status);
      if (st === "active") active.push(t);
      else if (st === "done") done.push(t);
      else if (st === "archived") archived.push(t);
      else if (st === "planned") planned.push(t);
      else other.push(t);
    }
    return { active, planned, done, archived, other };
  }, [tasks]);

  const milestonesByStatus = useMemo(() => {
    const list = Array.isArray(context?.milestones) ? context.milestones! : [];
    const normalize = (s: unknown) => String(s || "planned").toLowerCase();
    const active: typeof list = [];
    const planned: typeof list = [];
    const done: typeof list = [];
    const archived: typeof list = [];
    const other: typeof list = [];
    for (const m of list) {
      const st = normalize(m.status);
      if (st === "active") active.push(m);
      else if (st === "done") done.push(m);
      else if (st === "archived") archived.push(m);
      else if (st === "planned") planned.push(m);
      else other.push(m);
    }
    return { active, planned, done, archived, other };
  }, [context?.milestones]);

  const scrollToSection = (id: string) => {
    const el = document.getElementById(id);
    if (!el) return;
    el.scrollIntoView({ block: "start", behavior: "smooth" });
  };

  const runOps = async (ops: ContextOp[]): Promise<boolean> => {
    if (!groupId) return false;
    setSyncBusy(true);
    setSyncError("");
    try {
      const resp = await contextSync(groupId, ops);
      if (!resp.ok) {
        setSyncError(resp.error?.message || "Failed to apply changes");
        return false;
      }
      await onRefreshContext();

      const needsTaskRefresh = ops.some((o) => String(o.op || "").startsWith("task."));
      if (needsTaskRefresh) {
        const tResp = await fetchTasks(groupId);
        if (tResp.ok) {
          setTasks(Array.isArray(tResp.result?.tasks) ? tResp.result.tasks : []);
        } else {
          setTasksError(tResp.error?.message || "Failed to load tasks");
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
    setTasks((prev) => (Array.isArray(prev) ? prev.map((t) => (t.id === taskId ? { ...t, status: "archived" } : t)) : prev));
    const ok = await runOps([{ op: "task.update", task_id: taskId, status: "archived" }]);
    if (!ok) {
      // Rollback on failure
      setTasks(prevTasks);
    }
  };

  const handleRestoreTask = async (taskId: string) => {
    await runOps([{ op: "task.restore", task_id: taskId }]);
  };

  const handleArchiveMilestone = async (milestoneId: string) => {
    await runOps([{ op: "milestone.update", milestone_id: milestoneId, status: "archived" }]);
  };

  const handleRestoreMilestone = async (milestoneId: string) => {
    await runOps([{ op: "milestone.restore", milestone_id: milestoneId }]);
  };

  const handleStartEditNote = (noteId: string) => {
    const n = (context?.notes || []).find((x) => x.id === noteId);
    if (!n) return;
    setEditingNoteId(noteId);
    setEditNoteContent(String(n.content || ""));
  };

  const handleSaveEditNote = async () => {
    if (!editingNoteId) return;
    const ok = await runOps([
      { op: "note.update", note_id: editingNoteId, content: editNoteContent },
    ]);
    if (ok) {
      setEditingNoteId(null);
    }
  };

  const handleRemoveNote = async (noteId: string) => {
    const okConfirm = window.confirm(`Delete note ${noteId}?`);
    if (!okConfirm) return;
    await runOps([{ op: "note.remove", note_id: noteId }]);
  };

  const handleAddNote = async () => {
    const ok = await runOps([{ op: "note.add", content: newNoteContent }]);
    if (ok) {
      setAddingNote(false);
      setNewNoteContent("");
    }
  };

  const handleStartEditRef = (refId: string) => {
    const r = (context?.references || []).find((x) => x.id === refId);
    if (!r) return;
    setEditingRefId(refId);
    setEditRefUrl(String(r.url || ""));
    setEditRefNote(String(r.note || ""));
  };

  const handleSaveEditRef = async () => {
    if (!editingRefId) return;
    const ok = await runOps([
      {
        op: "reference.update",
        reference_id: editingRefId,
        url: editRefUrl,
        note: editRefNote,
      },
    ]);
    if (ok) {
      setEditingRefId(null);
    }
  };

  const handleRemoveRef = async (refId: string) => {
    const okConfirm = window.confirm(`Delete reference ${refId}?`);
    if (!okConfirm) return;
    await runOps([{ op: "reference.remove", reference_id: refId }]);
  };

  const handleAddRef = async () => {
    const ok = await runOps([{ op: "reference.add", url: newRefUrl, note: newRefNote }]);
    if (ok) {
      setAddingRef(false);
      setNewRefUrl("");
      setNewRefNote("");
    }
  };

  const handleEditVision = () => {
    setVisionText(context?.vision || "");
    setEditingVision(true);
  };

  const handleSaveVision = async () => {
    // onUpdateVision returns Promise<void>, so we wrap it to handle errors if we want to stay in edit mode
    // But onUpdateVision is passed from parent. Let's assume it throws or we can't detect error easily 
    // unless we change the prop signature. 
    // However, the prop implementation in AppModals calls api.updateVision.
    // Let's rely on the fact that if it fails, it usually shows an error toast.
    // For now, let's keep simple:
    await onUpdateVision(visionText);
    setEditingVision(false);
  };

  const handleEditSketch = () => {
    setSketchText(context?.sketch || "");
    setEditingSketch(true);
  };

  const handleSaveSketch = async () => {
    await onUpdateSketch(sketchText);
    setEditingSketch(false);
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
        setProjectError(resp.error?.message || "Failed to save PROJECT.md");
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
          setNotifyError(resp.error?.message || "Failed to notify agents");
          return;
        }
      }
      setShowNotifyModal(false);
    } finally {
      setNotifyBusy(false);
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-stretch sm:items-center justify-center p-0 sm:p-4 animate-fade-in">
      {/* Backdrop */}
      <div
        className={isDark ? "absolute inset-0 bg-black/60" : "absolute inset-0 bg-black/40"}
        onPointerDown={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
        aria-hidden="true"
      />

      {/* Modal */}
      <div
        className={`relative w-full h-full sm:h-auto sm:max-h-[80vh] sm:max-w-2xl flex flex-col border shadow-2xl animate-scale-in rounded-none sm:rounded-xl ${isDark
          ? "bg-slate-900 border-slate-700"
          : "bg-white border-gray-200"
          }`}
        ref={modalRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="context-modal-title"
      >
        {/* Header */}
        <div className={`flex items-center justify-between px-5 py-4 border-b safe-area-inset-top ${isDark ? "border-slate-800" : "border-gray-200"
          }`}>
          <h2 id="context-modal-title" className={`text-lg font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>
            📋 Project Context
          </h2>
          <button
            onClick={onClose}
            className={`text-xl leading-none min-w-[44px] min-h-[44px] flex items-center justify-center rounded-lg transition-colors ${isDark ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800" : "text-gray-400 hover:text-gray-600 hover:bg-gray-100"
              }`}
            aria-label="Close context modal"
          >
            ×
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-5 space-y-6">
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className={classNames(
                "px-2.5 py-1.5 rounded-xl text-xs transition-all glass-btn",
                isDark ? "text-slate-200" : "text-gray-800"
              )}
              onClick={() => scrollToSection("context-project")}
            >
              PROJECT
            </button>
            <button
              type="button"
              className={classNames(
                "px-2.5 py-1.5 rounded-xl text-xs transition-all glass-btn",
                isDark ? "text-slate-200" : "text-gray-800"
              )}
              onClick={() => scrollToSection("context-vision")}
            >
              Vision
            </button>
            <button
              type="button"
              className={classNames(
                "px-2.5 py-1.5 rounded-xl text-xs transition-all glass-btn",
                isDark ? "text-slate-200" : "text-gray-800"
              )}
              onClick={() => scrollToSection("context-sketch")}
            >
              Sketch
            </button>
            <button
              type="button"
              className={classNames(
                "px-2.5 py-1.5 rounded-xl text-xs transition-all glass-btn",
                isDark ? "text-slate-200" : "text-gray-800"
              )}
              onClick={() => scrollToSection("context-tasks")}
            >
              Tasks
            </button>
            <button
              type="button"
              className={classNames(
                "px-2.5 py-1.5 rounded-xl text-xs transition-all glass-btn",
                isDark ? "text-slate-200" : "text-gray-800"
              )}
              onClick={() => scrollToSection("context-notes")}
            >
              Notes
            </button>
            <button
              type="button"
              className={classNames(
                "px-2.5 py-1.5 rounded-xl text-xs transition-all glass-btn",
                isDark ? "text-slate-200" : "text-gray-800"
              )}
              onClick={() => scrollToSection("context-references")}
            >
              References
            </button>
          </div>

          <details id="context-presence" open>
            <summary className={classNames("cursor-pointer select-none text-sm font-medium", isDark ? "text-slate-300" : "text-gray-700")}>
              Presence
            </summary>
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
                            title={formatFullTime(a.updated_at)}
                          >
                            Updated {formatTime(a.updated_at)}
                          </span>
                        ) : null}
                      </div>
                      {a.status ? (
                        <MarkdownRenderer
                          content={String(a.status)}
                          isDark={isDark}
                          className={classNames("text-xs mt-1", isDark ? "text-slate-300" : "text-gray-700")}
                        />
                      ) : (
                        <div className={`text-xs mt-1 italic ${isDark ? "text-slate-500" : "text-gray-500"}`}>No presence yet</div>
                      )}
                    </div>
                  ))}
                </div>
              ) : (
                <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                  No presence
                </div>
              )}
            </div>
          </details>

          {/* PROJECT.md */}
          <details id="context-project" open>
            <summary className={classNames("cursor-pointer select-none text-sm font-medium", isDark ? "text-slate-300" : "text-gray-700")}>
              PROJECT.md
            </summary>
            <div className="mt-2">
              <div className="flex items-center justify-between mb-2 gap-2">
                <div className="min-w-0">
                  <div className={`text-[11px] truncate ${isDark ? "text-slate-500" : "text-gray-500"}`} title={projectPathLabel}>
                    {projectBusy ? "Loading…" : projectMd?.found ? projectPathLabel : projectMd?.path ? `Missing: ${projectMd.path}` : "Missing"}
                  </div>
                </div>
                {!editingProject && (
                  <button
                    onClick={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      handleEditProject();
                    }}
                    disabled={projectBusy || !groupId}
                    className={`text-xs min-h-[36px] px-2 rounded transition-colors disabled:opacity-50 ${isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-500 hover:text-gray-700"
                      }`}
                  >
                    {projectMd?.found ? "✏️ Edit" : "＋ Create"}
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
                    className={`w-full h-64 px-3 py-2 border rounded-lg text-sm resize-none font-mono transition-colors ${isDark
                      ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500"
                      : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                      }`}
                    placeholder="Write your project constitution here…"
                  />
                  <div className="flex gap-2">
                    <button
                      onClick={handleSaveProject}
                      disabled={projectBusy || !groupId}
                      className="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm rounded-lg disabled:opacity-50 min-h-[44px] transition-colors"
                    >
                      {projectBusy ? "Saving..." : "Save"}
                    </button>
                    <button
                      onClick={() => setEditingProject(false)}
                      disabled={projectBusy}
                      className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors disabled:opacity-50 ${isDark
                        ? "bg-slate-700 hover:bg-slate-600 text-slate-200"
                        : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                        }`}
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              ) : (
                <div className={`px-3 py-2 rounded-lg text-sm min-h-[60px] max-h-[220px] overflow-auto ${isDark ? "bg-slate-800/50 text-slate-300" : "bg-gray-50 text-gray-700"
                  }`}>
                  {projectMd?.found && projectMd.content ? (
                    <MarkdownRenderer
                      content={String(projectMd.content)}
                      isDark={isDark}
                    />
                  ) : (
                    <span className={isDark ? "text-slate-500 italic" : "text-gray-400 italic"}>No PROJECT.md found</span>
                  )}
                </div>
              )}
            </div>
          </details>

          {/* Vision */}
          <details id="context-vision" open>
            <summary className={classNames("cursor-pointer select-none text-sm font-medium", isDark ? "text-slate-300" : "text-gray-700")}>
              Vision
            </summary>
            <div className="mt-2">
              {!editingVision && (
                <div className="flex justify-end mb-2">
                  <button
                    onClick={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      handleEditVision();
                    }}
                    className={`text-xs min-h-[36px] px-2 rounded transition-colors ${isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-500 hover:text-gray-700"
                      }`}
                  >
                    ✏️ Edit
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
                    placeholder="Describe the project vision..."
                  />
                  <div className="flex gap-2">
                    <button
                      onClick={handleSaveVision}
                      disabled={busy}
                      className="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm rounded-lg disabled:opacity-50 min-h-[44px] transition-colors"
                    >
                      {busy ? "Saving..." : "Save"}
                    </button>
                    <button
                      onClick={() => setEditingVision(false)}
                      className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors ${isDark
                        ? "bg-slate-700 hover:bg-slate-600 text-slate-200"
                        : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                        }`}
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              ) : (
                <div className={`px-3 py-2 rounded-lg text-sm min-h-[60px] ${isDark ? "bg-slate-800/50 text-slate-300" : "bg-gray-50 text-gray-700"
                  }`}>
                  {context?.vision ? (
                    <MarkdownRenderer content={context.vision} isDark={isDark} />
                  ) : (
                    <span className={isDark ? "text-slate-500 italic" : "text-gray-400 italic"}>No vision set</span>
                  )}
                </div>
              )}
            </div>
          </details>

          {/* Sketch */}
          <details id="context-sketch" open>
            <summary className={classNames("cursor-pointer select-none text-sm font-medium", isDark ? "text-slate-300" : "text-gray-700")}>
              Sketch
            </summary>
            <div className="mt-2">
              {!editingSketch && (
                <div className="flex justify-end mb-2">
                  <button
                    onClick={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      handleEditSketch();
                    }}
                    className={`text-xs min-h-[36px] px-2 rounded transition-colors ${isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-500 hover:text-gray-700"
                      }`}
                  >
                    ✏️ Edit
                  </button>
                </div>
              )}
              {editingSketch ? (
                <div className="space-y-2">
                  <textarea
                    value={sketchText}
                    onChange={(e) => setSketchText(e.target.value)}
                    className={`w-full h-32 px-3 py-2 border rounded-lg text-sm resize-none transition-colors ${isDark
                      ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500"
                      : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                      }`}
                    placeholder="Technical sketch or architecture notes..."
                  />
                  <div className="flex gap-2">
                    <button
                      onClick={handleSaveSketch}
                      disabled={busy}
                      className="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white text-sm rounded-lg disabled:opacity-50 min-h-[44px] transition-colors"
                    >
                      {busy ? "Saving..." : "Save"}
                    </button>
                    <button
                      onClick={() => setEditingSketch(false)}
                      className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors ${isDark
                        ? "bg-slate-700 hover:bg-slate-600 text-slate-200"
                        : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                        }`}
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              ) : (
                <div className={`px-3 py-2 rounded-lg text-sm min-h-[60px] ${isDark ? "bg-slate-800/50 text-slate-300" : "bg-gray-50 text-gray-700"
                  }`}>
                  {context?.sketch ? (
                    <MarkdownRenderer content={context.sketch} isDark={isDark} />
                  ) : (
                    <span className={isDark ? "text-slate-500 italic" : "text-gray-400 italic"}>No sketch set</span>
                  )}
                </div>
              )}
            </div>
          </details>

          <details id="context-milestones">
            <summary className={classNames("cursor-pointer select-none text-sm font-medium", isDark ? "text-slate-300" : "text-gray-700")}>
              Milestones
            </summary>
            <div className="mt-2">
              {(context?.milestones && context.milestones.length > 0) ? (
                <div className="space-y-2">
                  <details open>
                    <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                      active ({milestonesByStatus.active.length})
                    </summary>
                    <div className="mt-2 space-y-2">
                      {milestonesByStatus.active.length === 0 ? (
                        <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                          No active milestones
                        </div>
                      ) : (
                        milestonesByStatus.active.map((m) => (
                          <div key={m.id} className={`px-3 py-2 rounded-lg space-y-1 ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                            <div className="flex items-start gap-2">
                              <span className={classNames(
                                "text-[11px] px-1.5 py-0.5 rounded flex-shrink-0",
                                isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"
                              )}>
                                {m.id}
                              </span>
                              <span className={`text-sm font-medium min-w-0 truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>{m.name}</span>
                              <button
                                type="button"
                                disabled={syncBusy}
                                onClick={() => void handleArchiveMilestone(m.id)}
                                className={classNames(
                                  "text-[11px] px-2 py-0.5 rounded flex-shrink-0 ml-auto transition-colors disabled:opacity-50",
                                  isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
                                )}
                              >
                                Archive
                              </button>
                            </div>
                            {(m.started || m.completed) && (
                              <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                {m.started ? `started ${m.started}` : ""}
                                {m.started && m.completed ? " · " : ""}
                                {m.completed ? `completed ${m.completed}` : ""}
                              </div>
                            )}
                            {m.description && (
                              <MarkdownRenderer
                                content={String(m.description)}
                                isDark={isDark}
                                className={classNames("text-xs", isDark ? "text-slate-400" : "text-gray-600")}
                              />
                            )}
                          </div>
                        ))
                      )}
                    </div>
                  </details>

                  <details open>
                    <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                      planned ({milestonesByStatus.planned.length})
                    </summary>
                    <div className="mt-2 space-y-2">
                      {milestonesByStatus.planned.length === 0 ? (
                        <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                          No planned milestones
                        </div>
                      ) : (
                        milestonesByStatus.planned.map((m) => (
                          <div key={m.id} className={`px-3 py-2 rounded-lg space-y-1 ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                            <div className="flex items-start gap-2">
                              <span className={classNames(
                                "text-[11px] px-1.5 py-0.5 rounded flex-shrink-0",
                                isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"
                              )}>
                                {m.id}
                              </span>
                              <span className={`text-sm font-medium min-w-0 truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>{m.name}</span>
                              <button
                                type="button"
                                disabled={syncBusy}
                                onClick={() => void handleArchiveMilestone(m.id)}
                                className={classNames(
                                  "text-[11px] px-2 py-0.5 rounded flex-shrink-0 ml-auto transition-colors disabled:opacity-50",
                                  isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
                                )}
                              >
                                Archive
                              </button>
                            </div>
                            {(m.started || m.completed) && (
                              <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                {m.started ? `started ${m.started}` : ""}
                                {m.started && m.completed ? " · " : ""}
                                {m.completed ? `completed ${m.completed}` : ""}
                              </div>
                            )}
                            {m.description && (
                              <MarkdownRenderer
                                content={String(m.description)}
                                isDark={isDark}
                                className={classNames("text-xs", isDark ? "text-slate-400" : "text-gray-600")}
                              />
                            )}
                            {m.outcomes && (
                              <div className={classNames("text-xs", isDark ? "text-slate-400" : "text-gray-600")}>
                                <span className="font-medium">outcomes: </span>
                                <MarkdownRenderer
                                  content={String(m.outcomes)}
                                  isDark={isDark}
                                />
                              </div>
                            )}
                          </div>
                        ))
                      )}
                    </div>
                  </details>

                  <details>
                    <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                      done ({milestonesByStatus.done.length})
                    </summary>
                    <div className="mt-2 space-y-2">
                      {milestonesByStatus.done.length === 0 ? (
                        <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                          No done milestones
                        </div>
                      ) : (
                        milestonesByStatus.done.map((m) => (
                          <div key={m.id} className={`px-3 py-2 rounded-lg space-y-1 ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                            <div className="flex items-start gap-2">
                              <span className={classNames(
                                "text-[11px] px-1.5 py-0.5 rounded flex-shrink-0",
                                isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"
                              )}>
                                {m.id}
                              </span>
                              <span className={`text-sm font-medium min-w-0 truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>{m.name}</span>
                              <button
                                type="button"
                                disabled={syncBusy}
                                onClick={() => void handleArchiveMilestone(m.id)}
                                className={classNames(
                                  "text-[11px] px-2 py-0.5 rounded flex-shrink-0 ml-auto transition-colors disabled:opacity-50",
                                  isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
                                )}
                              >
                                Archive
                              </button>
                            </div>
                            {(m.started || m.completed) && (
                              <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                {m.started ? `started ${m.started}` : ""}
                                {m.started && m.completed ? " · " : ""}
                                {m.completed ? `completed ${m.completed}` : ""}
                              </div>
                            )}
                            {m.description && (
                              <MarkdownRenderer
                                content={String(m.description)}
                                isDark={isDark}
                                className={classNames("text-xs", isDark ? "text-slate-400" : "text-gray-600")}
                              />
                            )}
                            {m.outcomes && (
                              <div className={classNames("text-xs", isDark ? "text-slate-400" : "text-gray-600")}>
                                <span className="font-medium">outcomes: </span>
                                <MarkdownRenderer
                                  content={String(m.outcomes)}
                                  isDark={isDark}
                                />
                              </div>
                            )}
                          </div>
                        ))
                      )}
                    </div>
                  </details>

                  <details>
                    <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                      archived ({milestonesByStatus.archived.length})
                    </summary>
                    <div className="mt-2 space-y-2">
                      {milestonesByStatus.archived.length === 0 ? (
                        <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                          No archived milestones
                        </div>
                      ) : (
                        milestonesByStatus.archived.map((m) => (
                          <div key={m.id} className={`px-3 py-2 rounded-lg space-y-1 ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                            <div className="flex items-start gap-2">
                              <span className={classNames(
                                "text-[11px] px-1.5 py-0.5 rounded flex-shrink-0",
                                isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"
                              )}>
                                {m.id}
                              </span>
                              <span className={`text-sm font-medium min-w-0 truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>{m.name}</span>
                              <button
                                type="button"
                                disabled={syncBusy}
                                onClick={() => void handleRestoreMilestone(m.id)}
                                className={classNames(
                                  "text-[11px] px-2 py-0.5 rounded flex-shrink-0 ml-auto transition-colors disabled:opacity-50",
                                  isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
                                )}
                              >
                                Restore
                              </button>
                            </div>
                            {(m.started || m.completed) && (
                              <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                {m.started ? `started ${m.started}` : ""}
                                {m.started && m.completed ? " · " : ""}
                                {m.completed ? `completed ${m.completed}` : ""}
                              </div>
                            )}
                            {m.description && (
                              <MarkdownRenderer
                                content={String(m.description)}
                                isDark={isDark}
                                className={classNames("text-xs", isDark ? "text-slate-400" : "text-gray-600")}
                              />
                            )}
                            {m.outcomes && (
                              <div className={classNames("text-xs", isDark ? "text-slate-400" : "text-gray-600")}>
                                <span className="font-medium">outcomes: </span>
                                <MarkdownRenderer
                                  content={String(m.outcomes)}
                                  isDark={isDark}
                                />
                              </div>
                            )}
                          </div>
                        ))
                      )}
                    </div>
                  </details>
                </div>
              ) : (
                <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                  No milestones
                </div>
              )}
            </div>
          </details>

          <details id="context-tasks" open>
            <summary className={classNames("cursor-pointer select-none text-sm font-medium", isDark ? "text-slate-300" : "text-gray-700")}>
              Tasks
            </summary>
            <div className="mt-2">
              {context?.tasks_summary ? (
                <div className="space-y-2">
                  <div className={`px-3 py-2 rounded-lg text-xs ${isDark ? "bg-slate-800/50 text-slate-300" : "bg-gray-50 text-gray-700"}`}>
                    total {context.tasks_summary.total} · active {context.tasks_summary.active} · planned {context.tasks_summary.planned} · done {context.tasks_summary.done}
                  </div>

                  {context.active_task ? (
                    <div className={`px-3 py-2 rounded-lg ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                      <div className="flex items-start gap-2">
                        <span className={`text-[11px] px-1.5 py-0.5 rounded ${isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"}`}>
                          {context.active_task.id}
                        </span>
                        <div className="min-w-0 flex-1">
                          <div className={`text-sm font-medium truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>
                            {context.active_task.name}
                          </div>
                          {context.active_task.goal && (
                            <div className={`text-xs mt-0.5 ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                              {context.active_task.goal}
                            </div>
                          )}
                        </div>
                        <span className={classNames(
                          "text-[11px] px-2 py-0.5 rounded flex-shrink-0",
                          context.active_task.status === "done"
                            ? isDark ? "bg-emerald-900/50 text-emerald-300" : "bg-emerald-100 text-emerald-700"
                            : context.active_task.status === "active"
                              ? isDark ? "bg-blue-900/50 text-blue-300" : "bg-blue-100 text-blue-700"
                              : isDark ? "bg-slate-700 text-slate-400" : "bg-gray-200 text-gray-600"
                        )}>
                          {context.active_task.status || "planned"}
                        </span>
                      </div>
                      {(context.active_task.assignee || context.active_task.milestone) && (
                        <div className={`text-[11px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                          {context.active_task.milestone ? `milestone ${context.active_task.milestone}` : ""}
                          {context.active_task.milestone && context.active_task.assignee ? " · " : ""}
                          {context.active_task.assignee ? `assignee ${context.active_task.assignee}` : ""}
                        </div>
                      )}
                      {Array.isArray(context.active_task.steps) && context.active_task.steps.length > 0 && (
                        <div className="mt-2 space-y-1">
                          {context.active_task.steps.map((s) => (
                            <div key={s.id} className="flex items-start gap-2">
                              <span className={`text-[11px] px-1.5 py-0.5 rounded ${isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"}`}>
                                {s.id}
                              </span>
                              <div className={`text-xs flex-1 ${isDark ? "text-slate-300" : "text-gray-700"}`}>
                                {s.name}
                                {s.acceptance ? (
                                  <div className={classNames("mt-0.5", isDark ? "text-slate-500" : "text-gray-500")}>
                                    {s.acceptance}
                                  </div>
                                ) : null}
                              </div>
                              <span className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>{s.status || ""}</span>
                            </div>
                          ))}
                        </div>
                      )}
                    </div>
                  ) : (
                    <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"
                      }`}>
                      No active task
                    </div>
                  )}

                  {tasksError ? (
                    <div className={`px-3 py-2 rounded-lg text-sm ${isDark ? "bg-rose-500/10 text-rose-300 border border-rose-500/30" : "bg-rose-50 text-rose-700 border border-rose-300"}`}>
                      {tasksError}
                    </div>
                  ) : tasksBusy ? (
                    <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                      Loading tasks…
                    </div>
                  ) : (
                    <div className="space-y-2">
                      {(tasksByStatus.active.length + tasksByStatus.planned.length + tasksByStatus.done.length + tasksByStatus.archived.length + tasksByStatus.other.length) === 0 ? (
                        <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                          No tasks
                        </div>
                      ) : (
                        <>
                          <details open>
                            <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                              active ({tasksByStatus.active.length})
                            </summary>
                            <div className="mt-2 space-y-2">
                              {tasksByStatus.active.length === 0 ? (
                                <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                                  No active tasks
                                </div>
                              ) : tasksByStatus.active.map((t) => (
                                <div key={t.id} className={`px-3 py-2 rounded-lg ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                                  <div className="flex items-start gap-2">
                                    <span className={`text-[11px] px-1.5 py-0.5 rounded ${isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"}`}>
                                      {t.id}
                                    </span>
                                    <div className="min-w-0 flex-1">
                                      <div className={`text-sm font-medium truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t.name}</div>
                                      {t.goal ? (
                                        <MarkdownRenderer
                                          content={String(t.goal)}
                                          isDark={isDark}
                                          className={classNames("text-xs mt-0.5", isDark ? "text-slate-400" : "text-gray-600")}
                                        />
                                      ) : null}
                                      {(t.milestone || t.assignee) ? (
                                        <div className={`text-[11px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                          {t.milestone ? `milestone ${t.milestone}` : ""}
                                          {t.milestone && t.assignee ? " · " : ""}
                                          {t.assignee ? `assignee ${t.assignee}` : ""}
                                        </div>
                                      ) : null}
                                    </div>
                                    <button
                                      type="button"
                                      disabled={syncBusy}
                                      onClick={() => void handleArchiveTask(t.id)}
                                      className={classNames(
                                        "text-[11px] px-2 py-0.5 rounded flex-shrink-0 transition-colors disabled:opacity-50",
                                        isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
                                      )}
                                    >
                                      Archive
                                    </button>
                                  </div>
                                </div>
                              ))}
                            </div>
                          </details>

                          <details open>
                            <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                              planned ({tasksByStatus.planned.length})
                            </summary>
                            <div className="mt-2 space-y-2">
                              {tasksByStatus.planned.length === 0 ? (
                                <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                                  No planned tasks
                                </div>
                              ) : tasksByStatus.planned.map((t) => (
                                <div key={t.id} className={`px-3 py-2 rounded-lg ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                                  <div className="flex items-start gap-2">
                                    <span className={`text-[11px] px-1.5 py-0.5 rounded ${isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"}`}>
                                      {t.id}
                                    </span>
                                    <div className="min-w-0 flex-1">
                                      <div className={`text-sm font-medium truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t.name}</div>
                                      {t.goal ? (
                                        <MarkdownRenderer
                                          content={String(t.goal)}
                                          isDark={isDark}
                                          className={classNames("text-xs mt-0.5", isDark ? "text-slate-400" : "text-gray-600")}
                                        />
                                      ) : null}
                                      {(t.milestone || t.assignee) ? (
                                        <div className={`text-[11px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                          {t.milestone ? `milestone ${t.milestone}` : ""}
                                          {t.milestone && t.assignee ? " · " : ""}
                                          {t.assignee ? `assignee ${t.assignee}` : ""}
                                        </div>
                                      ) : null}
                                    </div>
                                    <button
                                      type="button"
                                      disabled={syncBusy}
                                      onClick={() => void handleArchiveTask(t.id)}
                                      className={classNames(
                                        "text-[11px] px-2 py-0.5 rounded flex-shrink-0 transition-colors disabled:opacity-50",
                                        isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
                                      )}
                                    >
                                      Archive
                                    </button>
                                  </div>
                                </div>
                              ))}
                            </div>
                          </details>

                          <details>
                            <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                              done ({tasksByStatus.done.length})
                            </summary>
                            <div className="mt-2 space-y-2">
                              {tasksByStatus.done.length === 0 ? (
                                <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                                  No done tasks
                                </div>
                              ) : tasksByStatus.done.map((t) => (
                                <div key={t.id} className={`px-3 py-2 rounded-lg ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                                  <div className="flex items-start gap-2">
                                    <span className={`text-[11px] px-1.5 py-0.5 rounded ${isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"}`}>
                                      {t.id}
                                    </span>
                                    <div className="min-w-0 flex-1">
                                      <div className={`text-sm font-medium truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t.name}</div>
                                      {t.goal ? (
                                        <MarkdownRenderer
                                          content={String(t.goal)}
                                          isDark={isDark}
                                          className={classNames("text-xs mt-0.5", isDark ? "text-slate-400" : "text-gray-600")}
                                        />
                                      ) : null}
                                      {(t.milestone || t.assignee) ? (
                                        <div className={`text-[11px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                          {t.milestone ? `milestone ${t.milestone}` : ""}
                                          {t.milestone && t.assignee ? " · " : ""}
                                          {t.assignee ? `assignee ${t.assignee}` : ""}
                                        </div>
                                      ) : null}
                                    </div>
                                    <button
                                      type="button"
                                      disabled={syncBusy}
                                      onClick={() => void handleArchiveTask(t.id)}
                                      className={classNames(
                                        "text-[11px] px-2 py-0.5 rounded flex-shrink-0 transition-colors disabled:opacity-50",
                                        isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
                                      )}
                                    >
                                      Archive
                                    </button>
                                  </div>
                                </div>
                              ))}
                            </div>
                          </details>

                          <details>
                            <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                              archived ({tasksByStatus.archived.length})
                            </summary>
                            <div className="mt-2 space-y-2">
                              {tasksByStatus.archived.length === 0 ? (
                                <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                                  No archived tasks
                                </div>
                              ) : tasksByStatus.archived.map((t) => (
                                <div key={t.id} className={`px-3 py-2 rounded-lg ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                                  <div className="flex items-start gap-2">
                                    <span className={`text-[11px] px-1.5 py-0.5 rounded ${isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"}`}>
                                      {t.id}
                                    </span>
                                    <div className="min-w-0 flex-1">
                                      <div className={`text-sm font-medium truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t.name}</div>
                                      {t.goal ? (
                                        <MarkdownRenderer
                                          content={String(t.goal)}
                                          isDark={isDark}
                                          className={classNames("text-xs mt-0.5", isDark ? "text-slate-400" : "text-gray-600")}
                                        />
                                      ) : null}
                                      {(t.milestone || t.assignee) ? (
                                        <div className={`text-[11px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                          {t.milestone ? `milestone ${t.milestone}` : ""}
                                          {t.milestone && t.assignee ? " · " : ""}
                                          {t.assignee ? `assignee ${t.assignee}` : ""}
                                        </div>
                                      ) : null}
                                    </div>
                                    <button
                                      type="button"
                                      disabled={syncBusy}
                                      onClick={() => void handleRestoreTask(t.id)}
                                      className={classNames(
                                        "text-[11px] px-2 py-0.5 rounded flex-shrink-0 transition-colors disabled:opacity-50",
                                        isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
                                      )}
                                    >
                                      Restore
                                    </button>
                                  </div>
                                </div>
                              ))}
                            </div>
                          </details>

                          <details>
                            <summary className={classNames("cursor-pointer select-none text-xs", isDark ? "text-slate-500" : "text-gray-500")}>
                              other ({tasksByStatus.other.length})
                            </summary>
                            <div className="mt-2 space-y-2">
                              {tasksByStatus.other.length === 0 ? (
                                <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"}`}>
                                  No other tasks
                                </div>
                              ) : tasksByStatus.other.map((t) => (
                                <div key={t.id} className={`px-3 py-2 rounded-lg ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                                  <div className="flex items-start gap-2">
                                    <span className={`text-[11px] px-1.5 py-0.5 rounded ${isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"}`}>
                                      {t.id}
                                    </span>
                                    <div className="min-w-0 flex-1">
                                      <div className={`text-sm font-medium truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>{t.name}</div>
                                      {t.goal ? (
                                        <MarkdownRenderer
                                          content={String(t.goal)}
                                          isDark={isDark}
                                          className={classNames("text-xs mt-0.5", isDark ? "text-slate-400" : "text-gray-600")}
                                        />
                                      ) : null}
                                      {(t.milestone || t.assignee) ? (
                                        <div className={`text-[11px] mt-1 ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                                          {t.milestone ? `milestone ${t.milestone}` : ""}
                                          {t.milestone && t.assignee ? " · " : ""}
                                          {t.assignee ? `assignee ${t.assignee}` : ""}
                                        </div>
                                      ) : null}
                                    </div>
                                    <button
                                      type="button"
                                      disabled={syncBusy}
                                      onClick={() => void handleArchiveTask(t.id)}
                                      className={classNames(
                                        "text-[11px] px-2 py-0.5 rounded flex-shrink-0 transition-colors disabled:opacity-50",
                                        isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
                                      )}
                                    >
                                      Archive
                                    </button>
                                  </div>
                                </div>
                              ))}
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
                  No task summary
                </div>
              )}
            </div>
          </details>

          <details id="context-notes" open>
            <summary className={classNames("cursor-pointer select-none text-sm font-medium", isDark ? "text-slate-300" : "text-gray-700")}>
              Notes
            </summary>
            <div className="mt-2">
              {syncError && (
                <div className={`mb-2 text-xs rounded-lg border px-3 py-2 ${isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-300 bg-rose-50 text-rose-700"}`}>
                  {syncError}
                </div>
              )}

              {!addingNote ? (
                <div className="flex justify-end mb-2">
                  <button
                    type="button"
                    disabled={syncBusy}
                    onClick={() => {
                      setAddingNote(true);
                      setNewNoteContent("");
                    }}
                    className={classNames(
                      "text-xs min-h-[36px] px-2 rounded transition-colors disabled:opacity-50",
                      isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-500 hover:text-gray-700"
                    )}
                  >
                    ＋ Add
                  </button>
                </div>
              ) : (
                <div className="space-y-2 mb-3">
                  <textarea
                    value={newNoteContent}
                    onChange={(e) => setNewNoteContent(e.target.value)}
                    className={`w-full h-28 px-3 py-2 border rounded-lg text-sm resize-none transition-colors ${isDark ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                      }`}
                    placeholder="Note…"
                  />
                  <div className="flex justify-end gap-2">
                    <button
                      type="button"
                      disabled={syncBusy || !newNoteContent.trim()}
                      onClick={() => void handleAddNote()}
                      className="px-3 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white text-xs rounded-lg disabled:opacity-50 min-h-[36px] transition-colors"
                    >
                      {syncBusy ? "Saving…" : "Save"}
                    </button>
                    <button
                      type="button"
                      disabled={syncBusy}
                      onClick={() => setAddingNote(false)}
                      className={classNames(
                        "px-3 py-1.5 text-xs rounded-lg min-h-[36px] transition-colors disabled:opacity-50",
                        isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                      )}
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              )}

              {context?.notes && context.notes.length > 0 ? (
                <div className="space-y-2">
                  {context.notes.map((n) => (
                    <div key={n.id} className={`px-3 py-2 rounded-lg ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                      <div className="flex items-center gap-2">
                        <span className={`text-xs px-1.5 py-0.5 rounded ${isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"}`}>
                          {n.id}
                        </span>
                        <div className="ml-auto flex items-center gap-2">
                          {editingNoteId !== n.id && (
                            <>
                              <button
                                type="button"
                                disabled={syncBusy}
                                onClick={() => handleStartEditNote(n.id)}
                                className={classNames(
                                  "text-[11px] px-2 py-0.5 rounded transition-colors disabled:opacity-50",
                                  isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
                                )}
                              >
                                Edit
                              </button>
                              <button
                                type="button"
                                disabled={syncBusy}
                                onClick={() => void handleRemoveNote(n.id)}
                                className={classNames(
                                  "text-[11px] px-2 py-0.5 rounded transition-colors disabled:opacity-50",
                                  isDark ? "bg-rose-900/30 text-rose-300 hover:bg-rose-900/40" : "bg-rose-100 text-rose-700 hover:bg-rose-200"
                                )}
                              >
                                Delete
                              </button>
                            </>
                          )}
                        </div>
                      </div>

                      {editingNoteId === n.id ? (
                        <div className="mt-2 space-y-2">
                          <textarea
                            value={editNoteContent}
                            onChange={(e) => setEditNoteContent(e.target.value)}
                            className={`w-full h-24 px-3 py-2 border rounded-lg text-sm resize-none transition-colors ${isDark ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                              }`}
                          />
                          <div className="flex justify-end gap-2">
                            <button
                              type="button"
                              disabled={syncBusy}
                              onClick={() => void handleSaveEditNote()}
                              className="px-3 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white text-xs rounded-lg disabled:opacity-50 min-h-[36px] transition-colors"
                            >
                              {syncBusy ? "Saving…" : "Save"}
                            </button>
                            <button
                              type="button"
                              disabled={syncBusy}
                              onClick={() => setEditingNoteId(null)}
                              className={classNames(
                                "px-3 py-1.5 text-xs rounded-lg min-h-[36px] transition-colors disabled:opacity-50",
                                isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                              )}
                            >
                              Cancel
                            </button>
                          </div>
                        </div>
                      ) : (
                        <MarkdownRenderer
                          content={String(n.content)}
                          isDark={isDark}
                          className={classNames("text-xs mt-1", isDark ? "text-slate-300" : "text-gray-700")}
                        />
                      )}
                    </div>
                  ))}
                </div>
              ) : (
                <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"
                  }`}>
                  No notes
                </div>
              )}
            </div>
          </details>

          <details id="context-references" open>
            <summary className={classNames("cursor-pointer select-none text-sm font-medium", isDark ? "text-slate-300" : "text-gray-700")}>
              References
            </summary>
            <div className="mt-2">
              {syncError && (
                <div className={`mb-2 text-xs rounded-lg border px-3 py-2 ${isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-300 bg-rose-50 text-rose-700"}`}>
                  {syncError}
                </div>
              )}

              {!addingRef ? (
                <div className="flex justify-end mb-2">
                  <button
                    type="button"
                    disabled={syncBusy}
                    onClick={() => {
                      setAddingRef(true);
                      setNewRefUrl("");
                      setNewRefNote("");
                    }}
                    className={classNames(
                      "text-xs min-h-[36px] px-2 rounded transition-colors disabled:opacity-50",
                      isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-500 hover:text-gray-700"
                    )}
                  >
                    ＋ Add
                  </button>
                </div>
              ) : (
                <div className="space-y-2 mb-3">
                  <input
                    value={newRefUrl}
                    onChange={(e) => setNewRefUrl(e.target.value)}
                    className={`w-full px-3 py-2 border rounded-lg text-sm transition-colors ${isDark ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                      }`}
                    placeholder="https://…"
                  />
                  <textarea
                    value={newRefNote}
                    onChange={(e) => setNewRefNote(e.target.value)}
                    className={`w-full h-24 px-3 py-2 border rounded-lg text-sm resize-none transition-colors ${isDark ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                      }`}
                    placeholder="Note (optional)…"
                  />
                  <div className="flex justify-end gap-2">
                    <button
                      type="button"
                      disabled={syncBusy || !newRefUrl.trim()}
                      onClick={() => void handleAddRef()}
                      className="px-3 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white text-xs rounded-lg disabled:opacity-50 min-h-[36px] transition-colors"
                    >
                      {syncBusy ? "Saving…" : "Save"}
                    </button>
                    <button
                      type="button"
                      disabled={syncBusy}
                      onClick={() => setAddingRef(false)}
                      className={classNames(
                        "px-3 py-1.5 text-xs rounded-lg min-h-[36px] transition-colors disabled:opacity-50",
                        isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                      )}
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              )}

              {context?.references && context.references.length > 0 ? (
                <div className="space-y-2">
                  {context.references.map((r) => (
                    <div key={r.id} className={`px-3 py-2 rounded-lg ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                      <div className="flex items-center gap-2">
                        <span className={`text-xs px-1.5 py-0.5 rounded ${isDark ? "bg-slate-700 text-slate-400" : "bg-gray-200 text-gray-600"}`}>
                          {r.id}
                        </span>
                        <a
                          href={r.url}
                          target="_blank"
                          rel="noopener noreferrer"
                          className={`text-sm truncate hover:underline ${isDark ? "text-blue-400" : "text-blue-600"}`}
                          title={r.url}
                        >
                          {r.url}
                        </a>
                        <div className="ml-auto flex items-center gap-2">
                          {editingRefId !== r.id && (
                            <>
                              <button
                                type="button"
                                disabled={syncBusy}
                                onClick={() => handleStartEditRef(r.id)}
                                className={classNames(
                                  "text-[11px] px-2 py-0.5 rounded transition-colors disabled:opacity-50",
                                  isDark ? "bg-slate-700 text-slate-300 hover:bg-slate-600" : "bg-gray-200 text-gray-700 hover:bg-gray-300"
                                )}
                              >
                                Edit
                              </button>
                              <button
                                type="button"
                                disabled={syncBusy}
                                onClick={() => void handleRemoveRef(r.id)}
                                className={classNames(
                                  "text-[11px] px-2 py-0.5 rounded transition-colors disabled:opacity-50",
                                  isDark ? "bg-rose-900/30 text-rose-300 hover:bg-rose-900/40" : "bg-rose-100 text-rose-700 hover:bg-rose-200"
                                )}
                              >
                                Delete
                              </button>
                            </>
                          )}
                        </div>
                      </div>

                      {editingRefId === r.id ? (
                        <div className="mt-2 space-y-2">
                          <input
                            value={editRefUrl}
                            onChange={(e) => setEditRefUrl(e.target.value)}
                            className={`w-full px-3 py-2 border rounded-lg text-sm transition-colors ${isDark ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                              }`}
                          />
                          <textarea
                            value={editRefNote}
                            onChange={(e) => setEditRefNote(e.target.value)}
                            className={`w-full h-20 px-3 py-2 border rounded-lg text-sm resize-none transition-colors ${isDark ? "bg-slate-800 border-slate-700 text-slate-200 focus:border-slate-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                              }`}
                          />
                          <div className="flex justify-end gap-2">
                            <button
                              type="button"
                              disabled={syncBusy || !editRefUrl.trim()}
                              onClick={() => void handleSaveEditRef()}
                              className="px-3 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white text-xs rounded-lg disabled:opacity-50 min-h-[36px] transition-colors"
                            >
                              {syncBusy ? "Saving…" : "Save"}
                            </button>
                            <button
                              type="button"
                              disabled={syncBusy}
                              onClick={() => setEditingRefId(null)}
                              className={classNames(
                                "px-3 py-1.5 text-xs rounded-lg min-h-[36px] transition-colors disabled:opacity-50",
                                isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                              )}
                            >
                              Cancel
                            </button>
                          </div>
                        </div>
                      ) : r.note ? (
                        <MarkdownRenderer
                          content={String(r.note)}
                          isDark={isDark}
                          className={classNames("text-xs mt-1", isDark ? "text-slate-400" : "text-gray-600")}
                        />
                      ) : null}
                    </div>
                  ))}
                </div>
              ) : (
                <div className={`px-3 py-2 rounded-lg text-sm italic ${isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"
                  }`}>
                  No references
                </div>
              )}
            </div>
          </details>

          {syncBusy && (
            <div className={classNames(
              "text-[11px] italic",
              isDark ? "text-slate-500" : "text-gray-500"
            )}>
              Applying changes…
            </div>
          )}
        </div>
      </div>

      {showNotifyModal && (
        <div className="fixed inset-0 z-overlay flex items-center justify-center p-4 animate-fade-in">
          <div
            className={isDark ? "absolute inset-0 bg-black/70" : "absolute inset-0 bg-black/50"}
            onPointerDown={(e) => {
              if (e.target !== e.currentTarget) return;
              if (!notifyBusy) setShowNotifyModal(false);
            }}
            aria-hidden="true"
          />
          <div
            className={`relative w-full max-w-md rounded-xl border shadow-2xl p-4 ${isDark ? "bg-slate-900 border-slate-700" : "bg-white border-gray-200"
              }`}
            role="dialog"
            aria-modal="true"
            aria-label="Project updated"
          >
            <div className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>PROJECT.md saved</div>
            <div className={`text-xs mt-1 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{projectPathLabel}</div>

            {notifyError && (
              <div className={`mt-3 text-xs rounded-lg border px-3 py-2 ${isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-300 bg-rose-50 text-rose-700"
                }`}>
                {notifyError}
              </div>
            )}

            <label className={`mt-3 flex items-center gap-2 text-sm ${isDark ? "text-slate-200" : "text-gray-800"}`}>
              <input
                type="checkbox"
                checked={notifyAgents}
                onChange={(e) => setNotifyAgents(e.target.checked)}
                disabled={notifyBusy}
              />
              Notify agents in chat (@all)
            </label>

            <MarkdownRenderer
              content={notifyMessage}
              isDark={isDark}
              className={classNames("mt-2 text-[11px] rounded-lg px-3 py-2", isDark ? "bg-slate-800/60 text-slate-300" : "bg-gray-50 text-gray-700")}
            />

            <div className="mt-3 flex gap-2">
              <button
                onClick={handleNotifyDone}
                disabled={notifyBusy}
                className="flex-1 px-4 py-2 bg-blue-600 hover:bg-blue-500 text-white text-sm rounded-lg disabled:opacity-50 min-h-[44px] transition-colors"
              >
                {notifyBusy ? "Working..." : "Done"}
              </button>
              <button
                onClick={() => setShowNotifyModal(false)}
                disabled={notifyBusy}
                className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors disabled:opacity-50 ${isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                  }`}
              >
                Close
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
