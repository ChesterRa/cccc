import { useLayoutEffect, useRef, useState, type DragEvent } from "react";
import { useTranslation } from "react-i18next";
import { workspaceRelativePath } from "./workspacePath";
import type { WorkspaceEntry } from "../../types";
import { WorkspaceEntryDialog, type WorkspaceEntryAction as Action } from "./WorkspaceEntryDialog";
import type { WorkspaceMenuItem } from "./WorkspaceEntryMenu";
import type { WorkspaceFilesController } from "./useWorkspaceFiles";
import {
  droppedEntries,
  joinWorkspacePath,
  pickerEntries,
  WORKSPACE_DRAG_TYPE,
} from "./workspaceUploads";

const parentOf = (path: string) => path.slice(0, Math.max(0, path.lastIndexOf("/")));

/** Menus, pickers and native drop share the same scoped mutation controller. */
export function useWorkspaceActions(files: WorkspaceFilesController, readOnly: boolean) {
  const { t } = useTranslation("chat");
  const [action, setAction] = useState<Action | null>(null);
  const [value, setValue] = useState("");
  const [localError, setLocalError] = useState("");
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const filePicker = useRef<HTMLInputElement>(null);
  const folderPicker = useRef<HTMLInputElement>(null);
  const pickerTarget = useRef("");
  const activeDrag = useRef<{ token: string; path: string; name: string } | null>(null);
  useLayoutEffect(() => {
    activeDrag.current = null;
    return () => {
      activeDrag.current = null;
    };
  }, [files.groupId, files.scopeKey, files.scopeUrl]);
  const canWrite = !readOnly && files.scopeAvailable && !files.mutations.busy;
  const open = (next: Action) => {
    setLocalError("");
    files.mutations.clearFeedback();
    setValue(
      next.kind === "rename" ? next.entry?.name || "" : next.kind === "move" ? next.path : "",
    );
    setAction(next);
  };
  const pick = (directory: string, folder: boolean) => {
    pickerTarget.current = directory;
    (folder ? folderPicker : filePicker).current?.click();
  };
  const createItems = (directory: string): WorkspaceMenuItem[] =>
    readOnly
      ? []
      : [
          {
            key: "new-file",
            label: t("workspaceManage.newFile"),
            disabled: !canWrite,
            onSelect: () => open({ kind: "file", path: directory }),
          },
          {
            key: "new-folder",
            label: t("workspaceManage.newFolder"),
            disabled: !canWrite,
            onSelect: () => open({ kind: "folder", path: directory }),
          },
          {
            key: "upload-files",
            label: t("workspaceManage.uploadFiles"),
            disabled: !canWrite,
            onSelect: () => pick(directory, false),
          },
          {
            key: "upload-folder",
            label: t("workspaceManage.uploadFolder"),
            disabled: !canWrite,
            onSelect: () => pick(directory, true),
          },
        ];
  const entryItems = (entry: WorkspaceEntry): WorkspaceMenuItem[] =>
    readOnly
      ? []
      : [
          ...(entry.is_dir && !entry.unavailable ? createItems(entry.path) : []),
          ...(["rename", "move", "delete"] as const).map((kind) => ({
            key: kind,
            label: t(`workspaceManage.${kind}`),
            disabled: !canWrite,
            onSelect: () => open({ kind, path: entry.path, entry }),
          })),
        ];
  const submit = async () => {
    if (!action || !canWrite) return;
    setLocalError("");
    const isMove = action.kind === "move";
    const windows = /^[A-Za-z]:[\\/]|^\\\\/.test(files.rootPath);
    if (
      action.kind !== "delete" &&
      (!value ||
        value === "." ||
        value === ".." ||
        value.includes("\0") ||
        (!isMove && (value.includes("/") || (windows && value.includes("\\")))))
    ) {
      setLocalError(t("workspaceManage.invalidPath"));
      return;
    }
    const moveDestination = isMove ? workspaceRelativePath(value, files.rootPath) : value;
    if (moveDestination === null) {
      setLocalError(t("workspaceManage.invalidPath"));
      return;
    }
    const operation =
      action.kind === "delete"
        ? { operation: "delete" as const, path: action.path }
        : action.kind === "rename" || isMove
          ? {
              operation: "move" as const,
              path: action.path,
              destination: isMove
                ? moveDestination
                : joinWorkspacePath(parentOf(action.path), value),
            }
          : {
              operation: "create" as const,
              path: joinWorkspacePath(action.path, value),
              directory: action.kind === "folder",
            };
    if (await files.mutations.change(operation)) {
      setAction(null);
      if (operation.operation === "create") {
        if (operation.directory) files.revealFile(operation.path, true);
        else void files.openFile(operation.path);
      } else if (operation.operation === "move")
        files.revealFile(operation.destination, action.entry?.is_dir);
    }
  };
  const isFileDrag = (event: DragEvent) =>
    Array.from(event.dataTransfer.types).some(
      (type) => type === "Files" || type === WORKSPACE_DRAG_TYPE,
    );
  const destinationOf = (event: DragEvent) =>
    event.target instanceof Element
      ? event.target.closest<HTMLElement>("[data-workspace-directory]")?.dataset
          .workspaceDirectory || ""
      : "";
  const dragProps = {
    "data-workspace-drop": true,
    onDragEnd: () => {
      activeDrag.current = null;
      setDropTarget(null);
    },
    onDragOver: (event: DragEvent<HTMLDivElement>) => {
      if (!isFileDrag(event)) return;
      event.preventDefault();
      event.stopPropagation();
      event.dataTransfer.dropEffect = canWrite
        ? event.dataTransfer.types.includes(WORKSPACE_DRAG_TYPE)
          ? activeDrag.current
            ? "move"
            : "none"
          : "copy"
        : "none";
      setDropTarget(canWrite ? destinationOf(event) : null);
    },
    onDragLeave: (event: DragEvent<HTMLDivElement>) => {
      if (
        !(event.relatedTarget instanceof Node) ||
        !event.currentTarget.contains(event.relatedTarget)
      )
        setDropTarget(null);
    },
    onDrop: (event: DragEvent<HTMLDivElement>) => {
      if (!isFileDrag(event)) return;
      event.preventDefault();
      event.stopPropagation();
      setDropTarget(null);
      const source = activeDrag.current;
      activeDrag.current = null;
      if (!canWrite) return;
      setLocalError("");
      const destination = destinationOf(event);
      const raw = event.dataTransfer.getData(WORKSPACE_DRAG_TYPE);
      if (event.dataTransfer.types.includes(WORKSPACE_DRAG_TYPE)) {
        if (!source || raw !== source.token) {
          setLocalError(t("workspaceManage.sameWorkspace"));
          return;
        }
        void files.mutations.change({
          operation: "move",
          path: source.path,
          destination: joinWorkspacePath(destination, source.name),
        });
      } else void files.mutations.upload(destination, droppedEntries(event.dataTransfer));
    },
  };
  const startDrag = canWrite
    ? (event: DragEvent, entry: WorkspaceEntry) => {
        event.dataTransfer.effectAllowed = "move";
        // Only this mounted panel can authorize a move; transferred metadata is not authority.
        const token = crypto.getRandomValues(new Uint32Array(4)).join("-");
        activeDrag.current = { token, path: entry.path, name: entry.name };
        event.dataTransfer.setData(WORKSPACE_DRAG_TYPE, token);
      }
    : undefined;
  const pickFiles = (input: HTMLInputElement) => {
    const selection = Array.from(input.files || []);
    input.value = "";
    if (!selection.length) return;
    void files.mutations.upload(
      pickerTarget.current,
      Promise.resolve().then(() => pickerEntries(selection)),
    );
  };
  const progress = files.mutations.uploadProgress;
  const controls = (
    <>
      <input
        ref={filePicker}
        type="file"
        multiple
        hidden
        onChange={(e) => pickFiles(e.currentTarget)}
      />
      <input
        ref={folderPicker}
        type="file"
        multiple
        hidden
        {...{ webkitdirectory: "" }}
        onChange={(e) => pickFiles(e.currentTarget)}
      />
      {dropTarget !== null && (
        <p
          role="status"
          className="shrink-0 break-all border-y border-[var(--color-border-focus)] px-3 py-2 text-xs"
        >
          {t("workspaceManage.dropHere", { path: dropTarget || "/" })}
        </p>
      )}
      {(localError || files.mutations.error) && !action && (
        <p role="alert" className="shrink-0 break-words px-3 py-2 text-xs text-rose-600">
          {localError || files.mutations.error}
        </p>
      )}
      {progress && (
        <div
          role="status"
          className="shrink-0 border-b border-[var(--glass-border-subtle)] px-3 py-2 text-xs"
        >
          <p>
            {t(
              progress.stopped ? "workspaceManage.uploadStopped" : "workspaceManage.uploadProgress",
              progress,
            )}
          </p>
          {!progress.stopped ? (
            <button className="mt-1 underline" onClick={files.mutations.cancelUpload}>
              {t("workspaceManage.cancelUpload")}
            </button>
          ) : (
            <button className="mt-1 underline" onClick={files.mutations.clearFeedback}>
              {t("workspaceManage.dismiss")}
            </button>
          )}
        </div>
      )}
      <WorkspaceEntryDialog
        action={action}
        value={value}
        busy={files.mutations.busy}
        error={localError || files.mutations.error}
        hasDrafts={!!action && files.hasDraftsUnder(action.path)}
        onValue={setValue}
        onClose={() => setAction(null)}
        onSubmit={submit}
      />
    </>
  );
  return { createItems, entryItems, controls, dragProps, startDrag, dropTarget };
}
