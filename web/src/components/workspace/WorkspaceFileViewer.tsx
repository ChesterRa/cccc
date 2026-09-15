import { useTranslation } from "react-i18next";
import { Paperclip, RotateCcw, Save, X } from "lucide-react";
import { classNames } from "../../utils/classNames";
import type { WorkspaceFile } from "../../types";
import { baseName } from "./workspaceTreeModel";
import { applyEditorText, editorText } from "./workspaceText";

type Props = {
  file: WorkspaceFile;
  draft: string;
  setDraft: (content: string) => void;
  isDark: boolean;
  readOnly: boolean;
  saving: boolean;
  error: string;
  conflict: boolean;
  onClose: () => void;
  onSave: (content: string) => Promise<boolean>;
  onReload: () => void;
  onAttach: () => void;
};

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function WorkspaceFileViewer({
  file,
  draft,
  setDraft,
  isDark,
  readOnly,
  saving,
  error,
  conflict,
  onClose,
  onSave,
  onReload,
  onAttach,
}: Props) {
  const { t } = useTranslation("chat");
  const dirty = draft !== file.content;
  const editable = !readOnly && !file.binary && !file.truncated;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div
        className={classNames(
          "flex items-center gap-1 border-b px-2 py-1.5",
          isDark ? "border-white/8" : "border-black/8",
        )}
      >
        <span className="min-w-0 flex-1 truncate text-[13px] font-medium" title={file.path}>
          {baseName(file.path)}
          {dirty ? <span className="ml-1 opacity-60">•</span> : null}
          <span className="ml-2 truncate text-[11px] font-normal opacity-45">{file.path}</span>
        </span>
        <span className="shrink-0 text-[11px] opacity-50">{formatBytes(file.bytes)}</span>
        <button
          type="button"
          onClick={onAttach}
          title={t("workspaceAttachContext", { defaultValue: "Attach as context" })}
          className={classNames(
            "rounded-md p-1 transition-colors",
            isDark ? "hover:bg-white/8" : "hover:bg-black/5",
          )}
        >
          <Paperclip className="h-3.5 w-3.5" />
        </button>
        {editable ? (
          <button
            type="button"
            disabled={!dirty || saving}
            onClick={() => void onSave(draft)}
            title={t("workspaceSave", { defaultValue: "Save" })}
            className={classNames(
              "rounded-md p-1 transition-colors",
              !dirty || saving
                ? "opacity-30"
                : isDark
                  ? "text-emerald-300 hover:bg-white/8"
                  : "text-emerald-600 hover:bg-black/5",
            )}
          >
            <Save className="h-3.5 w-3.5" />
          </button>
        ) : null}
        <button
          type="button"
          onClick={onClose}
          title={t("workspaceCloseFile", { defaultValue: "Close file" })}
          className={classNames(
            "rounded-md p-1 transition-colors",
            isDark ? "hover:bg-white/8" : "hover:bg-black/5",
          )}
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </div>

      {error ? (
        <div
          className={classNames(
            "flex items-center gap-2 px-3 py-2 text-[12px]",
            isDark ? "bg-rose-400/10 text-rose-200" : "bg-rose-500/8 text-rose-700",
          )}
        >
          <span className="min-w-0 flex-1">
            {conflict
              ? t("workspaceConflict", {
                  defaultValue:
                    "This file changed on disk since you opened it. Reload to see the latest version.",
                })
              : error}
          </span>
          {conflict ? (
            <button
              type="button"
              onClick={onReload}
              className="flex shrink-0 items-center gap-1 rounded-md px-1.5 py-0.5 underline-offset-2 hover:underline"
            >
              <RotateCcw className="h-3 w-3" />
              {t("workspaceReload", { defaultValue: "Reload" })}
            </button>
          ) : null}
        </div>
      ) : null}

      {file.truncated ? (
        <div className="px-3 py-6 text-center text-[12px] opacity-60">
          {t("workspaceTooLarge", {
            defaultValue: "This file is too large to open here ({{size}}).",
            size: formatBytes(file.bytes),
          })}
        </div>
      ) : file.binary ? (
        <div className="px-3 py-6 text-center text-[12px] opacity-60">
          {t("workspaceBinary", { defaultValue: "Binary file — no text preview." })}
        </div>
      ) : editable ? (
        <textarea
          value={editorText(draft)}
          spellCheck={false}
          onChange={(event) => setDraft(applyEditorText(draft, event.target.value, file.content))}
          onKeyDown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key === "s") {
              event.preventDefault();
              if (dirty && !saving) void onSave(draft);
            }
          }}
          className={classNames(
            "min-h-0 flex-1 resize-none bg-transparent px-3 py-2 font-mono text-[12px] leading-relaxed outline-none",
            isDark ? "text-slate-200" : "text-slate-800",
          )}
        />
      ) : (
        <pre
          className={classNames(
            // Wraps rather than scrolling sideways: on a phone a horizontal drag per line is
            // unreadable, and the editable path (a textarea) already soft-wraps.
            "min-h-0 flex-1 overflow-auto px-3 py-2 font-mono text-[12px] leading-relaxed",
            "whitespace-pre-wrap break-words",
            isDark ? "text-slate-200" : "text-slate-800",
          )}
        >
          {file.content}
        </pre>
      )}
    </div>
  );
}
