import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { fetchWorkspaceFile, workspaceContentUrl } from "../../../services/api/workspace";
import type { WorkspaceResource } from "../../../stores/useResourcePanelStore";
import type { WorkspaceFile } from "../../../types";
import { WorkspaceFilePreview } from "../../workspace/WorkspaceFilePreview";
import { workspacePreviewKind } from "../../workspace/workspacePreview";
import type {
  WorkspaceFileNavigation,
  WorkspaceOpenFileOptions,
} from "../../workspace/useWorkspaceEditor";

type Loaded = { key: string; file: WorkspaceFile } | { key: string; error: string };

/**
 * A workspace file on one side of the split view, read on its own: showing it never changes the
 * file the shared editor holds, and an unsaved draft is shown as it stands. A link to another
 * file opens that file as a tab, the way it does when the file is shown alone.
 */
export function WorkspaceResourceView({
  groupId,
  resource,
  draft,
  isDark,
  onOpenFile,
  navigation,
  visible,
}: {
  groupId: string;
  resource: WorkspaceResource;
  draft: string | null;
  isDark: boolean;
  /** On screen; a hidden player is paused. */
  visible: boolean;
  onOpenFile?: (path: string, options?: WorkspaceOpenFileOptions) => void;
  /** Where a link into this file asked to land, when it is the file just opened. */
  navigation?: WorkspaceFileNavigation | null;
}) {
  const { t } = useTranslation(["chat", "common"]);
  const { path, scopeKey, scopeUrl } = resource;
  const key = JSON.stringify([groupId, scopeKey, scopeUrl, path]);
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  // A link's page in a PDF stays in its address, as in the file view: dropping it later would
  // load the PDF again from the first page.
  const [fragment, setFragment] = useState({ key, value: navigation?.fragment ?? "" });
  useEffect(() => {
    if (navigation) setFragment({ key, value: navigation.fragment });
  }, [key, navigation]);
  useEffect(() => {
    let live = true;
    void fetchWorkspaceFile(groupId, path, scopeKey, scopeUrl).then(
      (response) => {
        if (!live) return;
        setLoaded(
          response.ok
            ? { key, file: response.result }
            : { key, error: response.error.message || t("documentPreviewFailed") },
        );
      },
      () => {
        if (live) setLoaded({ key, error: t("documentPreviewFailed") });
      },
    );
    return () => {
      live = false;
    };
  }, [groupId, key, path, scopeKey, scopeUrl, t]);

  if (!loaded || loaded.key !== key)
    return (
      <p role="status" className="p-4 text-sm text-[var(--color-text-secondary)]">
        {t("common:loading")}
      </p>
    );
  if ("error" in loaded)
    return (
      <p role="alert" className="p-4 text-sm">
        {loaded.error}
      </p>
    );
  return (
    <div className="flex min-h-0 flex-1 flex-col" data-resource-split-file={path}>
      <WorkspaceFilePreview
        groupId={groupId}
        file={loaded.file}
        content={draft ?? loaded.file.content}
        contentUrl={
          workspaceContentUrl(groupId, loaded.file) +
          (workspacePreviewKind(loaded.file) === "pdf" && fragment.key === key
            ? fragment.value
            : "")
        }
        isDark={isDark}
        onOpenFile={onOpenFile}
        navigation={navigation}
        visible={visible}
      />
    </div>
  );
}
