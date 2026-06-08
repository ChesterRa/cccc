import { describe, expect, it } from "vitest";

import {
  buildComposerMentionSuggestions,
  getComposerGroupRouteDestination,
  hasComposerGroupRouteToken,
} from "./chatMentionSuggestions";

describe("buildComposerMentionSuggestions", () => {
  it("builds current-route agent suggestions for @ mentions", () => {
    const items = buildComposerMentionSuggestions({
      kind: "agent",
      filter: "peer",
      recipientActors: [{ id: "peer-1", title: "Peer One", role: "peer" }],
      groups: [],
    });

    expect(items.map((item) => item.value)).toEqual(["@peers", "peer-1"]);
  });

  it("builds group suggestions with description and id metadata for # mentions", () => {
    const items = buildComposerMentionSuggestions({
      kind: "group",
      filter: "sdk",
      recipientActors: [],
      groups: [{ group_id: "g_sdk", title: "cccc-sdk", topic: "SDK integration" }],
    });

    expect(items).toEqual([
      expect.objectContaining({
        kind: "group",
        value: "g_sdk",
        label: "cccc-sdk",
        description: "SDK integration",
        meta: "g_sdk",
      }),
    ]);
  });
});

describe("hasComposerGroupRouteToken", () => {
  it("keeps a destination group active only while its # route token remains", () => {
    const groups = [{ group_id: "g_sdk", title: "cccc-sdk", topic: "SDK integration" }];

    expect(hasComposerGroupRouteToken({
      text: "#cccc-sdk @foreman ",
      destGroupId: "g_sdk",
      selectedGroupId: "g_current",
      groups,
    })).toBe(true);
    expect(hasComposerGroupRouteToken({
      text: "@foreman ",
      destGroupId: "g_sdk",
      selectedGroupId: "g_current",
      groups,
    })).toBe(false);
  });

  it("does not treat partial issue-style hashes as group route tokens", () => {
    expect(hasComposerGroupRouteToken({
      text: "参考 #g_sdk-issue @foreman",
      destGroupId: "g_sdk",
      selectedGroupId: "g_current",
      groups: [{ group_id: "g_sdk", title: "cccc-sdk" }],
    })).toBe(false);
  });
});

describe("getComposerGroupRouteDestination", () => {
  const groups = [
    { group_id: "g_first", title: "first-group" },
    { group_id: "g_second", title: "second-group" },
  ];

  it("uses the last complete #group route token as the destination", () => {
    expect(getComposerGroupRouteDestination({
      text: "#first-group @foreman #second-group @peer ",
      selectedGroupId: "g_current",
      groups,
    })).toBe("g_second");
  });

  it("returns to the previous #group route when the later route is removed", () => {
    expect(getComposerGroupRouteDestination({
      text: "#first-group @foreman ",
      selectedGroupId: "g_current",
      groups,
    })).toBe("g_first");
  });

  it("falls back to the selected group when no complete #group route remains", () => {
    expect(getComposerGroupRouteDestination({
      text: "@foreman ",
      selectedGroupId: "g_current",
      groups,
    })).toBe("g_current");
  });
});
