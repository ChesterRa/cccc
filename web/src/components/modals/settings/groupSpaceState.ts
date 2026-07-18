import type { GroupSpaceRemoteSpace } from "../../../types";

export function shouldRefreshNotebookSpaces(
  writeReady: boolean,
  refreshSpaces: boolean,
  currentSpacesCount: number,
): boolean {
  return writeReady && (refreshSpaces || currentSpacesCount <= 0);
}

export function resolveNotebookSpacesAfterLoad(
  currentSpaces: GroupSpaceRemoteSpace[],
  options: { writeReady: boolean; fetchedSpaces?: GroupSpaceRemoteSpace[] | null },
): GroupSpaceRemoteSpace[] {
  if (!options.writeReady) return [];
  if (Array.isArray(options.fetchedSpaces)) return options.fetchedSpaces;
  return currentSpaces;
}
