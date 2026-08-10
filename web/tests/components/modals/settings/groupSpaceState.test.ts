import { describe, expect, it } from "vite-plus/test";

import {
  isProviderConnectionVerified,
  resolveNotebookSpacesAfterLoad,
  shouldRefreshNotebookSpaces,
} from "../../../../src/components/modals/settings/groupSpaceState";

describe("groupSpaceState", () => {
  const existingSpaces = [
    { remote_space_id: "nb-1", title: "Notebook 1", created_at: "", is_owner: true },
    { remote_space_id: "nb-2", title: "Notebook 2", created_at: "", is_owner: false },
  ];

  it("does not treat an unverified saved credential as connected", () => {
    expect(isProviderConnectionVerified(true, false)).toBe(false);
    expect(isProviderConnectionVerified(true, true)).toBe(true);
    expect(isProviderConnectionVerified(false, true)).toBe(false);
  });

  it("refreshes notebook spaces only when write-ready and refresh is requested or the list is empty", () => {
    expect(shouldRefreshNotebookSpaces(true, false, 2)).toBe(false);
    expect(shouldRefreshNotebookSpaces(true, true, 2)).toBe(true);
    expect(shouldRefreshNotebookSpaces(true, false, 0)).toBe(true);
    expect(shouldRefreshNotebookSpaces(false, true, 0)).toBe(false);
  });

  it("preserves existing spaces when a non-refresh load does not fetch a new list", () => {
    expect(
      resolveNotebookSpacesAfterLoad(existingSpaces, { writeReady: true, fetchedSpaces: null }),
    ).toBe(existingSpaces);
  });

  it("replaces spaces when a fresh list is fetched successfully", () => {
    const fetchedSpaces = [
      { remote_space_id: "nb-3", title: "Notebook 3", created_at: "", is_owner: true },
    ];

    expect(
      resolveNotebookSpacesAfterLoad(existingSpaces, { writeReady: true, fetchedSpaces }),
    ).toEqual(fetchedSpaces);
  });

  it("clears spaces when the provider is no longer write-ready", () => {
    expect(
      resolveNotebookSpacesAfterLoad(existingSpaces, { writeReady: false, fetchedSpaces: null }),
    ).toEqual([]);
  });
});
