import type { PresentationMessageRef } from "../types";

/** Where the panel sits: over the messages, or in the right-hand column. */
export type ResourceDock = "left" | "right";

export type WorkspaceResource = {
  kind: "workspace";
  scopeKey: string;
  /** A scope bound to another root is another workspace. */
  scopeUrl: string;
  /** Server-resolved, so a symlink and its target share one tab. */
  path: string;
};

export type AttachmentResource = {
  kind: "attachment";
  /** The client id for an own message, otherwise its event id. */
  messageKey: string;
  /** Empty until the ledger confirms the message. */
  eventId: string;
  index: number;
  /** The blob's owner can differ from the panel's Group. */
  blobGroupId: string;
  blobPath: string;
  /** Only until confirmation; never persisted. */
  localPreviewUrl?: string;
  sender?: string;
  sentAt?: string;
  media: "image" | "video" | "markdown" | "text" | "pdf";
  isSvg: boolean;
};

/** Closing a view never clears the shared Presentation slot. */
export type PresentationResource = {
  kind: "presentation";
  slotId: string;
  slotIndex: number;
  /** "live", or `${eventId}:${index in the filtered Presentation references}`. */
  refKey: string;
  focusEventId?: string;
  /** In memory only; restored quotes fetch it with current permissions. */
  focusRef?: PresentationMessageRef | null;
};

export type ResourceDescriptor = WorkspaceResource | AttachmentResource | PresentationResource;
export type ResourceTab = ResourceDescriptor & { key: string; title: string };
export type AttachmentUpdate = Partial<
  Pick<AttachmentResource, "eventId" | "blobGroupId" | "blobPath" | "localPreviewUrl" | "sentAt">
>;
export type ClosedEntry = { id: number; tabs: ResourceTab[] };
/** Two editor sides; the selected resource belongs to the focused side. */
export type ResourceSplit = { left: string; right: string; focus: "left" | "right"; ratio: number };
export type GroupPanel = {
  tabs: ResourceTab[];
  activeKey: string;
  open: boolean;
  maximized: boolean;
  closed: ClosedEntry[];
  split: ResourceSplit | null;
};

export function resourceKey(resource: ResourceDescriptor): string {
  if (resource.kind === "workspace")
    return `workspace:${resource.scopeKey}:${resource.scopeUrl}:${resource.path}`;
  if (resource.kind === "presentation") return `presentation:${resource.slotId}:${resource.refKey}`;
  return `attachment:${resource.messageKey}:${resource.index}`;
}
