export function shouldMountAppModals(input: {
  modals?: Record<string, boolean>;
  recipientsEventId?: string | null;
  presentationViewer?: unknown | null;
  presentationPin?: unknown | null;
  editingActor?: unknown | null;
}): boolean {
  if (input.editingActor || input.presentationViewer || input.presentationPin) return true;
  if (String(input.recipientsEventId || "").trim()) return true;
  return Object.values(input.modals || {}).some((value) => Boolean(value));
}
