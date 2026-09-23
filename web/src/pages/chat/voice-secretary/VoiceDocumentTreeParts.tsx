import type { ReactNode } from "react";
import { useDraggable, useDroppable } from "@dnd-kit/core";
import { useSortable } from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { ChevronRight, Folder, FolderOpen, Pencil, X } from "lucide-react";
import { IconButton } from "../../../components/ui/icon-button";
import type { VoiceFolder } from "../../../services/api/voiceDocumentLibrary";
import type { AssistantVoiceDocument } from "../../../types";
import { classNames } from "../../../utils/classNames";
import { VoiceDocumentRow, type VoiceDocumentRowContext } from "./VoiceDocumentRow";
import { ROOT_DROP_ID, type DragItem, type DropTarget } from "./voiceDocumentTreeModel";

export function DraggableDocumentRow({
  ctx,
  document,
  depth,
  disabled,
}: {
  ctx: VoiceDocumentRowContext;
  document: AssistantVoiceDocument;
  depth: number;
  disabled: boolean;
}) {
  const id = ctx.documentPath(document) || ctx.documentKey(document);
  const { setNodeRef, attributes, listeners, isDragging } = useDraggable({
    id,
    data: { document } satisfies DragItem,
    disabled: disabled || !ctx.documentPath(document),
  });
  return (
    <VoiceDocumentRow
      ctx={ctx}
      document={document}
      depth={depth}
      drag={{ ref: setNodeRef, props: { ...attributes, ...listeners }, dragging: isDragging }}
    />
  );
}

/** A root document's place in the sortable order: a folder can land here, it never drags. */
export function RootSlot({ rootKey, children }: { rootKey: string; children: ReactNode }) {
  const { setNodeRef, transform, transition } = useSortable({
    id: rootKey,
    data: { rootKey } satisfies DropTarget,
    disabled: { draggable: true },
  });
  return (
    <div
      ref={setNodeRef}
      data-voice-root-slot
      style={{ transform: CSS.Translate.toString(transform), transition }}
    >
      {children}
    </div>
  );
}

/** A folder header plus its expanded documents, moved together when reordering. */
export function SortableFolder({
  ctx,
  folder,
  rootKey,
  count,
  open,
  busy,
  acceptsDocument,
  onToggle,
  onRename,
  onRemove,
  children,
}: {
  ctx: VoiceDocumentRowContext;
  folder: VoiceFolder;
  rootKey: string;
  count: number;
  open: boolean;
  busy: boolean;
  acceptsDocument: boolean;
  onToggle: () => void;
  onRename: () => void;
  onRemove: () => void;
  children: ReactNode;
}) {
  const { isDark, t } = ctx;
  const {
    setNodeRef,
    setActivatorNodeRef,
    attributes,
    listeners,
    transform,
    transition,
    isDragging,
    isOver,
  } = useSortable({
    id: rootKey,
    data: { folder, rootKey, folderId: folder.folder_id } satisfies DragItem & DropTarget,
    disabled: busy,
  });
  const highlighted = isOver && acceptsDocument;
  const FolderIcon = open || highlighted ? FolderOpen : Folder;
  return (
    <div
      ref={setNodeRef}
      data-voice-folder-group
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={classNames("space-y-0.5", isDragging && "opacity-40")}
    >
      <div
        data-voice-folder
        className={classNames(
          "group/folder flex min-w-0 items-center rounded-lg pr-1 transition-colors",
          highlighted
            ? isDark
              ? "bg-sky-400/20 ring-1 ring-sky-300/60"
              : "bg-sky-50 ring-1 ring-sky-400/70"
            : isDark
              ? "hover:bg-white/8"
              : "hover:bg-black/[0.04]",
        )}
      >
        <button
          ref={setActivatorNodeRef}
          {...attributes}
          {...listeners}
          type="button"
          aria-expanded={open}
          title={folder.name}
          onClick={onToggle}
          className={classNames(
            "flex min-w-0 flex-1 items-center gap-1 py-1.5 pl-0.5 text-left text-sm font-medium outline-none focus-visible:ring-2 rounded-lg pointer-coarse:py-2.5",
            isDark
              ? "text-slate-200 focus-visible:ring-white/35"
              : "text-gray-800 focus-visible:ring-black/25",
          )}
        >
          <ChevronRight
            size={14}
            aria-hidden="true"
            className={classNames("shrink-0 opacity-50 transition-transform", open && "rotate-90")}
          />
          <FolderIcon size={15} aria-hidden="true" className="shrink-0 opacity-70" />
          <span className="min-w-0 flex-1 truncate pl-0.5">{folder.name}</span>
          <span className="shrink-0 px-1 text-xs font-normal text-[var(--color-text-muted)]">
            {count}
          </span>
        </button>
        <FolderAction
          label={t("voiceFolderRename", { defaultValue: "Rename folder" })}
          disabled={busy}
          onClick={onRename}
        >
          <Pencil size={13} />
        </FolderAction>
        <FolderAction
          label={t("voiceFolderRemove", { defaultValue: "Remove folder" })}
          disabled={busy}
          onClick={onRemove}
        >
          <X size={13} />
        </FolderAction>
      </div>
      {children}
    </div>
  );
}

function FolderAction({
  label,
  disabled,
  onClick,
  children,
}: {
  label: string;
  disabled: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <IconButton
      variant="ghost"
      size="sm"
      label={label}
      disabled={disabled}
      onClick={onClick}
      // Revealed on hover like row menus; always visible where there is no hover.
      className="shrink-0 text-[var(--color-text-tertiary)] opacity-0 group-hover/folder:opacity-100 focus-visible:opacity-100 pointer-coarse:opacity-100"
    >
      {children}
    </IconButton>
  );
}

export function RootDropZone({
  isDark,
  active,
  children,
}: {
  isDark: boolean;
  active: boolean;
  children: ReactNode;
}) {
  const { setNodeRef, isOver } = useDroppable({
    id: ROOT_DROP_ID,
    data: { folderId: "" } satisfies DropTarget,
    disabled: !active,
  });
  return (
    <div
      ref={setNodeRef}
      data-voice-folder-root
      className={classNames(
        "min-h-16 space-y-0.5 rounded-lg pb-6 transition-colors",
        active && "outline-1 -outline-offset-1 outline-dashed outline-[var(--glass-border-subtle)]",
        active && isOver && (isDark ? "bg-sky-400/15" : "bg-sky-50"),
      )}
    >
      {children}
    </div>
  );
}
