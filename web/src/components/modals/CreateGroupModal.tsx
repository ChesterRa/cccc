import { DirItem, DirSuggestion } from "../../types";
import { BranchInfo } from "../../services/api";
import { TemplatePreviewDetails } from "../TemplatePreviewDetails";

export interface CreateGroupModalProps {
  isOpen: boolean;
  isDark: boolean;
  busy: string;

  dirSuggestions: DirSuggestion[];
  dirItems: DirItem[];
  currentDir: string;
  parentDir: string | null;
  showDirBrowser: boolean;

  createGroupPath: string;
  setCreateGroupPath: (path: string) => void;
  createGroupName: string;
  setCreateGroupName: (name: string) => void;
  createGroupTemplateFile: File | null;
  templatePreview: unknown | null;
  scopeRoot: string;
  promptOverwriteFiles: string[];
  templateError: string;
  templateBusy: boolean;
  onSelectTemplate: (file: File | null) => void;

  // Worktree options
  createWorktree: boolean;
  setCreateWorktree: (v: boolean) => void;
  baseBranch: string;
  setBaseBranch: (v: string) => void;
  branches: BranchInfo[];
  loadingBranches: boolean;

  onFetchDirContents: (path: string) => void;
  onCreateGroup: () => void;
  onClose: () => void;
  onCancelAndReset: () => void;
}

