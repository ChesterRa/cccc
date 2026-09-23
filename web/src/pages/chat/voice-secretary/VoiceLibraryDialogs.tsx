import { useState } from "react";
import type { TFunction } from "i18next";
import {
  Dialog,
  DialogContent,
  DialogTitle,
  DialogDescription,
} from "../../../components/ui/dialog";
import { Button } from "../../../components/ui/button";
import { Input } from "../../../components/ui/input";
import { MarkdownDocumentSurface } from "../../../components/document/MarkdownDocumentSurface";
import type { LibraryDocument, VoiceFolder } from "../../../services/api/voiceDocumentLibrary";

export function VoiceFolderDialog({
  mode,
  folders,
  busy,
  error,
  onClose,
  onSubmit,
  t,
}: {
  mode: {
    kind: "create" | "rename" | "move" | "rename_document";
    folder?: VoiceFolder;
    document?: LibraryDocument;
  };
  folders: VoiceFolder[];
  busy: boolean;
  error?: string;
  onClose: () => void;
  onSubmit: (value: string) => void;
  t: TFunction;
}) {
  const [name, setName] = useState(
    (mode.kind === "rename_document" ? mode.document?.title : mode.folder?.name) || "",
  );
  const title =
    mode.kind === "move"
      ? t("voiceFolderMove", { defaultValue: "Move to folder" })
      : mode.kind === "rename_document"
        ? t("voiceSecretaryDocumentRenameTitle", { defaultValue: "Rename document" })
        : mode.kind === "rename"
          ? t("voiceFolderRename", { defaultValue: "Rename folder" })
          : t("voiceFolderCreate", { defaultValue: "New folder" });
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !busy) onClose();
      }}
    >
      <DialogContent className="max-w-sm gap-3 p-5">
        <DialogTitle className="pr-8">{title}</DialogTitle>
        {error ? (
          <p role="alert" className="text-sm text-rose-600">
            {error}
          </p>
        ) : null}
        <DialogDescription>
          {mode.kind === "rename_document"
            ? t("voiceSecretaryDocumentRenameHint", {
                defaultValue: "Only the title changes; the file keeps its path.",
              })
            : mode.document?.title ||
              t("voiceFolderNameHint", { defaultValue: "Use folders to organize your documents." })}
        </DialogDescription>
        {mode.kind === "move" ? (
          <div className="max-h-72 space-y-1 overflow-auto">
            {[
              { folder_id: "", name: t("voiceFolderRoot", { defaultValue: "Unfiled documents" }) },
              ...folders,
            ].map((folder) => (
              <Button
                key={folder.folder_id}
                className="w-full justify-start"
                variant="ghost"
                disabled={busy}
                onClick={() => onSubmit(folder.folder_id)}
              >
                {folder.name}
              </Button>
            ))}
          </div>
        ) : (
          <form
            className="space-y-3"
            onSubmit={(event) => {
              event.preventDefault();
              if (name.trim() && !busy) onSubmit(name.trim());
            }}
          >
            <Input
              aria-label={title}
              autoFocus
              value={name}
              maxLength={80}
              onChange={(event) => setName(event.target.value)}
            />
            <Button type="submit" disabled={busy || !name.trim()}>
              {t("voiceFolderSave", { defaultValue: "Save" })}
            </Button>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}

export function VoiceArchiveDialog({
  documents,
  busy,
  error,
  isDark,
  onClose,
  onRestore,
  onDelete,
  t,
}: {
  documents: LibraryDocument[];
  busy: boolean;
  error?: string;
  isDark: boolean;
  onClose: () => void;
  onRestore: (document: LibraryDocument) => void;
  onDelete: (document: LibraryDocument) => void;
  t: TFunction;
}) {
  const [selected, setSelected] = useState("");
  const viewed = documents.find((document) => document.document_id === selected);
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent className="max-w-3xl max-h-[85dvh] gap-3 overflow-auto p-5">
        <DialogTitle className="pr-8">
          {t("voiceArchiveTitle", { defaultValue: "Archived documents" })}
        </DialogTitle>
        {error ? (
          <p role="alert" className="text-sm text-rose-600">
            {error}
          </p>
        ) : null}
        <DialogDescription>
          {t("voiceArchiveHint", {
            defaultValue: "Archived documents are kept here. Deleted documents are not included.",
          })}
        </DialogDescription>
        {documents.length ? (
          documents.map((document) => (
            <div
              key={document.document_id}
              className="flex flex-wrap items-center gap-2 border-b border-[var(--glass-border-subtle)] py-2"
            >
              <Button
                variant="ghost"
                className="h-auto min-w-0 flex-1 justify-start whitespace-normal text-left"
                onClick={() => setSelected(document.document_id)}
              >
                {document.title}
              </Button>
              <Button variant="outline" disabled={busy} onClick={() => onRestore(document)}>
                {t("voiceArchiveRestore", { defaultValue: "Restore" })}
              </Button>
              <Button
                variant="ghost"
                className="text-rose-600 dark:text-rose-400"
                disabled={busy}
                onClick={() => onDelete(document)}
              >
                {t("voiceSecretaryDocumentDeleteAction", { defaultValue: "Delete" })}
              </Button>
            </div>
          ))
        ) : (
          <p>{t("voiceArchiveEmpty", { defaultValue: "No archived documents" })}</p>
        )}
        {viewed ? (
          <section aria-label={viewed.title} className="min-w-0 border-t pt-3">
            <h3 className="mb-2 font-semibold break-words">{viewed.title}</h3>
            <MarkdownDocumentSurface content={viewed.content || ""} isDark={isDark} />
          </section>
        ) : null}
      </DialogContent>
    </Dialog>
  );
}
