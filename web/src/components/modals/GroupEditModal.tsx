import { useModalA11y } from "../../hooks/useModalA11y";

export interface GroupEditModalProps {
  isOpen: boolean;
  isDark: boolean;
  busy: string;
  groupId: string;
  ccccHome: string;
  projectRoot: string;
  title: string;
  topic: string;
  onChangeTitle: (title: string) => void;
  onChangeTopic: (topic: string) => void;
  onSave: () => void;
  onCancel: () => void;
  onDelete: () => void;
}

export function GroupEditModal({
  isOpen,
  isDark,
  busy,
  groupId,
  ccccHome,
  projectRoot,
  title,
  topic,
  onChangeTitle,
  onChangeTopic,
  onSave,
  onCancel,
  onDelete,
}: GroupEditModalProps) {
  const { modalRef } = useModalA11y(isOpen, onCancel);
  if (!isOpen) return null;

  const homeRoot = String(ccccHome || "").trim();
  const gid = String(groupId || "").trim();
  const groupDataDir = homeRoot && gid ? `${homeRoot}/groups/${gid}` : "";
  const groupConfigFile = groupDataDir ? `${groupDataDir}/group.yaml` : "";
  const groupLedgerFile = groupDataDir ? `${groupDataDir}/ledger.jsonl` : "";

  async function copyToClipboard(text: string): Promise<boolean> {
    const t = String(text || "").trim();
    if (!t) return false;
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(t);
        return true;
      }
    } catch {
      // ignore
    }
    try {
      window.prompt("Copy to clipboard:", t);
      return true;
    } catch {
      return false;
    }
  }

  return (
    <div
      className={`fixed inset-0 backdrop-blur-sm flex items-stretch sm:items-start justify-center p-0 sm:p-6 z-50 animate-fade-in ${isDark ? "bg-black/50" : "bg-black/30"}`}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="group-edit-title"
    >
      <div
        ref={modalRef}
        className={`w-full h-full sm:h-auto sm:max-w-md sm:mt-16 border shadow-2xl animate-scale-in flex flex-col rounded-none sm:rounded-2xl ${
          isDark ? "border-slate-700/50 bg-gradient-to-b from-slate-800 to-slate-900" : "border-gray-200 bg-white"
        }`}
      >
        <div className={`px-6 py-4 border-b safe-area-inset-top ${isDark ? "border-slate-700/50" : "border-gray-200"}`}>
          <div id="group-edit-title" className={`text-lg font-semibold ${isDark ? "text-white" : "text-gray-900"}`}>
            Edit Group
          </div>
        </div>
        <div className="p-6 space-y-4 flex-1 overflow-y-auto">
          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Name</label>
            <input
              className={`w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors ${
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
              }`}
              value={title}
              onChange={(e) => onChangeTitle(e.target.value)}
              placeholder="Group name"
            />
          </div>
          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Description (optional)</label>
            <input
              className={`w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors ${
                isDark ? "bg-slate-900/80 border-slate-600/50 text-white focus:border-blue-500" : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
              }`}
              value={topic}
              onChange={(e) => onChangeTopic(e.target.value)}
              placeholder="What is this group working on?"
            />
          </div>
          <div className={`rounded-xl border p-4 ${isDark ? "border-slate-700/50 bg-slate-900/30" : "border-gray-200 bg-gray-50"}`}>
            <div className="space-y-2">
              <div className="flex items-center gap-2">
                <div className={`text-xs ${isDark ? "text-slate-300" : "text-gray-700"}`}>Group ID</div>
                <div className={`flex-1 min-w-0 font-mono text-xs truncate ${isDark ? "text-white" : "text-gray-900"}`}>
                  {groupId || "—"}
                </div>
                <button
                  className={`px-2 py-1 rounded-lg text-xs border transition-colors ${isDark ? "border-slate-600/50 bg-slate-800/50 text-slate-200 hover:bg-slate-700/60" : "border-gray-200 bg-white text-gray-700 hover:bg-gray-100"}`}
                  onClick={async () => {
                    const ok = await copyToClipboard(groupId);
                    if (!ok) return;
                  }}
                  disabled={!groupId}
                  title="Copy group_id"
                  type="button"
                >
                  Copy
                </button>
              </div>
              <div className="flex items-center gap-2">
                <div className={`text-xs ${isDark ? "text-slate-300" : "text-gray-700"}`}>Project Root</div>
                <div className={`flex-1 min-w-0 font-mono text-xs truncate ${isDark ? "text-white" : "text-gray-900"}`}>
                  {projectRoot || "— (no scope attached)"}
                </div>
                <button
                  className={`px-2 py-1 rounded-lg text-xs border transition-colors ${isDark ? "border-slate-600/50 bg-slate-800/50 text-slate-200 hover:bg-slate-700/60" : "border-gray-200 bg-white text-gray-700 hover:bg-gray-100"}`}
                  onClick={async () => {
                    const ok = await copyToClipboard(projectRoot);
                    if (!ok) return;
                  }}
                  disabled={!projectRoot}
                  title="Copy project root path"
                  type="button"
                >
                  Copy
                </button>
              </div>
              <div className="flex items-center gap-2">
                <div className={`text-xs ${isDark ? "text-slate-300" : "text-gray-700"}`}>Group Data Directory</div>
                <div className={`flex-1 min-w-0 font-mono text-xs truncate ${isDark ? "text-white" : "text-gray-900"}`}>
                  {groupDataDir || "—"}
                </div>
                <button
                  className={`px-2 py-1 rounded-lg text-xs border transition-colors ${isDark ? "border-slate-600/50 bg-slate-800/50 text-slate-200 hover:bg-slate-700/60" : "border-gray-200 bg-white text-gray-700 hover:bg-gray-100"}`}
                  onClick={async () => {
                    const ok = await copyToClipboard(groupDataDir);
                    if (!ok) return;
                  }}
                  disabled={!groupDataDir}
                  title="Copy group data directory"
                  type="button"
                >
                  Copy
                </button>
              </div>
              <div className="flex items-center gap-2">
                <div className={`text-xs ${isDark ? "text-slate-300" : "text-gray-700"}`}>Group Config File</div>
                <div className={`flex-1 min-w-0 font-mono text-xs truncate ${isDark ? "text-white" : "text-gray-900"}`}>
                  {groupConfigFile || "—"}
                </div>
                <button
                  className={`px-2 py-1 rounded-lg text-xs border transition-colors ${isDark ? "border-slate-600/50 bg-slate-800/50 text-slate-200 hover:bg-slate-700/60" : "border-gray-200 bg-white text-gray-700 hover:bg-gray-100"}`}
                  onClick={async () => {
                    const ok = await copyToClipboard(groupConfigFile);
                    if (!ok) return;
                  }}
                  disabled={!groupConfigFile}
                  title="Copy group config file path"
                  type="button"
                >
                  Copy
                </button>
              </div>
              <div className="flex items-center gap-2">
                <div className={`text-xs ${isDark ? "text-slate-300" : "text-gray-700"}`}>Group Ledger File</div>
                <div className={`flex-1 min-w-0 font-mono text-xs truncate ${isDark ? "text-white" : "text-gray-900"}`}>
                  {groupLedgerFile || "—"}
                </div>
                <button
                  className={`px-2 py-1 rounded-lg text-xs border transition-colors ${isDark ? "border-slate-600/50 bg-slate-800/50 text-slate-200 hover:bg-slate-700/60" : "border-gray-200 bg-white text-gray-700 hover:bg-gray-100"}`}
                  onClick={async () => {
                    const ok = await copyToClipboard(groupLedgerFile);
                    if (!ok) return;
                  }}
                  disabled={!groupLedgerFile}
                  title="Copy group ledger file path"
                  type="button"
                >
                  Copy
                </button>
              </div>
            </div>
          </div>
          <div className="flex gap-3 pt-3 flex-wrap">
            <button
              className="flex-1 rounded-xl bg-blue-600 hover:bg-blue-500 text-white px-4 py-2.5 text-sm font-semibold shadow-lg disabled:opacity-50 transition-all min-h-[44px]"
              onClick={onSave}
              disabled={!title.trim() || busy === "group-update"}
            >
              Save
            </button>
            <button
              className={`px-4 py-2.5 rounded-xl text-sm font-medium transition-colors min-h-[44px] ${
                isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
              }`}
              onClick={onCancel}
            >
              Cancel
            </button>
            <button
              className={`px-4 py-2.5 rounded-xl border text-sm font-medium disabled:opacity-50 transition-colors min-h-[44px] ${
                isDark ? "bg-rose-500/20 border-rose-500/30 text-rose-400 hover:bg-rose-500/30" : "bg-rose-50 border-rose-200 text-rose-600 hover:bg-rose-100"
              }`}
              onClick={() => {
                onCancel();
                onDelete();
              }}
              disabled={busy === "group-delete"}
              title="Delete this group permanently"
            >
              Delete
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
