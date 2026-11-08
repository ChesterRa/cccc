import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { classNames } from "../../utils/classNames";
import type { WorkspaceFile } from "../../types";
import { GraphicViewer } from "../viewer/GraphicViewer";
import { PdfPreview } from "../viewer/PdfPreview";
import { WorkspaceMediaPreview } from "./WorkspaceMediaPreview";
import { WorkspaceDocumentPreview } from "./WorkspaceDocumentPreview";
import { workspacePreviewKind } from "./workspacePreview";
import { baseName } from "./workspaceTreeModel";
import { formatBytes } from "./workspaceText";
import type { WorkspaceFileNavigation, WorkspaceOpenFileOptions } from "./useWorkspaceEditor";

/**
 * A workspace file as it reads, not as it is edited: pictures, players, PDFs, rendered
 * documents and tables, or the text itself. The file viewer and the resource panel's side-by-side
 * view both show files through it.
 */
export function WorkspaceFilePreview({
  groupId,
  file,
  content,
  contentUrl,
  isDark,
  showSource = false,
  onOpenFile,
  navigation,
  visible = true,
}: {
  groupId: string;
  file: WorkspaceFile;
  /** The text to show, which is the unsaved draft when there is one. */
  content: string;
  /** Where the browser fetches the file itself, for pictures, players and PDFs. */
  contentUrl: string;
  isDark: boolean;
  /** Markdown, HTML and tables as their source text rather than rendered. */
  showSource?: boolean;
  onOpenFile?: (path: string, options?: WorkspaceOpenFileOptions) => void;
  navigation?: WorkspaceFileNavigation | null;
  /** On screen; a hidden player is paused. */
  visible?: boolean;
}) {
  const { t } = useTranslation("chat");
  const kind = workspacePreviewKind(file);
  const svgSource = file.mime_type === "image/svg+xml" && !file.binary && !file.truncated;
  const dirty = content !== file.content;
  // Render SVG drafts through an image, never inject workspace markup into the App DOM.
  const imageUrl = useMemo(
    () => (svgSource && dirty ? `data:image/svg+xml,${encodeURIComponent(content)}` : contentUrl),
    [svgSource, dirty, content, contentUrl],
  );

  if (kind !== "text" && !showSource) {
    if (kind === "image")
      return <GraphicViewer src={imageUrl} alt={baseName(file.path)} isSvg={svgSource} />;
    if (kind === "video" || kind === "audio")
      return (
        <WorkspaceMediaPreview
          key={contentUrl}
          src={contentUrl}
          name={baseName(file.path)}
          audio={kind === "audio"}
          visible={visible}
        />
      );
    if (kind === "pdf")
      return (
        <PdfPreview
          href={contentUrl}
          title={file.path}
          hint={t("workspacePdfHint")}
          failedText={t("documentPreviewFailed")}
        />
      );
    return (
      <WorkspaceDocumentPreview
        kind={kind}
        groupId={groupId}
        file={file}
        content={content}
        isDark={isDark}
        onOpenFile={onOpenFile}
        navigation={navigation}
      />
    );
  }
  if (file.truncated)
    return (
      <div className="px-3 py-6 text-center text-xs text-[var(--color-text-tertiary)]">
        {t("workspaceTooLarge", {
          defaultValue:
            "This file is too large for the text viewer ({{size}}). Download it to open locally.",
          size: formatBytes(file.bytes),
        })}
      </div>
    );
  if (file.binary)
    return (
      <div className="px-3 py-6 text-center text-xs text-[var(--color-text-tertiary)]">
        {t("workspaceBinary", { defaultValue: "Binary file — no text preview." })}
      </div>
    );
  return (
    <pre
      data-workspace-text
      data-workspace-scroll
      className={classNames(
        // Wraps rather than scrolling sideways: on a phone a horizontal drag per line is
        // unreadable, and the editable path (a textarea) already soft-wraps.
        "min-h-0 flex-1 overflow-auto px-3 py-2 font-mono text-xs leading-relaxed",
        "whitespace-pre-wrap break-words",
        isDark ? "text-slate-200" : "text-slate-800",
      )}
    >
      {content}
    </pre>
  );
}
