import { describe, expect, it } from "vitest";

import type { Actor } from "../../types";
import { resolveRuntimeInspectorActor } from "./appShellRuntimeActors";

describe("runtime inspector actor stability", () => {
  it("keeps the last mounted actor when a transient refresh omits it", () => {
    const currentActors: Actor[] = [];
    const mountedActorsById: Record<string, Actor> = {
      "claude-1": { id: "claude-1", role: "peer", runtime: "claude", runner: "pty" },
    };

    expect(resolveRuntimeInspectorActor("claude-1", currentActors, mountedActorsById)).toBe(mountedActorsById["claude-1"]);
  });

  it("does not fall back to a mounted actor from another group", () => {
    const currentActors: Actor[] = [];
    const mountedActorsById: Record<string, Actor> = {};

    expect(resolveRuntimeInspectorActor("peer-1", currentActors, mountedActorsById)).toBeNull();
  });
});
