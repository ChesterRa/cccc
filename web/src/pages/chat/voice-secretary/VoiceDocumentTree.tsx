import { useState } from "react";
import {
  DndContext,
  DragOverlay,
  MouseSensor,
  TouchSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import { SortableContext, arrayMove, verticalListSortingStrategy } from "@dnd-kit/sortable";
import { FileText, Folder } from "lucide-react";
import { getSidebarSensorActivationConstraints } from "../../../components/layout/groupSidebarModel";
import type { VoiceFolder } from "../../../services/api/voiceDocumentLibrary";
import type { AssistantVoiceDocument } from "../../../types";
import { classNames } from "../../../utils/classNames";
import type { VoiceDocumentRowContext } from "./VoiceDocumentRow";
import { VoiceDocumentListEmpty } from "./VoiceSecretaryDocumentListPanel";
import {
  DraggableDocumentRow,
  RootSlot,
  SortableFolder,
  RootDropZone,
} from "./VoiceDocumentTreeParts";
import {
  collisionDetection,
  dropTarget,
  folderRootKey,
  type DragItem,
  type RootItem,
} from "./voiceDocumentTreeModel";

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
