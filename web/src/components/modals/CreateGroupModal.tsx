import { useTranslation } from "react-i18next";
import { DirItem, DirSuggestion } from "../../types";
import { TemplatePreviewDetails } from "../TemplatePreviewDetails";
import type { TemplatePreviewDetailsProps } from "../TemplatePreviewDetails";
import { useModalA11y } from "../../hooks/useModalA11y";

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
  templatePreview: TemplatePreviewDetailsProps["template"] | null;
  templateError: string;
  templateBusy: boolean;
  onSelectTemplate: (file: File | null) => void;

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
  templateError,
  templateBusy,
  onSelectTemplate,
  onFetchDirContents,
  onCreateGroup,
  onClose,
  onCancelAndReset,
}: CreateGroupModalProps) {
  const { t } = useTranslation("modals");
  const { modalRef } = useModalA11y(isOpen, onClose);
  if (!isOpen) return null;

  return (
    <div
      className={`fixed inset-0 backdrop-blur-sm flex items-stretch sm:items-start justify-center p-0 sm:p-6 z-50 animate-fade-in ${isDark ? "bg-black/50" : "bg-black/30"}`}
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      role="dialog"
      aria-modal="true"
      aria-labelledby="create-group-title"
    >
      <div
        ref={modalRef}
        className={`w-full h-full sm:h-auto sm:max-w-lg sm:mt-16 border shadow-2xl animate-scale-in overflow-hidden flex flex-col sm:max-h-[calc(100vh-8rem)] rounded-none sm:rounded-2xl ${
          isDark ? "border-slate-700/50 bg-gradient-to-b from-slate-800 to-slate-900" : "border-gray-200 bg-white"
        }`}
      >
        <div className={`px-6 py-4 border-b safe-area-inset-top ${isDark ? "border-slate-700/50" : "border-gray-200"}`}>
          <div id="create-group-title" className={`text-lg font-semibold ${isDark ? "text-white" : "text-gray-900"}`}>
            {t("createGroup.title")}
          </div>
          <div className={`text-sm mt-1 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("createGroup.subtitle")}</div>
        </div>
        <div className="p-6 space-y-5 overflow-y-auto min-h-0 flex-1">
          {dirSuggestions.length > 0 && !createGroupPath && (
            <div>
              <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("createGroup.quickSelect")}</label>
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
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("createGroup.projectDirectory")}</label>
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
                placeholder={t("createGroup.pathPlaceholder")}
                autoFocus
              />
              <button
                className={`px-4 py-2 rounded-xl text-sm font-medium transition-colors min-h-[44px] ${
                  isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
                }`}
                onClick={() => onFetchDirContents(createGroupPath || "~")}
              >
                {t("createGroup.browse")}
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
                <div className={`px-3 py-4 text-center text-sm ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("createGroup.noSubdirectories")}</div>
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
            <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>{t("createGroup.groupName")}</label>
            <input
              className={`w-full rounded-xl border px-4 py-2.5 text-sm min-h-[44px] transition-colors ${
                isDark
                  ? "bg-slate-900/80 border-slate-600/50 text-white placeholder-slate-500 focus:border-blue-500"
                  : "bg-white border-gray-300 text-gray-900 placeholder-gray-400 focus:border-blue-500"
              }`}
              value={createGroupName}
              onChange={(e) => setCreateGroupName(e.target.value)}
              placeholder={t("createGroup.groupNamePlaceholder")}
            />
          </div>

          <div>
              <label className={`block text-xs font-medium mb-2 ${isDark ? "text-slate-400" : "text-gray-500"}`}>
                {t("createGroup.blueprintLabel")}
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
                    {t("common:reset")}
                  </button>
                )}
              </div>
              {templateBusy && (
                <div className={`mt-2 text-xs ${isDark ? "text-slate-500" : "text-gray-500"}`}>{t("createGroup.loadingBlueprint")}</div>
              )}
              {!templateBusy && templateError && (
                <div className={`mt-2 text-xs ${isDark ? "text-rose-300" : "text-red-600"}`}>{templateError}</div>
              )}
              {!templateBusy && createGroupTemplateFile && !!templatePreview && (
                <div className="mt-3">
                  <TemplatePreviewDetails
                    isDark={isDark}
                    template={templatePreview}
                    detailsOpenByDefault={true}
                    wrap={false}
                  />
                </div>
              )}
              <div className={`mt-2 text-[11px] ${isDark ? "text-slate-500" : "text-gray-500"}`}>
                {t("createGroup.blueprintHint")}
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
                (!!createGroupTemplateFile && !!templateError)
              }
            >
              {busy === "create" ? t("createGroup.creating") : createGroupTemplateFile ? t("createGroup.createFromBlueprint") : t("createGroup.createGroup")}
            </button>
            <button
              className={`px-4 py-2.5 rounded-xl text-sm font-medium transition-colors min-h-[44px] ${
                isDark ? "bg-slate-700 hover:bg-slate-600 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
              }`}
              onClick={onCancelAndReset}
            >
              {t("common:cancel")}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
