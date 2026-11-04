import { beforeEach, describe, expect, it } from "vite-plus/test";

import { useComposerStore } from "../../../stores/useComposerStore";
import {
  createComposerAgentMentionToken,
  createComposerGroupMentionToken,
} from "../../../hooks/composerGroupMentions";
import { buildComposerConnectGroupRefs } from "../../../hooks/composerLocalGroupRouteRefs";
import { hashComposerSnapshot } from "./voiceComposerUtils";
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
  it("retains new typing when a late refinement tries to replace the active composer", () => {
    const store = useComposerStore.getState();
    const expectedSnapshotHash = hashComposerSnapshot(store.composerText);
    store.setComposerText("new typing that must survive");
    expect(
      routeVoiceTextToComposerGroup({
        groupId: "new-group",
        text: "late refinement",
        mode: "replace",
        expectedSnapshotHash,
      }),
    ).toBe("changed");
    expect(useComposerStore.getState().composerText).toBe("new typing that must survive");
  });

  it("checks the original Group draft after navigation instead of the visible composer", () => {
    const store = useComposerStore.getState();
    const expectedSnapshotHash = hashComposerSnapshot(store.composerText);
    store.setComposerText("edited original Group");
    store.switchGroup("new-group", "other-group");
    store.setComposerText("other Group typing");
    expect(
      routeVoiceTextToComposerGroup({
        groupId: "new-group",
        text: "late refinement",
        mode: "replace",
        expectedSnapshotHash,
      }),
    ).toBe("changed");
    expect(useComposerStore.getState().drafts["new-group"].composerText).toBe(
      "edited original Group",
    );
    expect(useComposerStore.getState().composerText).toBe("other Group typing");
  });

  it("automatically applies to an unchanged original Group after navigation", () => {
    const store = useComposerStore.getState();
    const expectedSnapshotHash = hashComposerSnapshot(store.composerText);
    store.switchGroup("new-group", "other-group");
    store.setComposerText("other Group typing");
    expect(
      routeVoiceTextToComposerGroup({
        groupId: "new-group",
        text: "refinement",
        mode: "replace",
        expectedSnapshotHash,
      }),
    ).toBe("draft");
    expect(useComposerStore.getState().drafts["new-group"].composerText).toBe("refinement");
    expect(useComposerStore.getState().composerText).toBe("other Group typing");
  });

  it("does not repopulate a composer that the user has sent or cleared", () => {
    const store = useComposerStore.getState();
    const expectedSnapshotHash = hashComposerSnapshot(store.composerText);
    store.clearComposer();
    expect(
      routeVoiceTextToComposerGroup({
        groupId: "new-group",
        text: "late refinement",
        mode: "replace",
        expectedSnapshotHash,
      }),
    ).toBe("changed");
    expect(useComposerStore.getState().composerText).toBe("");
  });

  it("appends speech to the active group", () => {
    expect(
      routeVoiceTextToComposerGroup({ groupId: "new-group", text: "voice", mode: "append" }),
    ).toBe("active");
    expect(useComposerStore.getState().composerText).toBe("new group draft voice");
  });

  it("preserves the visible composer and writes speech to the recording group draft", () => {
    expect(
      routeVoiceTextToComposerGroup({ groupId: "old-group", text: "voice", mode: "append" }),
    ).toBe("draft");
    expect(useComposerStore.getState().composerText).toBe("new group draft");
    expect(useComposerStore.getState().drafts["old-group"]?.composerText).toBe("voice");
  });

  it.each([
    { mode: "append" as const, text: "voice", expected: "original voice" },
    { mode: "replace" as const, text: "original", expected: "original" },
  ])(
    "preserves the background draft's destination but retires its retry identity on $mode",
    ({ mode, text, expected }) => {
      const attachment = new File(["attachment"], "example.txt");
      const replyTarget = {
        eventId: "event-1",
        by: "peer-1",
        text: "question",
        remoteDstGroupId: "destination",
        remoteDstTo: ["peer-2"],
      };
      useComposerStore.setState({ activeGroupId: "old-group", composerText: "" });
      const store = useComposerStore.getState();
      store.enqueueFailedSend({
        id: "send-1",
        originGroupId: "old-group",
        destGroupId: "destination",
        retryKey: "original-request",
        composerText: "original",
        composerFiles: [attachment],
        toText: "peer-2",
        replyTarget,
        quotedPresentationRef: null,
        quotedVoiceDocumentRef: null,
        messageMode: "send",
      });
      expect(store.restoreFailedSend("old-group", "send-1")).toBe(true);
      store.switchGroup("old-group", "new-group");
      expect(useComposerStore.getState().drafts["old-group"].retryCandidate?.id).toBe("send-1");

      expect(routeVoiceTextToComposerGroup({ groupId: "old-group", text, mode })).toBe("draft");
      const draft = useComposerStore.getState().drafts["old-group"];
      expect(draft.composerText).toBe(expected);
      expect(draft.destGroupId).toBe("destination");
      expect(draft.replyTarget).toEqual(replyTarget);
      expect(draft.toText).toBe("peer-2");
      expect(draft.composerFiles).toEqual([attachment]);
      expect(draft.composerFiles[0]).toBe(attachment);
      expect(draft.retryCandidate).toBeNull();
      expect(useComposerStore.getState().composerText).toBe("");

      store.switchGroup("new-group", "old-group");
      const restored = useComposerStore.getState();
      expect(restored.destGroupId).toBe("destination");
      expect(restored.retryCandidate).toBeNull();
    },
  );

  it("supports replacement without duplicating shared merge logic", () => {
    expect(mergeVoiceComposerDraftText("old", "new", "replace")).toBe("new");
  });

  it.each([
    ["好的。", "然后发布", "好的。然后发布"],
    ["先跑测试", "再提交", "先跑测试再提交"],
    ["了解です", "次に進みます", "了解です次に進みます"],
    ["Run the tests", "then commit", "Run the tests then commit"],
    ["发布 CCCC", "v0.4.48", "发布 CCCC v0.4.48"],
    ["draft  ", "voice", "draft  voice"],
    ["- item\t", "voice", "- item\tvoice"],
    ["first line\n", "voice", "first line\nvoice"],
    ["Hello", ", world", "Hello, world"],
    ["  ", "voice", "  voice"],
    ["\n", "voice", "\nvoice"],
    ["", "voice", "voice"],
    ["Please check:", "1. totals\n2. coupons", "Please check:\n1. totals\n2. coupons"],
    ["Notes\n", "1. totals\n2. coupons", "Notes\n1. totals\n2. coupons"],
    ["Notes\n  ", "1. totals\n2. coupons", "Notes\n  1. totals\n2. coupons"],
  ])("appends %j and %j", (current, transcript, expected) => {
    expect(mergeVoiceComposerDraftText(current, transcript, "append")).toBe(expected);
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
    // The user ended the draft with a line break, so the speech starts on that line.
    expect(restored.composerText).toBe(`${mentionedText}  \nvoice`);
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
