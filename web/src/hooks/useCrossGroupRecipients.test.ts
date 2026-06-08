import { describe, expect, it } from "vitest";

import { getRemoteActorsFetchDecision, resolveRecipientActorsForComposer } from "./useCrossGroupRecipients";
import type { Actor } from "../types";

const currentActors: Actor[] = [
  { id: "old-actor", title: "Old Actor", role: "peer", runtime: "codex" },
];

const remoteActors: Actor[] = [
  { id: "remote-actor", title: "Remote Actor", role: "peer", runtime: "codex" },
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

  it("exposes remote actors while the composer targets a different settled group", () => {
    expect(resolveRecipientActorsForComposer({
      actors: currentActors,
      remoteActorsByGroup: { "remote-group": remoteActors },
      selectedGroupId: "current-group",
      composerGroupId: "current-group",
      sendGroupId: "remote-group",
    })).toEqual(remoteActors);
  });

  it("refreshes cached remote actors after the dynamic refresh interval", () => {
    expect(getRemoteActorsFetchDecision({
      canFetchRemoteRecipients: true,
      hasCachedActors: true,
      fetchedAtMs: 1000,
      nowMs: 1000 + 30000,
    })).toEqual({ shouldFetch: false, noCache: false });

    expect(getRemoteActorsFetchDecision({
      canFetchRemoteRecipients: true,
      hasCachedActors: true,
      fetchedAtMs: 1000,
      nowMs: 1000 + 60000,
    })).toEqual({ shouldFetch: true, noCache: true });
  });
});
