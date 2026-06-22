import { describe, expect, it } from "vitest";

import {
  buildComposerMentionSuggestions,
  buildFederationRouteGroups,
  getComposerGroupMentionInsertToken,
  extractSegmentTargetActor,
  getComposerGroupRouteDestination,
  hasComposerGroupRouteToken,
  resolveComposerHashRouting,
  resolveComposerMentionContext,
} from "./chatMentionSuggestions";

describe("resolveComposerMentionContext", () => {
  const groups = [
    { group_id: "self-agent", title: "Self Agent" },
    { group_id: "g_other", title: "Other" },
  ] as unknown as Parameters<typeof resolveComposerMentionContext>[0]["groups"];

  const ctx = (text: string) => {
    const atIndex = text.lastIndexOf("@");
    return resolveComposerMentionContext({ text, atIndex, selectedGroupId: "g_local", groups });
  };

  it("bare @ at start → selected/local", () => {
    expect(ctx("@")).toEqual({ scope: "selected", mentionTargetGroupId: "" });
  });

  it("@ after plain text → selected/local", () => {
    expect(ctx("hello there @")).toEqual({ scope: "selected", mentionTargetGroupId: "" });
  });

  it("#self-agent @ (valid group, same segment) → destination/target group", () => {
    expect(ctx("ask #self-agent to help @")).toEqual({ scope: "destination", mentionTargetGroupId: "self-agent" });
  });

  it("invalid #not-a-group @ → selected/local", () => {
    expect(ctx("#not-a-group @")).toEqual({ scope: "selected", mentionTargetGroupId: "" });
  });

  it("a # on a previous line does not pollute @ on the next line", () => {
    expect(ctx("#self-agent first line\nsecond line @")).toEqual({ scope: "selected", mentionTargetGroupId: "" });
  });
});

describe("extractSegmentTargetActor", () => {
  const groups = [{ group_id: "self-agent", title: "Self Agent" }] as unknown as Parameters<
    typeof extractSegmentTargetActor
  >[0]["groups"];

  it("extracts @target after a valid #group in the same segment", () => {
    expect(extractSegmentTargetActor({ text: "#self-agent some text @target-agent", selectedGroupId: "g_local", groups })).toBe(
      "target-agent",
    );
  });

  it("returns empty for a bare @ with no #group", () => {
    expect(extractSegmentTargetActor({ text: "@local-peer hello", selectedGroupId: "g_local", groups })).toBe("");
  });

  it("returns empty when @ is on a different line from the #group", () => {
    expect(extractSegmentTargetActor({ text: "#self-agent line one\n@local-peer", selectedGroupId: "g_local", groups })).toBe("");
  });
});

