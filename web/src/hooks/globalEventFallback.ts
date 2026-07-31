export function refreshGlobalEventsFallback(
  documentHidden: boolean,
  refreshGroups: () => void,
  refreshActors: () => void,
): void {
  if (documentHidden) return;
  refreshGroups();
  refreshActors();
}
