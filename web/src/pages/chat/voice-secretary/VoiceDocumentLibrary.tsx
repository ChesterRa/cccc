import { useState, type ComponentProps } from "react";
import { Archive, FolderPlus } from "lucide-react";
import { Button } from "../../../components/ui/button";
import { IconButton } from "../../../components/ui/icon-button";
import type { AssistantVoiceDocument } from "../../../types";
import type { LibraryDocument, VoiceFolder } from "../../../services/api/voiceDocumentLibrary";
import { VoiceSecretaryDocumentListPanel } from "./VoiceSecretaryDocumentListPanel";
import { useVoiceDocumentLibrary } from "./useVoiceDocumentLibrary";
import { VoiceDocumentTree } from "./VoiceDocumentTree";
import { VoiceArchiveDialog, VoiceFolderDialog } from "./VoiceLibraryDialogs";

type Props = ComponentProps<typeof VoiceSecretaryDocumentListPanel> & {
  groupId: string;
  onRestored: (document: LibraryDocument) => void;
  onRenamed?: () => void;
};
export function VoiceDocumentLibrary(props: Props) {
  // Group changes remount local navigation and dialogs, preventing cross-group selection.
  return <Library key={props.groupId} {...props} />;
}
function Library(props: Props) {
  const { t } = props;
  const library = useVoiceDocumentLibrary(props.groupId, props.documents, props.actionBusy);
  const [archiveOpen, setArchiveOpen] = useState(false);
  const [dialog, setDialog] = useState<ComponentProps<typeof VoiceFolderDialog>["mode"] | null>(
    null,
  );
  const folders = library.data.folders || [];
  const archived = library.data.documents.filter((document) => document.status === "archived");
  const folderIds = new Map(
    library.data.documents.map((document) => [props.documentPath(document), document.folder_id]),
  );
  const folderOf = (document: AssistantVoiceDocument) =>
    folderIds.get(props.documentPath(document)) || "";
  const busy = library.busy || !!props.actionBusy || !!props.recording;
  const rowContext = {
    ...props,
    onMoveDocument: (document: AssistantVoiceDocument) => setDialog({ kind: "move", document }),
    onRenameDocument: (document: AssistantVoiceDocument) =>
      setDialog({ kind: "rename_document", document }),
  };
  const folderTitle = t("voiceFolderCreate", { defaultValue: "New folder" });
  const removeFolder = async (target: VoiceFolder) => {
    if (
      !window.confirm(
        t("voiceFolderRemoveConfirm", {
          name: target.name,
          defaultValue: 'Remove folder "{{name}}"? Documents return to the unfiled list.',
        }),
      )
    )
      return;
    await library.mutate({ action: "remove_folder", folder_id: target.folder_id });
  };
  return (
    <>
      <VoiceSecretaryDocumentListPanel
        {...rowContext}
        headerActions={
          <IconButton
            variant="ghost"
            size="sm"
            label={folderTitle}
            disabled={busy}
            onClick={() => setDialog({ kind: "create" })}
          >
            <FolderPlus size={16} />
          </IconButton>
        }
        navigation={
          library.error ? (
            <p role="alert" className="mb-2 break-words text-xs text-rose-600">
              {library.error}
            </p>
          ) : null
        }
        footer={
          <div className="shrink-0 border-t border-[var(--glass-border-subtle)] p-2.5">
            <Button
              variant="ghost"
              className="w-full justify-start gap-2"
              onClick={() => setArchiveOpen(true)}
            >
              <Archive size={16} />
              {t("voiceArchiveTitle", { defaultValue: "Archived documents" })}
              <span className="ml-auto text-xs">{archived.length}</span>
            </Button>
          </div>
        }
      >
        <VoiceDocumentTree
          ctx={rowContext}
          documents={props.documents}
          folders={folders}
          folderOf={folderOf}
          busy={busy}
          onDropDocument={(document, folderId) =>
            library.mutate({
              action: "move",
              document_path: props.documentPath(document),
              folder_id: folderId,
            })
          }
          rootOrder={library.data.root_order || []}
          onReorderRoot={(rootOrder) =>
            library.mutate({ action: "reorder_root", root_order: rootOrder })
          }
          onRenameFolder={(folder) => setDialog({ kind: "rename", folder })}
          onRemoveFolder={(folder) => void removeFolder(folder)}
        />
      </VoiceSecretaryDocumentListPanel>
      {dialog ? (
        <VoiceFolderDialog
          error={library.error}
          mode={dialog}
          folders={folders}
          busy={busy}
          t={t}
          onClose={() => setDialog(null)}
          onSubmit={async (value) => {
            const ok = await library.mutate(
              dialog.kind === "move"
                ? {
                    action: "move",
                    document_path: props.documentPath(dialog.document!),
                    folder_id: value,
                  }
                : dialog.kind === "rename_document"
                  ? {
                      action: "rename",
                      document_path: props.documentPath(dialog.document!),
                      name: value,
                    }
                  : {
                      action: dialog.kind === "create" ? "create_folder" : "rename_folder",
                      folder_id: dialog.folder?.folder_id,
                      name: value,
                    },
            );
            if (!ok) return;
            if (dialog.kind === "rename_document") props.onRenamed?.();
            setDialog(null);
          }}
        />
      ) : null}
      {archiveOpen ? (
        <VoiceArchiveDialog
          error={library.error}
          documents={archived}
          busy={busy}
          isDark={props.isDark}
          t={t}
          onClose={() => setArchiveOpen(false)}
          onDelete={(document) => props.onDeleteDocument?.(document)}
          onRestore={async (document) => {
            if (
              await library.mutate({
                action: "restore",
                document_path: props.documentPath(document),
              })
            )
              props.onRestored({ ...document, status: "active" });
          }}
        />
      ) : null}
    </>
  );
}