describe("resolveComposerHashRouting", () => {
  const groups = [
    { group_id: "self-agent", title: "Self Agent" },
    { group_id: "g_other", title: "Other" },
  ] as unknown as Parameters<typeof resolveComposerHashRouting>[0]["groups"];

  it("never sets a cross-group destination even when # matches a real group", () => {
    const routing = resolveComposerHashRouting({
      text: "please contact #self-agent about this",
      selectedGroupId: "g_local",
      groups,
    });
    // Destination stays local: the message is delivered to the local group's
    // agent, never sent directly to the referenced group.
    expect(routing.destGroupId).toBe("g_local");
    expect(routing.destGroupId).not.toBe("self-agent");
  });

  it("surfaces the referenced group as delegation context", () => {
    const routing = resolveComposerHashRouting({
      text: "ping #self-agent",
      selectedGroupId: "g_local",
      groups,
    });
    expect(routing.delegationGroupId).toBe("self-agent");
  });

  it("has no delegation target when the # token matches no real group", () => {
    const routing = resolveComposerHashRouting({
      text: "hello #nobody",
      selectedGroupId: "g_local",
      groups,
    });
    expect(routing.destGroupId).toBe("g_local");
    expect(routing.delegationGroupId).toBe("");
  });
});

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

  it("builds group suggestions for trusted remote federation groups", () => {
    const remoteGroups = buildFederationRouteGroups([
      {
        trust_id: "ptrust_1",
        status: "active",
        group_id: "g_owner",
        remote_group_id: "g_remote",
        remote_group_title: "Remote Product",
        remote_peer_id: "peer_remote",
        remote_endpoint: "https://remote.example",
        access_level: "full",
      },
    ]);

    expect(remoteGroups[0]?.federation_access_level).toBeUndefined();

    const items = buildComposerMentionSuggestions({
      kind: "group",
      filter: "remote",
      recipientActors: [],
      groups: remoteGroups,
    });

    expect(items).toEqual([
      expect.objectContaining({
        kind: "group",
        value: "g_remote",
        label: "Remote Product",
        description: "g_remote",
        meta: "peer_remote",
      }),
    ]);
  });

  it("builds group suggestions for active federation session routes", () => {
    const remoteGroups = buildFederationRouteGroups([
      {
        trust_id: "ptrust_9263feebd4920ad3",
        request_id: "preq_1",
        registration_id: "reg_81761c7e6e309356",
        status: "active",
        transport: "federation_session",
        group_id: "g_owner",
        remote_group_id: "g_0fb5f39478cc",
        remote_group_title: "CCCC Cross Test",
        remote_peer_id: "peer_00e780d5eb7bad9dea41bba479a9c292",
        remote_endpoint: "http://127.0.0.1:8858",
      },
    ]);

    const items = buildComposerMentionSuggestions({
      kind: "group",
      filter: "CCCC",
      recipientActors: [],
      groups: remoteGroups,
    });

    expect(remoteGroups).toHaveLength(1);
    expect(items).toEqual([
      expect.objectContaining({
        kind: "group",
        value: "g_0fb5f39478cc",
        label: "CCCC Cross Test",
        description: "g_0fb5f39478cc",
        meta: "peer_00e780d5eb7bad9dea41bba479a9c292",
      }),
    ]);
  });

  it("excludes revoked federation routes from # suggestions", () => {
    const remoteGroups = buildFederationRouteGroups([
      {
        trust_id: "ptrust_revoked",
        status: "revoked",
        group_id: "g_owner",
        remote_group_id: "g_0fb5f39478cc",
        remote_group_title: "CCCC Cross Test",
        remote_peer_id: "peer_00e780d5eb7bad9dea41bba479a9c292",
        remote_endpoint: "http://127.0.0.1:8858",
      },
    ]);

    const items = buildComposerMentionSuggestions({
      kind: "group",
      filter: "CCCC Cross Test",
      recipientActors: [],
      groups: remoteGroups,
    });

    expect(remoteGroups).toEqual([]);
    expect(items).toEqual([]);
  });

  it("does not use opaque remote group ids as the primary # suggestion label", () => {
    const remoteGroups = buildFederationRouteGroups([
      {
        trust_id: "ptrust_1",
        status: "active",
        remote_group_id: "g_0fb5f39478cc",
        remote_peer_id: "peer_00e780d5eb7bad9dea41bba479a9c292",
        remote_endpoint: "https://remote.example",
      },
    ]);

    const items = buildComposerMentionSuggestions({
      kind: "group",
      filter: "remote",
      recipientActors: [],
      groups: remoteGroups,
    });

    expect(items[0]).toEqual(expect.objectContaining({
      label: "remote.example",
      description: "g_0fb5f39478cc",
      meta: "peer_00e780d5eb7bad9dea41bba479a9c292",
    }));
  });

  it("inserts the readable remote group label for federation # suggestions", () => {
    const [remoteGroup] = buildFederationRouteGroups([
      {
        trust_id: "ptrust_1",
        status: "active",
        remote_group_id: "g_0fb5f39478cc",
        remote_group_title: "Remote Product",
        remote_peer_id: "peer_00e780d5eb7bad9dea41bba479a9c292",
      },
    ]);
    const [item] = buildComposerMentionSuggestions({
      kind: "group",
      filter: "remote",
      recipientActors: [],
      groups: [remoteGroup],
    });

    expect(item.label).toBe("Remote Product");
    expect(item.value).toBe("g_0fb5f39478cc");
    expect(getComposerGroupMentionInsertToken(item)).toBe("#Remote Product");
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
