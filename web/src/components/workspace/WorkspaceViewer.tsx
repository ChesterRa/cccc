import { lazy, Suspense } from "react";
import { useTranslation } from "react-i18next";
import { WorkspaceFileViewer } from "./WorkspaceFileViewer";
import type { WorkspaceFilesController } from "./useWorkspaceFiles";
const WorkspaceDiffViewer = lazy(() =>
  import("./WorkspaceDiffViewer").then((module) => ({ default: module.WorkspaceDiffViewer })),
);

export function WorkspaceViewer({
  files,
  isDark,
  readOnly,
  onAttach,
  inPanel = false,
  visible = true,
}: {
  files: WorkspaceFilesController;
  isDark: boolean;
  readOnly: boolean;
  onAttach: (path: string) => void;
  inPanel?: boolean;
  /** On screen; a hidden player is paused. */
  visible?: boolean;
}) {
  const { t } = useTranslation("chat");
  if (files.mode === "changes")
    return (
      <Suspense
        fallback={
          <p role="status" className="p-4 text-sm">
            {t("workspaceGit.loading")}
          </p>
        }
      >
        <WorkspaceDiffViewer files={files} />
      </Suspense>
    );
  if (!files.file) return null;
  return (
    <WorkspaceFileViewer
      onOpenFile={files.openFile}
      navigation={files.navigation}
      groupId={files.groupId}
      draft={files.draft}
      setDraft={files.setDraft}
      file={files.file}
      isDark={isDark}
      readOnly={readOnly}
      saving={files.saving || files.changingEntries}
      loading={files.fileLoading}
      reloadVersion={files.reloadVersion}
      error={files.fileError}
      conflict={files.conflict}
      onClose={files.closeFile}
      onSave={files.saveFile}
      onReload={() => files.file && void files.openFile(files.file.path, { reload: true })}
      onAttach={() => files.file && onAttach(files.file.path)}
      inPanel={inPanel}
      visible={visible}
    />
  );
}
