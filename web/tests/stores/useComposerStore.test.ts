import { beforeEach, describe, expect, it } from "vitest";
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
      priority: "normal",
      replyRequired: false,
      destGroupId: "",
      drafts: {},
      normalToTextByGroup: {},
    });
  });

  it("keeps the current group's normal recipient after clearing a sent composer", () => {
    const store = useComposerStore.getState();
    store.switchGroup(null, "g-1");
    useComposerStore.getState().setToText("@foreman");
    useComposerStore.getState().setComposerText("hello");

    useComposerStore.getState().clearComposer();

    expect(useComposerStore.getState().composerText).toBe("");
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

  it("does not carry recipient memory across groups", () => {
    const store = useComposerStore.getState();
    store.switchGroup(null, "g-1");
    useComposerStore.getState().setToText("peer-1");

    useComposerStore.getState().switchGroup("g-1", "g-2");

    expect(useComposerStore.getState().toText).toBe("");
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
    expect(state.drafts["g-a"]).toMatchObject({
      composerText: "draft for a",
      toText: "@all",
    });
  });
});
