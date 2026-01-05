import { useEffect, useMemo, useState } from "react";
import { apiJson } from "../services/api";
import { GroupContext, ProjectMdInfo } from "../types";
import { classNames } from "../utils/classNames";

interface ContextModalProps {
  isOpen: boolean;
  onClose: () => void;
  groupId: string;
  context: GroupContext | null;
  onUpdateVision: (vision: string) => Promise<void>;
  onUpdateSketch: (sketch: string) => Promise<void>;
  busy: boolean;
  isDark: boolean;
}

export function ContextModal({
  isOpen,
  onClose,
  groupId,
  context,
  onUpdateVision,
  onUpdateSketch,
  busy,
  isDark,
}: ContextModalProps) {
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

    void (async () => {
      const resp = await apiJson<ProjectMdInfo>(`/api/v1/groups/${encodeURIComponent(groupId)}/project_md`);
      if (cancelled) return;
      if (!resp.ok) {
        setProjectMd(null);
        setProjectError(resp.error?.message || "Failed to load PROJECT.md");
        setProjectBusy(false);
        return;
      }
      setProjectMd(resp.result);
      setProjectBusy(false);
    })();

    return () => {
      cancelled = true;
    };
  }, [groupId, isOpen]);

  const handleEditVision = () => {
    setVisionText(context?.vision || "");
    setEditingVision(true);
  };

  const handleSaveVision = async () => {
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
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4 animate-fade-in">
      {/* Backdrop */}
      <div 
        className={isDark ? "absolute inset-0 bg-black/60" : "absolute inset-0 bg-black/40"} 
        onClick={onClose}
        aria-hidden="true"
      />
      
      {/* Modal */}
      <div 
        className={`relative rounded-xl border shadow-2xl w-full max-w-2xl max-h-[80vh] flex flex-col animate-scale-in ${
          isDark 
            ? "bg-slate-900 border-slate-700" 
            : "bg-white border-gray-200"
        }`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="context-modal-title"
      >
        {/* Header */}
        <div className={`flex items-center justify-between px-5 py-4 border-b ${
          isDark ? "border-slate-800" : "border-gray-200"
        }`}>
          <h2 id="context-modal-title" className={`text-lg font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>
            📋 Project Context
          </h2>
          <button
            onClick={onClose}
            className={`text-xl leading-none min-w-[44px] min-h-[44px] flex items-center justify-center rounded-lg transition-colors ${
              isDark ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800" : "text-gray-400 hover:text-gray-600 hover:bg-gray-100"
            }`}
            aria-label="Close context modal"
          >
            ×
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-5 space-y-6">
          {/* PROJECT.md */}
          <div>
            <div className="flex items-center justify-between mb-2 gap-2">
              <div className="min-w-0">
                <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>PROJECT.md</h3>
                <div className={`text-[11px] truncate ${isDark ? "text-slate-500" : "text-gray-500"}`} title={projectPathLabel}>
                  {projectBusy ? "Loading…" : projectMd?.found ? projectPathLabel : projectMd?.path ? `Missing: ${projectMd.path}` : "Missing"}
                </div>
              </div>
              {!editingProject && (
                <button
                  onClick={handleEditProject}
                  disabled={projectBusy || !groupId}
                  className={`text-xs min-h-[36px] px-2 rounded transition-colors disabled:opacity-50 ${
                    isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-500 hover:text-gray-700"
                  }`}
                >
                  {projectMd?.found ? "✏️ Edit" : "＋ Create"}
                </button>
              )}
            </div>
            {projectError && (
              <div className={`mb-2 text-xs rounded-lg border px-3 py-2 ${
                isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-300 bg-rose-50 text-rose-700"
              }`}>
                {projectError}
              </div>
            )}
            {editingProject ? (
              <div className="space-y-2">
                <textarea
                  value={projectText}
                  onChange={(e) => setProjectText(e.target.value)}
                  className={`w-full h-64 px-3 py-2 border rounded-lg text-sm resize-none font-mono transition-colors ${
                    isDark
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
                    className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors disabled:opacity-50 ${
                      isDark
                        ? "bg-slate-700 hover:bg-slate-600 text-slate-200"
                        : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                    }`}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            ) : (
              <div className={`px-3 py-2 rounded-lg text-sm whitespace-pre-wrap min-h-[60px] max-h-[220px] overflow-auto ${
                isDark ? "bg-slate-800/50 text-slate-300" : "bg-gray-50 text-gray-700"
              }`}>
                {projectMd?.found && projectMd.content ? (
                  String(projectMd.content)
                ) : (
                  <span className={isDark ? "text-slate-500 italic" : "text-gray-400 italic"}>No PROJECT.md found</span>
                )}
              </div>
            )}
          </div>

          {/* Vision */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>Vision</h3>
              {!editingVision && (
                <button
                  onClick={handleEditVision}
                  className={`text-xs min-h-[36px] px-2 rounded transition-colors ${
                    isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-500 hover:text-gray-700"
                  }`}
                >
                  ✏️ Edit
                </button>
              )}
            </div>
            {editingVision ? (
              <div className="space-y-2">
                <textarea
                  value={visionText}
                  onChange={(e) => setVisionText(e.target.value)}
                  className={`w-full h-32 px-3 py-2 border rounded-lg text-sm resize-none transition-colors ${
                    isDark 
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
                    className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors ${
                      isDark 
                        ? "bg-slate-700 hover:bg-slate-600 text-slate-200" 
                        : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                    }`}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            ) : (
              <div className={`px-3 py-2 rounded-lg text-sm whitespace-pre-wrap min-h-[60px] ${
                isDark ? "bg-slate-800/50 text-slate-300" : "bg-gray-50 text-gray-700"
              }`}>
                {context?.vision || <span className={isDark ? "text-slate-500 italic" : "text-gray-400 italic"}>No vision set</span>}
              </div>
            )}
          </div>

          {/* Sketch */}
          <div>
            <div className="flex items-center justify-between mb-2">
              <h3 className={`text-sm font-medium ${isDark ? "text-slate-300" : "text-gray-700"}`}>Sketch</h3>
              {!editingSketch && (
                <button
                  onClick={handleEditSketch}
                  className={`text-xs min-h-[36px] px-2 rounded transition-colors ${
                    isDark ? "text-slate-400 hover:text-slate-200" : "text-gray-500 hover:text-gray-700"
                  }`}
                >
                  ✏️ Edit
                </button>
              )}
            </div>
            {editingSketch ? (
              <div className="space-y-2">
                <textarea
                  value={sketchText}
                  onChange={(e) => setSketchText(e.target.value)}
                  className={`w-full h-32 px-3 py-2 border rounded-lg text-sm resize-none transition-colors ${
                    isDark 
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
                    className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors ${
                      isDark 
                        ? "bg-slate-700 hover:bg-slate-600 text-slate-200" 
                        : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                    }`}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            ) : (
              <div className={`px-3 py-2 rounded-lg text-sm whitespace-pre-wrap min-h-[60px] ${
                isDark ? "bg-slate-800/50 text-slate-300" : "bg-gray-50 text-gray-700"
              }`}>
                {context?.sketch || <span className={isDark ? "text-slate-500 italic" : "text-gray-400 italic"}>No sketch set</span>}
              </div>
            )}
          </div>

          {/* Milestones */}
          <div>
            <h3 className={`text-sm font-medium mb-2 ${isDark ? "text-slate-300" : "text-gray-700"}`}>Milestones</h3>
            {context?.milestones && context.milestones.length > 0 ? (
              <div className="space-y-2">
                {context.milestones.map((m) => (
                  <div key={m.id} className={`px-3 py-2 rounded-lg space-y-1 ${
                    isDark ? "bg-slate-800/50" : "bg-gray-50"
                  }`}>
                    <div className="flex items-start gap-2">
                      <span className={classNames(
                        "text-[11px] px-1.5 py-0.5 rounded flex-shrink-0",
                        isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"
                      )}>
                        {m.id}
                      </span>
                      <span className={`text-sm font-medium min-w-0 truncate ${isDark ? "text-slate-200" : "text-gray-800"}`}>{m.name}</span>
                      <span className={classNames(
                        "text-[11px] px-2 py-0.5 rounded flex-shrink-0 ml-auto",
                        m.status === "done"
                          ? isDark ? "bg-emerald-900/50 text-emerald-300" : "bg-emerald-100 text-emerald-700"
                          : m.status === "active"
                            ? isDark ? "bg-blue-900/50 text-blue-300" : "bg-blue-100 text-blue-700"
                            : isDark ? "bg-slate-700 text-slate-400" : "bg-gray-200 text-gray-600"
                      )}>
                        {m.status || "pending"}
                      </span>
                    </div>
                    {(m.started || m.completed) && (
                      <div className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                        {m.started ? `started ${m.started}` : ""}
                        {m.started && m.completed ? " · " : ""}
                        {m.completed ? `completed ${m.completed}` : ""}
                      </div>
                    )}
                    {m.description && (
                      <div className={`text-xs whitespace-pre-wrap ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                        {m.description}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            ) : (
              <div className={`px-3 py-2 rounded-lg text-sm italic ${
                isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"
              }`}>
                No milestones
              </div>
            )}
          </div>

          {/* Tasks */}
          <div>
            <h3 className={`text-sm font-medium mb-2 ${isDark ? "text-slate-300" : "text-gray-700"}`}>Tasks</h3>
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
                          <div className={`text-xs mt-0.5 whitespace-pre-wrap ${isDark ? "text-slate-400" : "text-gray-600"}`}>
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
                                <div className={classNames("mt-0.5 whitespace-pre-wrap", isDark ? "text-slate-500" : "text-gray-500")}>
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
                  <div className={`px-3 py-2 rounded-lg text-sm italic ${
                    isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"
                  }`}>
                    No active task
                  </div>
                )}
              </div>
            ) : (
              <div className={`px-3 py-2 rounded-lg text-sm italic ${
                isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"
              }`}>
                No task summary
              </div>
            )}
          </div>

          {/* Notes */}
          <div>
            <h3 className={`text-sm font-medium mb-2 ${isDark ? "text-slate-300" : "text-gray-700"}`}>Notes</h3>
            {context?.notes && context.notes.length > 0 ? (
              <div className="space-y-2">
                {context.notes.map((n) => (
                  <div key={n.id} className={`px-3 py-2 rounded-lg ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                    <div className="flex items-center gap-2">
                      <span className={`text-xs px-1.5 py-0.5 rounded ${isDark ? "bg-slate-700 text-slate-300" : "bg-gray-200 text-gray-700"}`}>
                        {n.id}
                      </span>
                      {typeof n.ttl === "number" && (
                        <span className={`text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>ttl {n.ttl}</span>
                      )}
                      {n.expiring ? (
                        <span className={`text-[11px] ${isDark ? "text-amber-300" : "text-amber-600"}`}>expiring</span>
                      ) : null}
                    </div>
                    <div className={`text-xs mt-1 whitespace-pre-wrap ${isDark ? "text-slate-300" : "text-gray-700"}`}>
                      {n.content}
                    </div>
                  </div>
                ))}
              </div>
            ) : (
              <div className={`px-3 py-2 rounded-lg text-sm italic ${
                isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"
              }`}>
                No notes
              </div>
            )}
          </div>

          {/* References */}
          <div>
            <h3 className={`text-sm font-medium mb-2 ${isDark ? "text-slate-300" : "text-gray-700"}`}>References</h3>
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
                    </div>
                    {r.note ? (
                      <div className={`text-xs mt-1 whitespace-pre-wrap ${isDark ? "text-slate-400" : "text-gray-600"}`}>
                        {r.note}
                      </div>
                    ) : null}
                  </div>
                ))}
              </div>
            ) : (
              <div className={`px-3 py-2 rounded-lg text-sm italic ${
                isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"
              }`}>
                No references
              </div>
            )}
          </div>

          {/* Presence */}
          <div>
            <h3 className={`text-sm font-medium mb-2 ${isDark ? "text-slate-300" : "text-gray-700"}`}>Presence</h3>
            {context?.presence?.agents && context.presence.agents.length > 0 ? (
              <div className="space-y-2">
                {context.presence.agents.map((a) => (
                  <div key={a.id} className={`px-3 py-2 rounded-lg ${isDark ? "bg-slate-800/50" : "bg-gray-50"}`}>
                    <div className="flex items-center gap-2">
                      <span className={`text-sm font-medium ${isDark ? "text-slate-200" : "text-gray-800"}`}>{a.id}</span>
                      {a.updated_at ? (
                        <span className={`text-[11px] ml-auto ${isDark ? "text-slate-500" : "text-gray-500"}`}>{a.updated_at}</span>
                      ) : null}
                    </div>
                    {a.status ? (
                      <div className={`text-xs mt-1 whitespace-pre-wrap ${isDark ? "text-slate-300" : "text-gray-700"}`}>
                        {a.status}
                      </div>
                    ) : (
                      <div className={`text-xs mt-1 italic ${isDark ? "text-slate-500" : "text-gray-500"}`}>No status</div>
                    )}
                  </div>
                ))}
              </div>
            ) : (
              <div className={`px-3 py-2 rounded-lg text-sm italic ${
                isDark ? "bg-slate-800/50 text-slate-500" : "bg-gray-50 text-gray-400"
              }`}>
                No presence
              </div>
            )}
          </div>
        </div>
      </div>

      {showNotifyModal && (
        <div className="fixed inset-0 z-[60] flex items-center justify-center p-4 animate-fade-in">
          <div
            className={isDark ? "absolute inset-0 bg-black/70" : "absolute inset-0 bg-black/50"}
            onClick={() => { if (!notifyBusy) setShowNotifyModal(false); }}
            aria-hidden="true"
          />
          <div
            className={`relative w-full max-w-md rounded-xl border shadow-2xl p-4 ${
              isDark ? "bg-slate-900 border-slate-700" : "bg-white border-gray-200"
            }`}
            role="dialog"
            aria-modal="true"
            aria-label="Project updated"
          >
            <div className={`text-sm font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>PROJECT.md saved</div>
            <div className={`text-xs mt-1 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{projectPathLabel}</div>

            {notifyError && (
              <div className={`mt-3 text-xs rounded-lg border px-3 py-2 ${
                isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-300 bg-rose-50 text-rose-700"
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

            <div className={`mt-2 text-[11px] rounded-lg px-3 py-2 whitespace-pre-wrap ${
              isDark ? "bg-slate-800/60 text-slate-300" : "bg-gray-50 text-gray-700"
            }`}>
              {notifyMessage}
            </div>

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
                className={`px-4 py-2 text-sm rounded-lg min-h-[44px] transition-colors disabled:opacity-50 ${
                  isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
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
