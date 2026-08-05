export function refreshGlobalEventsFallback(
  documentHidden: boolean,
  refreshGroups: () => void,
): void {
  if (documentHidden) return;
  refreshGroups();
}
