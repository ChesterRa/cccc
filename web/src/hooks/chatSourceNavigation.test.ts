import { describe, expect, it } from "vitest";

import { canOpenSourceMessageLocally } from "./chatSourceNavigation";
import type { GroupMeta } from "../types";

describe("canOpenSourceMessageLocally", () => {
  it("allows source navigation only for local groups", () => {
    const groups: GroupMeta[] = [
      { group_id: "g_local", title: "Local" },
      { group_id: "g_remote", title: "Remote", federation_remote: true },
    ];

    expect(canOpenSourceMessageLocally(groups, "g_local")).toBe(true);
    expect(canOpenSourceMessageLocally(groups, "g_remote")).toBe(false);
    expect(canOpenSourceMessageLocally(groups, "g_missing")).toBe(false);
  });
});
