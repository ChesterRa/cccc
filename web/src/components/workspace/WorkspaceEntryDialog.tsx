import { useTranslation } from "react-i18next";
import type { WorkspaceEntry } from "../../types";
import { Button } from "../ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "../ui/dialog";
export type WorkspaceEntryAction = {
  kind: "file" | "folder" | "rename" | "move" | "delete";
  path: string;
  entry?: WorkspaceEntry;
};
export function WorkspaceEntryDialog({
  action,
  value,
  busy,
  error,
  hasDrafts,
  onValue,
  onClose,
  onSubmit,
}: {
  action: WorkspaceEntryAction | null;
  value: string;
  busy: boolean;
  error: string;
  hasDrafts: boolean;
  onValue: (value: string) => void;
  onClose: () => void;
  onSubmit: () => Promise<void>;
}) {
  const { t } = useTranslation("chat");
  return (
    <Dialog
      open={!!action}
      onOpenChange={(next) => {
        if (!next && !busy) onClose();
      }}
    >
      <DialogContent className="gap-4 p-6" onEscapeKeyDown={(event) => event.stopPropagation()}>
        <DialogTitle className="pr-6">
          {action &&
            t(
              `workspaceManage.${action.kind === "file" ? "newFile" : action.kind === "folder" ? "newFolder" : action.kind}`,
            )}
        </DialogTitle>
        <DialogDescription className="break-all">
          {action?.kind === "delete"
            ? t("workspaceManage.deleteConfirm", { path: action.path })
            : t("workspaceManage.target", { path: action?.path || "/" })}
        </DialogDescription>
        {action?.kind === "delete" ? (
          <>
            <p className="text-sm text-[var(--color-text-secondary)]">
              {t(
                action.entry?.is_symlink
                  ? "workspaceManage.deleteLink"
                  : "workspaceManage.deletePermanent",
              )}
            </p>
            {hasDrafts && (
              <p className="text-sm text-rose-600">{t("workspaceManage.deleteDrafts")}</p>
            )}
          </>
        ) : (
          <form
            id="workspace-entry-form"
            onSubmit={(event) => {
              event.preventDefault();
              void onSubmit();
            }}
          >
            <label className="text-sm">
              {t(action?.kind === "move" ? "workspaceManage.destination" : "workspaceManage.name")}
              <input
                autoFocus
                value={value}
                disabled={busy}
                onChange={(event) => onValue(event.target.value)}
                className="mt-2 w-full rounded-lg border border-[var(--glass-border-subtle)] bg-transparent px-3 py-2 outline-none focus:border-[var(--color-border-focus)]"
              />
            </label>
            {action?.kind === "move" && (
              <p className="mt-2 text-xs text-[var(--color-text-secondary)]">
                {t("workspaceManage.moveHint")}
              </p>
            )}
          </form>
        )}
        {error && (
          <p role="alert" className="break-words text-sm text-rose-600">
            {error}
          </p>
        )}
        <div className="flex justify-end gap-2">
          <Button
            autoFocus={action?.kind === "delete"}
            variant="outline"
            disabled={busy}
            onClick={() => onClose()}
          >
            {t("workspaceManage.cancel")}
          </Button>
          <Button disabled={busy} onClick={() => void onSubmit()}>
            {t(
              busy
                ? "workspaceManage.working"
                : action?.kind === "delete"
                  ? "workspaceManage.delete"
                  : "workspaceManage.apply",
            )}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
