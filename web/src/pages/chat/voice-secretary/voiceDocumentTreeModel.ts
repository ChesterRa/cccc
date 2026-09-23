import { closestCenter, pointerWithin, type CollisionDetection } from "@dnd-kit/core";
import type { VoiceFolder } from "../../../services/api/voiceDocumentLibrary";
import type { AssistantVoiceDocument } from "../../../types";

export const ROOT_DROP_ID = "voice-folder-root";
export const folderRootKey = (folderId: string) => `folder:${folderId}`;

/** What a drag carries: a document to file, or a folder to reorder among root items. */
export type DragItem =
  | { document: AssistantVoiceDocument }
  | { folder: VoiceFolder; rootKey: string };
/** Where it lands: a folder or the root to file into, and/or a root slot to reorder against. */
export type DropTarget = { folderId?: string; rootKey?: string };
export type RootItem =
  | { key: string; folder: VoiceFolder }
  | { key: string; document: AssistantVoiceDocument };

export const dropTarget = (container: { data: { current?: unknown } } | undefined) =>
  container?.data.current as DropTarget | undefined;

// Folders reorder against the nearest root item. Documents file into the folder under the
// pointer, else the root; root document slots only serve folder reordering.
export const collisionDetection: CollisionDetection = (args) => {
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
