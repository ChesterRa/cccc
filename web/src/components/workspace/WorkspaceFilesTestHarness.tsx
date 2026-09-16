import { WorkspaceFileViewer } from "./WorkspaceFileViewer";
import { WorkspaceFilesPanel } from "./WorkspaceFilesPanel";
import { useWorkspaceFiles } from "./useWorkspaceFiles";

export type HarnessProps = {
  readOnly?: boolean;
  groupId?: string;
  scopeKey?: string;
  scopeUrl?: string;
  onPinPath?: (path: string) => void;
  onAttachPath?: (path: string) => void;
};

/**
 * Mirrors how the chat shell composes the two surfaces: the tree stays in the side panel while
 * the opened file renders in the main area.
 */
export function Harness({
  readOnly = false,
  groupId = "group-1",
  scopeKey = "scope-a",
  scopeUrl = "/repo",
  onPinPath,
  onAttachPath = () => undefined,
}: HarnessProps) {
  const files = useWorkspaceFiles(groupId, true, scopeKey, scopeUrl);
  return (
    <div>
      <div data-testid="main-area">
        {files.file ? (
          <WorkspaceFileViewer
            groupId={groupId}
            {...{ draft: files.draft, setDraft: files.setDraft }}
            file={files.file}
            isDark={false}
            readOnly={readOnly}
            saving={files.saving}
            loading={files.fileLoading}
            reloadVersion={files.reloadVersion}
            error={files.fileError}
            conflict={files.conflict}
            onOpenFile={files.openFile}
            navigation={files.navigation}
            onClose={files.closeFile}
            onSave={files.saveFile}
            onReload={() => files.file && void files.openFile(files.file.path, { reload: true })}
            onAttach={() => files.file && onAttachPath(files.file.path)}
          />
        ) : (
          <span>chat</span>
        )}
      </div>
      {/* Stands in for the tree reload a save triggers, without routing through a write. */}
      <button type="button" data-testid="refresh" onClick={() => files.refresh()}>
        refresh
      </button>
      <div data-testid="side-panel">
        <WorkspaceFilesPanel
          files={files}
          isDark={false}
          readOnly={readOnly}
          onClose={() => undefined}
          onAttachPath={onAttachPath}
          onPinPath={onPinPath}
        />
      </div>
    </div>
  );
}
