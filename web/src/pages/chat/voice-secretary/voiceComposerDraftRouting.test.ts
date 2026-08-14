import { beforeEach, describe, expect, it } from "vite-plus/test";

import { useComposerStore } from "../../../stores/useComposerStore";
import {
  mergeVoiceComposerDraftText,
  routeVoiceTextToComposerGroup,
} from "./voiceComposerDraftRouting";

beforeEach(() => {
  useComposerStore.setState({
    activeGroupId: "new-group",
    composerText: "new group draft",
    drafts: {},
  });
});

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
});
