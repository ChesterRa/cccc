import { buttonVariants } from "../ui/button-variants";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Code, Download, Eye, ExternalLink, Paperclip, RotateCcw, Save, X } from "lucide-react";
import { classNames } from "../../utils/classNames";
import type { WorkspaceFile } from "../../types";
import { baseName } from "./workspaceTreeModel";
import { applyEditorText, editorText } from "./workspaceText";
import { workspaceContentUrl } from "../../services/api/workspace";
import { GraphicViewer } from "../viewer/GraphicViewer";
import { WorkspaceMediaPreview } from "./WorkspaceMediaPreview";
import { WorkspaceDocumentPreview } from "./WorkspaceDocumentPreview";
import { workspacePreviewKind } from "./workspacePreview";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "../ui/dialog";

import type { WorkspaceFileNavigation, WorkspaceOpenFileOptions } from "./useWorkspaceEditor";

type Props = {
  groupId: string;
  file: WorkspaceFile;
  draft: string;
  setDraft: (content: string) => void;
  isDark: boolean;
  readOnly: boolean;
  saving: boolean;
  loading?: boolean;
  reloadVersion?: number;
  error: string;
  conflict: boolean;
  onClose: () => void;
  onSave: (content: string) => Promise<boolean>;
  onReload: () => void;
  onAttach: () => void;
  onOpenFile?: (path: string, options?: WorkspaceOpenFileOptions) => void;
  navigation?: WorkspaceFileNavigation | null;
};

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function WorkspaceFileViewer(props: Props) {
  const { groupId, file } = props;
  return (
    <FileViewer
      key={JSON.stringify([groupId, file.scope_key, file.scope_url, file.path])}
      {...props}
    />
  );
}

