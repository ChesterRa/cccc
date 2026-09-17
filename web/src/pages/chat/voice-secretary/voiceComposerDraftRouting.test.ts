import { beforeEach, describe, expect, it } from "vite-plus/test";

import { useComposerStore } from "../../../stores/useComposerStore";
import {
  createComposerAgentMentionToken,
  createComposerGroupMentionToken,
} from "../../../hooks/composerGroupMentions";
import { buildComposerConnectGroupRefs } from "../../../hooks/composerLocalGroupRouteRefs";
import {
  mergeVoiceComposerDraftText,
  routeVoiceTextToComposerGroup,
} from "./voiceComposerDraftRouting";

beforeEach(() => {
  useComposerStore.setState({
    ...useComposerStore.getInitialState(),
    activeGroupId: "new-group",
    composerText: "new group draft",
    drafts: {},
  });
});

const groupToken = createComposerGroupMentionToken({
  groupId: "g_remote",
  token: "#Team · Mac",
  start: 0,
  remote: {
    instance_id: "i_mac",
    instance_name: "Mac",
    group_id: "g_remote",
    title: "Team",
    actors: [{ id: "worker", title: "Worker", enabled: true }],
    fresh: true,
  },
})!;
const agentToken = createComposerAgentMentionToken({
  actorId: "worker",
  token: "@Worker",
  start: groupToken.end + 1,
  scope: "destination",
})!;
const mentionedText = `${groupToken.token} ${agentToken.token}`;

describe("voice composer draft routing", () => {
  it("appends speech to the active group", () => {
    expect(
      routeVoiceTextToComposerGroup({ groupId: "new-group", text: "voice", mode: "append" }),
    ).toBe("active");
    expect(useComposerStore.getState().composerText).toBe("new group draft\n\nvoice");
  });

  it("preserves the visible composer and writes speech to the recording group draft", () => {
    expect(
      routeVoiceTextToComposerGroup({ groupId: "old-group", text: "voice", mode: "append" }),
    ).toBe("draft");
    expect(useComposerStore.getState().composerText).toBe("new group draft");
    expect(useComposerStore.getState().drafts["old-group"]?.composerText).toBe("voice");
  });

  it("supports replacement without duplicating shared merge logic", () => {
    expect(mergeVoiceComposerDraftText("old", "new", "replace")).toBe("new");
  });

  it("preserves selected remote identities when speech finishes after switching groups", () => {
    const store = useComposerStore.getState();
    useComposerStore.setState({
      activeGroupId: "old-group",
      composerText: `${mentionedText}  \n`,
      composerGroupMentionTokens: [groupToken],
      composerAgentMentionTokens: [agentToken],
    });
    store.switchGroup("old-group", "new-group");
    store.setComposerText("visible draft");

    expect(
      routeVoiceTextToComposerGroup({ groupId: "old-group", text: "voice", mode: "append" }),
    ).toBe("draft");
    expect(useComposerStore.getState().composerText).toBe("visible draft");
    expect(useComposerStore.getState().composerGroupMentionTokens).toEqual([]);
    expect(useComposerStore.getState().composerAgentMentionTokens).toEqual([]);

    store.switchGroup("new-group", "old-group");
    const restored = useComposerStore.getState();
    expect(restored.composerText).toBe(`${mentionedText}\n\nvoice`);
    expect(restored.composerGroupMentionTokens).toEqual([groupToken]);
    expect(restored.composerAgentMentionTokens).toEqual([agentToken]);
    expect(
      buildComposerConnectGroupRefs(restored.composerText, restored.composerGroupMentionTokens),
    ).toEqual([
      {
        kind: "connect_group_ref",
        instance_id: "i_mac",
        instance_name: "Mac",
        group_id: "g_remote",
        group_title: "Team",
        token: groupToken.token,
      },
    ]);
  });

  it.each([
    {
      mode: "append" as const,
      draftText: mentionedText.replace("Mac", "Other").replace("Worker", "Other"),
    },
    { mode: "replace" as const, draftText: mentionedText },
  ])("discards invalidated references during $mode", ({ mode, draftText }) => {
    const store = useComposerStore.getState();
    useComposerStore.setState({
      activeGroupId: "old-group",
      composerText: draftText,
      composerGroupMentionTokens: [groupToken],
      composerAgentMentionTokens: [agentToken],
    });
    store.switchGroup("old-group", "new-group");
    routeVoiceTextToComposerGroup({ groupId: "old-group", text: "voice", mode });
    store.switchGroup("new-group", "old-group");
    const restored = useComposerStore.getState();
    expect(restored.composerGroupMentionTokens).toEqual([]);
    expect(restored.composerAgentMentionTokens).toEqual([]);
    expect(
      buildComposerConnectGroupRefs(restored.composerText, restored.composerGroupMentionTokens),
    ).toEqual([]);
  });

  it("does not infer a remote identity from dictated mention text", () => {
    routeVoiceTextToComposerGroup({ groupId: "old-group", text: mentionedText, mode: "append" });
    useComposerStore.getState().switchGroup("new-group", "old-group");
    const restored = useComposerStore.getState();
    expect(restored.composerText).toBe(mentionedText);
    expect(restored.composerGroupMentionTokens).toEqual([]);
    expect(restored.composerAgentMentionTokens).toEqual([]);
    expect(
      buildComposerConnectGroupRefs(restored.composerText, restored.composerGroupMentionTokens),
    ).toEqual([]);
  });
});
