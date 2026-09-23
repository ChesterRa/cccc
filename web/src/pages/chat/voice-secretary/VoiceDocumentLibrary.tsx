import { useState, type ComponentProps } from "react";
import { Archive, ChevronLeft, Folder, FolderPlus, Pencil, X } from "lucide-react";
import { Button } from "../../../components/ui/button";
import type { LibraryDocument, VoiceFolder } from "../../../services/api/voiceDocumentLibrary";
import { VoiceSecretaryDocumentListPanel } from "./VoiceSecretaryDocumentListPanel";
import { useVoiceDocumentLibrary } from "./useVoiceDocumentLibrary";
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
  const [folderId, setFolderId] = useState("");
  const [archiveOpen, setArchiveOpen] = useState(false);
  const [dialog, setDialog] = useState<ComponentProps<typeof VoiceFolderDialog>["mode"] | null>(
    null,
  );
  const folders = library.data.folders || [];
  const folder = folders.find((folder) => folder.folder_id === folderId);
  const archived = library.data.documents.filter((document) => document.status === "archived");
  const metadata = new Map(
    library.data.documents.map((document) => [props.documentPath(document), document]),
  );
  const visible = props.documents.filter(
    (document) =>
      (metadata.get(props.documentPath(document))?.folder_id || "") === (folder?.folder_id || ""),
  );
  const busy = library.busy || !!props.actionBusy || !!props.recording;
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
    if (await library.mutate({ action: "remove_folder", folder_id: target.folder_id }))
      setFolderId("");
  };
  return (
    <>
      <VoiceSecretaryDocumentListPanel
        {...props}
        emptyLabel={t("voiceFolderEmpty", { defaultValue: "No documents in this folder" })}
        documents={visible}
        onStartCreateDocument={() => {
          setFolderId("");
          props.onStartCreateDocument();
        }}
        onMoveDocument={(document) => setDialog({ kind: "move", document })}
        onRenameDocument={(document) => setDialog({ kind: "rename_document", document })}
        navigation={
          <div className="space-y-1 border-b border-[var(--glass-border-subtle)] pb-2 mb-2">
            {library.error ? (
              <p role="alert" className="break-words text-xs text-rose-600">
                {library.error}
              </p>
            ) : null}
            {folder ? (
              <Button
                variant="ghost"
                className="w-full justify-start gap-2"
                onClick={() => setFolderId("")}
              >
                <ChevronLeft size={16} />
                <span className="truncate">{folder.name}</span>
              </Button>
            ) : (
              <>
                <Button
                  variant="ghost"
                  disabled={busy}
                  className="w-full justify-start gap-2"
                  onClick={() => setDialog({ kind: "create" })}
                >
                  <FolderPlus size={16} />
                  {folderTitle}
                </Button>
                {folders.map((folder) => (
                  <div
                    key={folder.folder_id}
                    className="flex min-w-0 items-center gap-1"
                    data-voice-folder
                  >
                    <Button
                      variant="ghost"
                      className="min-w-0 flex-1 justify-start gap-2"
                      title={folder.name}
                      onClick={() => setFolderId(folder.folder_id)}
                    >
                      <Folder size={16} className="shrink-0" />
                      <span className="truncate">{folder.name}</span>
                      <span className="ml-auto text-xs text-[var(--color-text-muted)]">
                        {
                          props.documents.filter(
                            (d) =>
                              metadata.get(props.documentPath(d))?.folder_id === folder.folder_id,
                          ).length
                        }
                      </span>
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      disabled={busy}
                      aria-label={t("voiceFolderRename", { defaultValue: "Rename folder" })}
                      onClick={() => setDialog({ kind: "rename", folder })}
                    >
                      <Pencil size={14} />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      disabled={busy}
                      aria-label={t("voiceFolderRemove", { defaultValue: "Remove folder" })}
                      onClick={() => void removeFolder(folder)}
                    >
                      <X size={14} />
                    </Button>
                  </div>
                ))}
              </>
            )}
          </div>
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
      />
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
