import { describe, expect, it } from "vitest";

import { resolveRecipientActorsForComposer } from "./useCrossGroupRecipients";
import type { Actor } from "../types";

const currentActors: Actor[] = [
  { id: "old-actor", title: "Old Actor", role: "peer", runtime: "codex" },
];

describe("useCrossGroupRecipients helpers", () => {
  it("does not expose stale selected-group actors while composer group is unsettled", () => {
    expect(resolveRecipientActorsForComposer({
      actors: currentActors,
      remoteActorsByGroup: {},
      selectedGroupId: "new-group",
      composerGroupId: "old-group",
      sendGroupId: "new-group",
    })).toEqual([]);
  });
});