export function CreateGroupModal({
  isOpen,
  isDark,
  busy,
  dirSuggestions,
  dirItems,
  currentDir,
  parentDir,
  showDirBrowser,
  createGroupPath,
  setCreateGroupPath,
  createGroupName,
  setCreateGroupName,
  createGroupTemplateFile,
  templatePreview,
  scopeRoot,
  promptOverwriteFiles,
  templateError,
  templateBusy,
  onSelectTemplate,
  createWorktree,
  setCreateWorktree,
  baseBranch,
  setBaseBranch,
  branches,
  loadingBranches,
  onFetchDirContents,
  onCreateGroup,
  onClose,
  onCancelAndReset,
}: CreateGroupModalProps) {
  if (!isOpen) return null;

  return (
    <div
      className={`fixed inset-0 backdrop-blur-sm flex items-start justify-center p-4 sm:p-6 z-50 animate-fade-in ${isDark ? "bg-black/50" : "bg-black/30"}`}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="create-group-title"
    >
      <div
        className={`w-full max-w-lg mt-8 sm:mt-16 rounded-2xl border shadow-2xl animate-scale-in overflow-hidden flex flex-col max-h-[calc(100vh-4rem)] sm:max-h-[calc(100vh-8rem)] ${
          isDark ? "border-slate-700/50 bg-gradient-to-b from-slate-800 to-slate-900" : "border-gray-200 bg-white"
        }`}
      >
        <div className={`px-6 py-4 border-b ${isDark ? "border-slate-700/50" : "border-gray-200"}`}>
          <div id="create-group-title" className={`text-lg font-semibold ${isDark ? "text-white" : "text-gray-900"}`}>
            Create Working Group
          </div>
          <div className={`text-sm mt-1 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Select a project directory to start collaborating</div>
        </div>
        <div className="p-6 space-y-5 overflow-y-auto min-h-0 flex-1">
          {dirSuggestions.length > 0 && !createGroupPath && (
            <div>
              <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Quick Select</label>
              <div className="grid grid-cols-2 gap-2">
                {dirSuggestions.slice(0, 6).map((s) => (
                  <button
                    key={s.path}
                    className={`flex items-center gap-2 px-3 py-2 rounded-xl border transition-colors text-left min-h-[56px] ${
                      isDark
                        ? "border-slate-600/50 bg-slate-800/50 hover:bg-slate-700/50 hover:border-slate-500"
                        : "border-gray-200 bg-gray-50 hover:bg-gray-100 hover:border-gray-300"
                    }`}
                    onClick={() => {
                      setCreateGroupPath(s.path);
                      setCreateGroupName(s.path.split("/").filter(Boolean).pop() || "");
                      onFetchDirContents(s.path);
                    }}
                  >
                    <span className="text-lg">{s.icon}</span>
                    <div className="min-w-0">
                      <div className={`text-sm font-medium truncate ${isDark ? "text-slate-200" : "text-gray-700"}`}>{s.name}</div>
                      <div className={`text-[10px] truncate ${isDark ? "text-slate-500" : "text-gray-500"}`}>{s.path}</div>
                    </div>
                  </button>
                ))}
              </div>
            </div>
          )}
          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Project Directory</label>
            <div className="flex gap-2">
              <input
                className={`flex-1 rounded-xl border px-4 py-2.5 text-sm font-mono min-h-[44px] transition-colors ${
                  isDark
                    ? "bg-slate-900/80 border-slate-600/50 text-white placeholder-slate-500 focus:border-blue-500"
                    : "bg-white border-gray-300 text-gray-900 placeholder-gray-400 focus:border-blue-500"
                }`}
                value={createGroupPath}
                onChange={(e) => {
                  setCreateGroupPath(e.target.value);
                  const dirName = e.target.value.split("/").filter(Boolean).pop() || "";
                  if (!createGroupName || createGroupName === currentDir.split("/").filter(Boolean).pop()) {
                    setCreateGroupName(dirName);
                  }
                }}
                placeholder="/path/to/your/project"
                autoFocus
              />
              <button
                className={`px-4 py-2 rounded-xl text-sm font-medium transition-colors min-h-[44px] ${
                  isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                }`}
                onClick={() => onFetchDirContents(createGroupPath || "~")}
              >
                Browse
              </button>
            </div>
          </div>
          {showDirBrowser && (
            <div className={`border rounded-xl max-h-48 overflow-auto ${isDark ? "border-slate-600/50 bg-slate-900/50" : "border-gray-200 bg-gray-50"}`}>
              {currentDir && (
                <div
                  className={`px-3 py-1.5 border-b text-xs font-mono truncate ${
                    isDark ? "border-slate-700/30 bg-slate-800/30 text-slate-400" : "border-gray-200 bg-gray-100 text-gray-500"
                  }`}
                >
                  {currentDir}
                </div>
              )}
              {parentDir && (
                <button
                  className={`w-full flex items-center gap-2 px-3 py-2 text-left border-b min-h-[44px] ${
                    isDark ? "hover:bg-slate-800/50 border-slate-700/30" : "hover:bg-gray-100 border-gray-200"
                  }`}
                  onClick={() => {
                    onFetchDirContents(parentDir);
                    setCreateGroupPath(parentDir);
                    setCreateGroupName(parentDir.split("/").filter(Boolean).pop() || "");
                  }}
                >
                  <span className={isDark ? "text-slate-400" : "text-gray-400"}>📁</span>
                  <span className={`text-sm ${isDark ? "text-slate-400" : "text-gray-500"}`}>..</span>
                </button>
              )}
              {dirItems.filter((d) => d.is_dir).length === 0 && (
                <div className={`px-3 py-4 text-center text-sm ${isDark ? "text-slate-500" : "text-gray-500"}`}>No subdirectories</div>
              )}
              {dirItems
                .filter((d) => d.is_dir)
                .map((item) => (
                  <button
                    key={item.path}
                    className={`w-full flex items-center gap-2 px-3 py-2 text-left min-h-[44px] ${isDark ? "hover:bg-slate-800/50" : "hover:bg-gray-100"}`}
                    onClick={() => {
                      setCreateGroupPath(item.path);
                      setCreateGroupName(item.name);
                      onFetchDirContents(item.path);
                    }}
                  >
                    <span className="text-blue-500">📁</span>
                    <span className={`text-sm ${isDark ? "text-slate-200" : "text-gray-700"}`}>{item.name}</span>
                  </button>
                ))}
            </div>
          )}
          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>Group Name</label>
            <input
              className={`w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors ${
                isDark
                  ? "bg-slate-900/80 border-slate-600/50 text-white placeholder-slate-500 focus:border-blue-500"
                  : "bg-white border-gray-300 text-gray-900 placeholder-gray-400 focus:border-blue-500"
              }`}
              value={createGroupName}
              onChange={(e) => setCreateGroupName(e.target.value)}
              placeholder="Auto-filled from directory name"
            />
          </div>

          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>
              Git Worktree (optional)
            </label>
            <div
              className={`rounded-xl border px-4 py-3 ${
                isDark ? "border-slate-600/50 bg-slate-900/50" : "border-gray-200 bg-gray-50"
              }`}
            >
              <label className="flex items-center gap-3 cursor-pointer">
                <input
                  type="checkbox"
                  checked={createWorktree}
                  onChange={(e) => setCreateWorktree(e.target.checked)}
                  className="w-4 h-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
                />
                <span className={`text-sm ${isDark ? "text-slate-200" : "text-gray-700"}`}>
                  Create isolated Git worktree for this group
                </span>
              </label>
              {createWorktree && (
                <div className="mt-3">
                  {loadingBranches ? (
                    <div className={`text-sm ${isDark ? "text-slate-400" : "text-gray-500"}`}>
                      Loading branches...
                    </div>
                  ) : branches.length > 0 ? (
                    <div className="space-y-2">
                      <label className={`block text-xs ${isDark ? "text-slate-400" : "text-gray-500"}`}>
                        Base Branch (new branch will be created from this)
                      </label>
                      <select
                        className={`w-full rounded-lg border px-3 py-2 text-sm min-h-[40px] transition-colors ${
                          isDark
                            ? "bg-slate-800/80 border-slate-600/50 text-white focus:border-blue-500"
                            : "bg-white border-gray-300 text-gray-900 focus:border-blue-500"
                        }`}
                        value={baseBranch}
                        onChange={(e) => setBaseBranch(e.target.value)}
                      >
                        <option value="">Select base branch...</option>
                        {branches.filter(b => !b.in_use || b.is_current).map((b) => (
                          <option key={b.name} value={b.name}>
                            {b.name}{b.is_current ? " (当前)" : b.is_remote ? " (远程)" : ""}
                          </option>
                        ))}
                      </select>
                      {baseBranch && (
                        <div className={`text-xs ${isDark ? "text-blue-400" : "text-blue-600"}`}>
                          A new branch will be auto-created based on "{baseBranch}"
                        </div>
                      )}
                    </div>
                  ) : (
                    <div className={`text-sm ${isDark ? "text-amber-400" : "text-amber-600"}`}>
                      No branches found. Make sure the directory is a Git repository.
                    </div>
                  )}
                </div>
              )}
              <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                Creates a separate working directory to avoid conflicts with other groups.
              </div>
            </div>
          </div>

          <div>
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>
              Create Group From Template (optional)
            </label>
            <div
              className={`rounded-xl border px-4 py-3 ${
                isDark ? "border-slate-600/50 bg-slate-900/50" : "border-gray-200 bg-gray-50"
              }`}
            >
              <div className="flex items-center gap-3">
                <input
                  key={createGroupTemplateFile ? createGroupTemplateFile.name : "none"}
                  type="file"
                  accept=".yaml,.yml,.json"
                  className={`text-sm ${isDark ? "text-slate-300" : "text-gray-700"}`}
                  disabled={templateBusy || busy === "create"}
                  onChange={(e) => {
                    const f = e.target.files && e.target.files.length > 0 ? e.target.files[0] : null;
                    onSelectTemplate(f);
                  }}
                />
                {createGroupTemplateFile && (
                  <button
                    type="button"
                    className={`ml-auto px-3 py-2 rounded-lg text-sm min-h-[40px] transition-colors ${
                      isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-100 text-gray-700 border border-gray-200"
                    }`}
                    disabled={templateBusy || busy === "create"}
                    onClick={() => onSelectTemplate(null)}
                  >
                    Clear
                  </button>
                )}
              </div>
              {templateBusy && (
                <div className={`mt-2 text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>Loading template…</div>
              )}
              {!templateBusy && templateError && (
                <div className={`mt-2 text-xs ${isDark ? "text-rose-300" : "text-red-600"}`}>{templateError}</div>
              )}
              {!templateBusy && createGroupTemplateFile && !!templatePreview && (
                <div className="mt-3">
                  <TemplatePreviewDetails
                    isDark={isDark}
                    template={templatePreview as any}
                    scopeRoot={scopeRoot}
                    promptOverwriteFiles={promptOverwriteFiles}
                    detailsOpenByDefault={true}
                    wrap={false}
                  />
                </div>
              )}
              <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                Applies actors, settings, and prompt files (CCCC_PREAMBLE.md / CCCC_HELP.md / CCCC_STANDUP.md).
              </div>
            </div>
          </div>
        </div>
        <div className={`px-6 py-4 border-t ${isDark ? "border-slate-700/50" : "border-gray-200"}`}>
          <div className="flex gap-3">
            <button
              className="flex-1 rounded-xl bg-blue-600 hover:bg-blue-500 text-white px-4 py-2.5 text-sm font-semibold shadow-lg disabled:opacity-50 transition-all min-h-[44px]"
              onClick={onCreateGroup}
              disabled={
                !createGroupPath.trim() ||
                busy === "create" ||
                templateBusy ||
                (!!createGroupTemplateFile && !templatePreview) ||
                (!!createGroupTemplateFile && !!templateError) ||
                (createWorktree && !baseBranch)
              }
            >
              {busy === "create" ? "Creating..." : createGroupTemplateFile ? "Create Group From Template" : "Create Group"}
            </button>
            <button
              className={`px-4 py-2.5 rounded-xl text-sm font-medium transition-colors min-h-[44px] ${
                isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
              }`}
              onClick={onCancelAndReset}
            >
              Cancel
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
