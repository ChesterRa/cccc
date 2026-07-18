import { describe, expect, it } from "vite-plus/test";

import { buildComposerMentionSuggestions } from "./chatMentionSuggestions";

describe("buildComposerMentionSuggestions ordering", () => {
  it("places concrete actors before special mention targets by default", () => {
    const items = buildComposerMentionSuggestions({
      kind: "agent",
      filter: "",
      recipientActors: [
        { id: "peer-1", title: "Peer One", role: "peer" },
        { id: "foreman-1", title: "Foreman One", role: "foreman" },
      ],
      groups: [],
    });

    expect(items.map((item) => item.value)).toEqual([
      "peer-1",
      "foreman-1",
      "@all",
      "@foreman",
      "@peers",
    ]);
  });
});
