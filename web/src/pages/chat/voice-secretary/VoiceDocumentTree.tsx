import { useState, type ReactNode } from "react";
import {
  DndContext,
  DragOverlay,
  MouseSensor,
  TouchSensor,
  closestCenter,
  pointerWithin,
  useDraggable,
  useDroppable,
  useSensor,
  useSensors,
  type CollisionDetection,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { ChevronRight, FileText, Folder, FolderOpen, Pencil, X } from "lucide-react";
import { IconButton } from "../../../components/ui/icon-button";
import { getSidebarSensorActivationConstraints } from "../../../components/layout/groupSidebarModel";
import type { VoiceFolder } from "../../../services/api/voiceDocumentLibrary";
import type { AssistantVoiceDocument } from "../../../types";
import { classNames } from "../../../utils/classNames";
import { VoiceDocumentRow, type VoiceDocumentRowContext } from "./VoiceDocumentRow";
import { VoiceDocumentListEmpty } from "./VoiceSecretaryDocumentListPanel";

const ROOT_DROP_ID = "voice-folder-root";
const folderRootKey = (folderId: string) => `folder:${folderId}`;

/** What a drag carries: a document to file, or a folder to reorder among root items. */
type DragItem = { document: AssistantVoiceDocument } | { folder: VoiceFolder; rootKey: string };
/** Where it lands: a folder or the root to file into, and/or a root slot to reorder against. */
type DropTarget = { folderId?: string; rootKey?: string };
type RootItem =
  | { key: string; folder: VoiceFolder }
  | { key: string; document: AssistantVoiceDocument };

const dropTarget = (container: { data: { current?: unknown } } | undefined) =>
  container?.data.current as DropTarget | undefined;

// Folders reorder against the nearest root item. Documents file into the folder under the
// pointer, else the root; root document slots only serve folder reordering.
const collisionDetection: CollisionDetection = (args) => {
  if (args.active.data.current?.folder)
    return closestCenter({
      ...args,
      droppableContainers: args.droppableContainers.filter((c) => dropTarget(c)?.rootKey),
    });
  const hits = pointerWithin(args).filter(
    (hit) => dropTarget(hit.data?.droppableContainer)?.folderId !== undefined,
  );
  return [...hits].sort(
    (a, b) =>
      Number(!!dropTarget(b.data?.droppableContainer)?.folderId) -
      Number(!!dropTarget(a.data?.droppableContainer)?.folderId),
  );
};

type Props = {
  ctx: VoiceDocumentRowContext;
  documents: AssistantVoiceDocument[];
  folders: VoiceFolder[];
  /** Saved mixed order of root items; unsaved (new) items come first in natural order. */
  rootOrder: string[];
  /** The folder a document lives in; "" means unfiled. */
  folderOf: (document: AssistantVoiceDocument) => string;
  busy: boolean;
  onDropDocument: (document: AssistantVoiceDocument, folderId: string) => Promise<boolean>;
  onReorderRoot: (rootOrder: string[]) => Promise<boolean>;
  onRenameFolder: (folder: VoiceFolder) => void;
  onRemoveFolder: (folder: VoiceFolder) => void;
};

/** Folders and documents as one tree: documents drag into folders, folders drag anywhere. */
export function VoiceDocumentTree(props: Props) {
  const { ctx, documents, folders, folderOf, busy } = props;
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());
  const [dragging, setDragging] = useState<DragItem | null>(null);
  // Optimistic order while a move is saving, so the dropped folder does not snap back.
  const [pendingOrder, setPendingOrder] = useState<string[] | null>(null);
  const activation = getSidebarSensorActivationConstraints();
  // Touch drags wait for a long-press so a swipe still scrolls the list.
  const sensors = useSensors(
    useSensor(MouseSensor, { activationConstraint: activation.mouse }),
    useSensor(TouchSensor, { activationConstraint: activation.touch }),
  );
  const byFolder = new Map<string, AssistantVoiceDocument[]>();
  for (const document of documents) {
    const key = folderOf(document);
    byFolder.set(key, [...(byFolder.get(key) || []), document]);
  }
  const documentRootKey = (document: AssistantVoiceDocument) =>
    `document:${ctx.documentPath(document) || ctx.documentKey(document)}`;
  const rank = new Map((pendingOrder || props.rootOrder).map((key, index) => [key, index]));
  const rootItems: RootItem[] = [
    ...folders.map((folder) => ({ key: folderRootKey(folder.folder_id), folder })),
    ...(byFolder.get("") || []).map((document) => ({ key: documentRootKey(document), document })),
  ].sort((a, b) => (rank.get(a.key) ?? -1) - (rank.get(b.key) ?? -1));
  const draggedDocument = dragging && "document" in dragging ? dragging.document : null;
  const setOpen = (folderId: string, open: boolean) =>
    setExpanded((current) => {
      if (current.has(folderId) === open) return current;
      const next = new Set(current);
      if (open) next.add(folderId);
      else next.delete(folderId);
      return next;
    });
  const moveFolder = async (rootKey: string, targetKey: string) => {
    const keys = rootItems.map((item) => item.key);
    const from = keys.indexOf(rootKey);
    const to = keys.indexOf(targetKey);
    if (from < 0 || to < 0 || from === to) return;
    const next = arrayMove(keys, from, to);
    setPendingOrder(next);
    await props.onReorderRoot(next);
    setPendingOrder(null);
  };
  const onDragEnd = async ({ active, over }: DragEndEvent) => {
    setDragging(null);
    const item = active.data.current as DragItem | undefined;
    const target = dropTarget(over ?? undefined);
    if (!item || !target) return;
    if ("folder" in item) {
      if (target.rootKey) await moveFolder(item.rootKey, target.rootKey);
      return;
    }
    const folderId = target.folderId;
    if (folderId === undefined || folderId === folderOf(item.document)) return;
    if ((await props.onDropDocument(item.document, folderId)) && folderId) setOpen(folderId, true);
  };
  const row = (document: AssistantVoiceDocument, depth: number) => (
    <DraggableDocumentRow
      key={ctx.documentKey(document) || document.title}
      ctx={ctx}
      document={document}
      depth={depth}
      disabled={busy}
    />
  );
  return (
    <DndContext
      sensors={sensors}
      collisionDetection={collisionDetection}
      onDragStart={({ active }) => setDragging((active.data.current as DragItem) || null)}
      onDragCancel={() => setDragging(null)}
      onDragEnd={(event) => void onDragEnd(event)}
    >
      <RootDropZone
        isDark={ctx.isDark}
        active={!!draggedDocument && folderOf(draggedDocument) !== ""}
      >
        <SortableContext
          items={rootItems.map((item) => item.key)}
          strategy={verticalListSortingStrategy}
        >
          {rootItems.map((item) => {
            if ("document" in item)
              return (
                <RootSlot key={item.key} rootKey={item.key}>
                  {row(item.document, 0)}
                </RootSlot>
              );
            const { folder } = item;
            const open = expanded.has(folder.folder_id);
            const children = byFolder.get(folder.folder_id) || [];
            return (
              <SortableFolder
                key={item.key}
                ctx={ctx}
                folder={folder}
                rootKey={item.key}
                count={children.length}
                open={open}
                busy={busy}
                acceptsDocument={
                  !!draggedDocument && folderOf(draggedDocument) !== folder.folder_id
                }
                onToggle={() => setOpen(folder.folder_id, !open)}
                onRename={() => props.onRenameFolder(folder)}
                onRemove={() => props.onRemoveFolder(folder)}
              >
                {open ? (
                  children.length ? (
                    children.map((document) => row(document, 1))
                  ) : (
                    <div className="py-1.5 pl-10 text-xs text-[var(--color-text-muted)]">
                      {ctx.t("voiceFolderEmpty", { defaultValue: "No documents in this folder" })}
                    </div>
                  )
                ) : null}
              </SortableFolder>
            );
          })}
        </SortableContext>
        {!documents.length ? <VoiceDocumentListEmpty t={ctx.t} /> : null}
      </RootDropZone>
      <DragOverlay dropAnimation={null}>
        {dragging ? (
          <div
            className={classNames(
              "flex max-w-56 items-center gap-1.5 rounded-lg border px-2.5 py-1.5 text-sm font-medium shadow-lg",
              ctx.isDark
                ? "border-white/15 bg-[rgb(40,40,44)] text-white"
                : "border-black/10 bg-white text-gray-900",
            )}
          >
            {"folder" in dragging ? (
              <Folder size={15} aria-hidden="true" className="shrink-0 opacity-70" />
            ) : (
              <FileText size={15} aria-hidden="true" className="shrink-0 opacity-60" />
            )}
            <span className="truncate">
              {"folder" in dragging
                ? dragging.folder.name
                : dragging.document.title || ctx.documentKey(dragging.document)}
            </span>
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}

function DraggableDocumentRow({
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
function RootSlot({ rootKey, children }: { rootKey: string; children: ReactNode }) {
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
function SortableFolder({
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

function RootDropZone({
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