function FileViewer({
  groupId,
  file,
  draft,
  setDraft,
  isDark,
  readOnly,
  saving,
  loading = false,
  reloadVersion = 0,
  error,
  conflict,
  onClose,
  onSave,
  onReload,
  onAttach,
  onOpenFile,
  navigation,
}: Props) {
  const { t } = useTranslation("chat");
  const [showSource, setShowSource] = useState(false);
  const [confirmReload, setConfirmReload] = useState(false);
  const cancelReload = useRef<HTMLButtonElement>(null);
  const reloadButton = useRef<HTMLButtonElement>(null);
  const [nativeFragment, setNativeFragment] = useState(navigation?.fragment || "");
  useEffect(() => {
    if (navigation) {
      setShowSource(false);
      setNativeFragment(navigation.fragment);
    }
  }, [navigation]);
  const kind = workspacePreviewKind(file);
  const svgSource = file.mime_type === "image/svg+xml" && !file.binary && !file.truncated;
  const hasSourcePreview = svgSource || ["markdown", "csv", "tsv", "html"].includes(kind);
  const preview = kind !== "text" && !showSource;
  const dirty = draft !== file.content;
  const editable = !readOnly && !preview && !file.binary && !file.truncated;
  const contentUrl =
    workspaceContentUrl(groupId, file) +
    (reloadVersion ? `&reload=${reloadVersion}` : "") +
    (kind === "pdf" ? nativeFragment : "");
  const requestReload = () => {
    if (dirty) setConfirmReload(true);
    else onReload();
  };
  // Render SVG drafts through an image, never inject workspace markup into the App DOM.
  const imageUrl = useMemo(
    () => (svgSource && dirty ? `data:image/svg+xml,${encodeURIComponent(draft)}` : contentUrl),
    [svgSource, dirty, draft, contentUrl],
  );

  const fileActionClass = `${buttonVariants({ variant: "ghost", size: "iconSm" })} max-sm:h-11 max-sm:w-11`;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div
        className={classNames(
          "flex shrink-0 flex-wrap items-center gap-1 border-b px-2 py-1.5",
          "border-[var(--glass-border-subtle)]",
        )}
      >
        <span className="min-w-0 flex-1 basis-24 truncate text-sm font-medium" title={file.path}>
          {baseName(file.path)}
          {dirty ? <span className="ml-1 opacity-60">•</span> : null}
          <span className="ml-2 truncate text-xs font-normal text-[var(--color-text-tertiary)]">
            {file.path}
          </span>
        </span>
        <span className="shrink-0 text-xs text-[var(--color-text-tertiary)]">
          {formatBytes(file.bytes)}
        </span>
        <button
          ref={reloadButton}
          type="button"
          onClick={requestReload}
          disabled={saving || loading}
          title={t("workspaceReload", { defaultValue: "Reload file" })}
          aria-label={t("workspaceReload", { defaultValue: "Reload file" })}
          className={fileActionClass}
        >
          <RotateCcw className={classNames("h-3.5 w-3.5", loading && "animate-spin")} />
        </button>
        {hasSourcePreview && (
          <button
            type="button"
            onClick={() => setShowSource(!showSource)}
            title={t(showSource ? "workspacePreview" : "workspaceViewSource")}
            aria-label={t(showSource ? "workspacePreview" : "workspaceViewSource")}
            className={fileActionClass}
          >
            {showSource ? <Eye className="h-3.5 w-3.5" /> : <Code className="h-3.5 w-3.5" />}
          </button>
        )}
        {kind === "pdf" && (
          <a
            href={contentUrl}
            target="_blank"
            rel="noopener noreferrer"
            title={t("workspaceOpenSeparate")}
            aria-label={t("workspaceOpenSeparate")}
            className={fileActionClass}
          >
            <ExternalLink className="h-3.5 w-3.5" />
          </a>
        )}
        <a
          href={workspaceContentUrl(groupId, file, true)}
          download={baseName(file.path)}
          title={t("workspaceDownload")}
          aria-label={t("workspaceDownload")}
          className={fileActionClass}
        >
          <Download className="h-3.5 w-3.5" />
        </a>
        <button
          type="button"
          onClick={onAttach}
          title={t("workspaceAttachContext", { defaultValue: "Attach as context" })}
          className={classNames(fileActionClass, isDark ? "hover:bg-white/8" : "hover:bg-black/5")}
        >
          <Paperclip className="h-3.5 w-3.5" />
        </button>
        {editable ? (
          <button
            type="button"
            disabled={!dirty || saving || loading}
            onClick={() => void onSave(draft)}
            title={t("workspaceSave", { defaultValue: "Save" })}
            className={classNames(
              fileActionClass,
              !dirty || saving || loading
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
          className={classNames(fileActionClass, isDark ? "hover:bg-white/8" : "hover:bg-black/5")}
        >
          <X className="h-3.5 w-3.5" />
        </button>
      </div>

      {error ? (
        <div
          className={classNames(
            "flex items-center gap-2 px-3 py-2 text-xs",
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
              onClick={requestReload}
              disabled={saving || loading}
              className="flex shrink-0 items-center gap-1 rounded-md px-1.5 py-0.5 underline-offset-2 hover:underline"
            >
              <RotateCcw className="h-3 w-3" />
              {t("workspaceReload", { defaultValue: "Reload file" })}
            </button>
          ) : null}
        </div>
      ) : null}

      {preview ? (
        kind === "image" ? (
          <GraphicViewer src={imageUrl} alt={baseName(file.path)} />
        ) : kind === "video" || kind === "audio" ? (
          <WorkspaceMediaPreview
            key={contentUrl}
            src={contentUrl}
            name={baseName(file.path)}
            audio={kind === "audio"}
          />
        ) : kind === "pdf" ? (
          <div className="flex min-h-0 flex-1 flex-col">
            <p className="shrink-0 px-3 py-2 text-xs text-[var(--color-text-secondary)]">
              {t("workspacePdfHint")}
            </p>
            {navigator.pdfViewerEnabled !== false && (
              <iframe
                title={file.path}
                src={contentUrl}
                className="min-h-0 w-full flex-1 border-0 bg-white"
              />
            )}
          </div>
        ) : (
          <WorkspaceDocumentPreview
            kind={kind}
            groupId={groupId}
            file={file}
            content={draft}
            isDark={isDark}
            onOpenFile={onOpenFile}
            navigation={navigation}
          />
        )
      ) : file.truncated ? (
        <div className="px-3 py-6 text-center text-xs text-[var(--color-text-tertiary)]">
          {t("workspaceTooLarge", {
            defaultValue:
              "This file is too large for the text viewer ({{size}}). Download it to open locally.",
            size: formatBytes(file.bytes),
          })}
        </div>
      ) : file.binary ? (
        <div className="px-3 py-6 text-center text-xs text-[var(--color-text-tertiary)]">
          {t("workspaceBinary", { defaultValue: "Binary file — no text preview." })}
        </div>
      ) : editable ? (
        <textarea
          data-workspace-editor="true"
          value={editorText(draft)}
          spellCheck={false}
          onChange={(event) => setDraft(applyEditorText(draft, event.target.value, file.content))}
          onKeyDown={(event) => {
            if ((event.metaKey || event.ctrlKey) && event.key === "s") {
              event.preventDefault();
              if (dirty && !saving && !loading) void onSave(draft);
            }
          }}
          className={classNames(
            "min-h-0 flex-1 resize-none bg-transparent px-3 py-2 font-mono text-xs leading-relaxed outline-none",
            isDark ? "text-slate-200" : "text-slate-800",
          )}
        />
      ) : (
        <pre
          className={classNames(
            // Wraps rather than scrolling sideways: on a phone a horizontal drag per line is
            // unreadable, and the editable path (a textarea) already soft-wraps.
            "min-h-0 flex-1 overflow-auto px-3 py-2 font-mono text-xs leading-relaxed",
            "whitespace-pre-wrap break-words",
            isDark ? "text-slate-200" : "text-slate-800",
          )}
        >
          {draft}
        </pre>
      )}
      <Dialog open={confirmReload} onOpenChange={setConfirmReload}>
        <DialogContent
          className="gap-4 p-6"
          onEscapeKeyDown={(event) => event.stopPropagation()}
          onKeyDown={(event) => {
            // Keep the enclosing phone surface's document handlers out of this dialog.
            if (event.key === "Tab") event.stopPropagation();
          }}
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            cancelReload.current?.focus();
          }}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            reloadButton.current?.focus();
          }}
        >
          <DialogTitle className="pr-8">
            {t("workspaceReloadConfirm", { defaultValue: "Discard edits and reload?" })}
          </DialogTitle>
          <DialogDescription>
            {t("workspaceReloadWarning", {
              defaultValue:
                "Unsaved changes in this file will be replaced with its latest contents from disk.",
            })}
          </DialogDescription>
          <div className="flex flex-wrap justify-end gap-2">
            <DialogClose asChild>
              <button
                ref={cancelReload}
                type="button"
                className="rounded-lg border border-[var(--glass-border-subtle)] px-3 py-2 text-sm"
              >
                {t("workspaceCancel", { defaultValue: "Cancel" })}
              </button>
            </DialogClose>
            <button
              type="button"
              disabled={saving || loading}
              className="rounded-lg bg-[var(--color-accent-primary)] px-3 py-2 text-sm text-[var(--primary-foreground)] disabled:opacity-40"
              onClick={() => {
                setConfirmReload(false);
                onReload();
              }}
            >
              {t("workspaceDiscardReload", { defaultValue: "Discard and reload" })}
            </button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
