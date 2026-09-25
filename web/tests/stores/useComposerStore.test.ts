import { beforeEach, describe, expect, it } from "vite-plus/test";
import {
  getEffectiveComposerDestGroupId,
  isComposerGroupSettled,
  useComposerStore,
} from "../../src/stores/useComposerStore";

describe("getEffectiveComposerDestGroupId", () => {
  it("falls back to the selected group while composer state still belongs to the previous group", () => {
    expect(getEffectiveComposerDestGroupId("g-old", "g-old", "g-new")).toBe("g-new");
  });

  it("keeps an explicit cross-group destination once composer state has switched to the current group", () => {
    expect(getEffectiveComposerDestGroupId("g-remote", "g-current", "g-current")).toBe("g-remote");
  });

  it("defaults to the selected group when there is no explicit destination", () => {
    expect(getEffectiveComposerDestGroupId("", "g-current", "g-current")).toBe("g-current");
  });
});

describe("isComposerGroupSettled", () => {
  it("requires composer ownership to match the selected group", () => {
    expect(isComposerGroupSettled("g-current", "g-current")).toBe(true);
    expect(isComposerGroupSettled("g-old", "g-current")).toBe(false);
  });
});

describe("useComposerStore recipient memory", () => {
  beforeEach(() => {
    useComposerStore.setState({
      activeGroupId: "",
      composerText: "",
      composerFiles: [],
      toText: "",
      replyTarget: null,
      quotedPresentationRef: null,
      quotedVoiceDocumentRef: null,
      preferredMessageMode: "send",
      messageMode: "send",
      destGroupId: "",
      drafts: {},
      normalToTextByGroup: {},
    });
  });

  it("keeps explicit recipients across consecutive normal sends", () => {
    const store = useComposerStore.getState();
    store.switchGroup(null, "g-1");
    useComposerStore.getState().setToText("@foreman");
    useComposerStore.getState().setComposerText("hello");

    useComposerStore.getState().clearComposer();

    expect(useComposerStore.getState().composerText).toBe("");
    expect(useComposerStore.getState().toText).toBe("@foreman");
    expect(useComposerStore.getState().normalToTextByGroup).toEqual({ "g-1": "@foreman" });
    useComposerStore.getState().setComposerText("second message");
    useComposerStore.getState().clearComposer();
    expect(useComposerStore.getState().toText).toBe("@foreman");
    expect(useComposerStore.getState().replyTarget).toBe(null);
  });

  it("restores the normal recipient after a reply is canceled", () => {
    const store = useComposerStore.getState();
    store.switchGroup(null, "g-1");
    useComposerStore.getState().setToText("@foreman");
    useComposerStore.getState().setReplyToText("peer-1");
    useComposerStore.getState().setReplyTarget({ eventId: "e-1", by: "peer-1", text: "prior" });

    useComposerStore.getState().setReplyTarget(null);

    expect(useComposerStore.getState().toText).toBe("@foreman");
    expect(useComposerStore.getState().replyTarget).toBe(null);
  });

  it("restores each group's own selection with its unsent draft", () => {
    const store = useComposerStore.getState();
    store.switchGroup(null, "g-1");
    useComposerStore.getState().setToText("peer-1");
    useComposerStore.getState().setComposerText("draft for one");

    useComposerStore.getState().switchGroup("g-1", "g-2");
    expect(useComposerStore.getState().toText).toBe("");
    useComposerStore.getState().setToText("peer-2");
    useComposerStore.getState().switchGroup("g-2", "g-1");

    expect(useComposerStore.getState().toText).toBe("peer-1");
    expect(useComposerStore.getState().composerText).toBe("draft for one");
    useComposerStore.getState().clearComposer();
    useComposerStore.getState().switchGroup("g-1", "g-2");
    expect(useComposerStore.getState().toText).toBe("peer-2");
  });

  it("remembers selections even without message drafts, including an explicit clear", () => {
    useComposerStore.getState().switchGroup(null, "g-1");
    useComposerStore.getState().setToText("peer-1, peer-2");
    useComposerStore.getState().switchGroup("g-1", "g-2");
    useComposerStore.getState().switchGroup("g-2", "g-1");
    expect(useComposerStore.getState().toText).toBe("peer-1, peer-2");
    useComposerStore.getState().setToText("");
    useComposerStore.getState().clearComposer();
    useComposerStore.getState().switchGroup("g-1", "g-2");
    useComposerStore.getState().switchGroup("g-2", "g-1");
    expect(useComposerStore.getState().toText).toBe("");
  });

  it.each(["send", "cancel"])("restores normal recipients after reply %s", (action) => {
    useComposerStore.getState().switchGroup(null, "g-1");
    useComposerStore.getState().setToText("peer-normal");
    useComposerStore.getState().setReplyToText("peer-reply");
    useComposerStore.getState().setReplyTarget({ eventId: "e-1", by: "peer-reply", text: "prior" });
    useComposerStore.getState().setToText("peer-reply-edited");
    // Reply changes and another reply must not become the ordinary recipients.
    useComposerStore.getState().setReplyToText("peer-other-reply");
    useComposerStore
      .getState()
      .setReplyTarget({ eventId: "e-2", by: "peer-other-reply", text: "other" });
    if (action === "send") useComposerStore.getState().clearComposer();
    else useComposerStore.getState().setReplyTarget(null);
    expect(useComposerStore.getState().toText).toBe("peer-normal");
    expect(useComposerStore.getState().normalToTextByGroup["g-1"]).toBe("peer-normal");
  });

  it("preserves normal memory while a reply draft travels between groups", () => {
    useComposerStore.getState().switchGroup(null, "g-1");
    useComposerStore.getState().setToText("@foreman");
    useComposerStore.getState().setReplyToText("peer-reply");
    useComposerStore.getState().setReplyTarget({ eventId: "e-1", by: "peer-reply", text: "prior" });
    useComposerStore.getState().switchGroup("g-1", "g-2");
    useComposerStore.getState().setToText("@all");
    useComposerStore.getState().clearComposer();
    useComposerStore.getState().switchGroup("g-2", "g-1");
    expect(useComposerStore.getState().toText).toBe("peer-reply");
    useComposerStore.getState().clearComposer();
    expect(useComposerStore.getState().toText).toBe("@foreman");
  });

  it("never treats remote recipients as normal local recipients", () => {
    useComposerStore.getState().switchGroup(null, "g-1");
    useComposerStore.getState().setToText("local-peer");
    useComposerStore.getState().setDestGroupId("g-remote");
    expect(useComposerStore.getState().toText).toBe("");
    useComposerStore.getState().setToText("remote-peer");
    expect(useComposerStore.getState().normalToTextByGroup["g-1"]).toBe("local-peer");
    useComposerStore.getState().clearComposer();
    expect(useComposerStore.getState().destGroupId).toBe("g-1");
    expect(useComposerStore.getState().toText).toBe("local-peer");
  });

  it("restores local selection when returning from a temporary destination", () => {
    useComposerStore.getState().switchGroup(null, "g-1");
    useComposerStore.getState().setToText("local-peer");
    useComposerStore.getState().setDestGroupId("g-remote");
    useComposerStore.getState().setToText("same-id-in-both-groups");
    useComposerStore.getState().setDestGroupId("g-1");
    expect(useComposerStore.getState().toText).toBe("local-peer");
    useComposerStore.getState().setDestGroupId("g-1");
    expect(useComposerStore.getState().toText).toBe("local-peer");
  });

  it("cancels a cross-group reply back to local recipients and destination", () => {
    useComposerStore.getState().switchGroup(null, "g-1");
    useComposerStore.getState().setToText("local-peer");
    // Same order as the real reply action.
    useComposerStore.getState().setDestGroupId("g-remote");
    useComposerStore.getState().setReplyToText("remote-peer");
    useComposerStore
      .getState()
      .setReplyTarget({
        eventId: "e-1",
        by: "remote-peer",
        text: "prior",
        remoteDstGroupId: "g-remote",
        remoteDstTo: ["remote-peer"],
      });
    useComposerStore.getState().setReplyTarget(null);
    expect(useComposerStore.getState().destGroupId).toBe("g-1");
    expect(useComposerStore.getState().toText).toBe("local-peer");
  });

  it("ignores duplicate switches to the already active group", () => {
    const store = useComposerStore.getState();
    store.switchGroup(null, "g-a");
    useComposerStore.getState().setToText("@all");
    useComposerStore.getState().setComposerText("draft for a");

    useComposerStore.getState().switchGroup("g-a", "g-b");
    useComposerStore.getState().setComposerText("fresh text for b");

    useComposerStore.getState().switchGroup("g-a", "g-b");

    const state = useComposerStore.getState();
    expect(state.activeGroupId).toBe("g-b");
    expect(state.composerText).toBe("fresh text for b");
    expect(state.toText).toBe("");
    expect(state.drafts["g-a"]).toMatchObject({ composerText: "draft for a", toText: "@all" });
  });
});
