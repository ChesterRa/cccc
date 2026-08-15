import { describe, expect, it } from "vite-plus/test";

import { getComposerDestGroupDisplayValue, useComposerStore } from "./useComposerStore";

describe("useComposerStore helpers", () => {
  it("shows the selected group while composer state is still switching groups", () => {
    expect(getComposerDestGroupDisplayValue("old-group", "new-group", false)).toBe("new-group");
  });
});

describe("useComposerStore clearComposer", () => {
  it("returns cross-group routing to the active group", () => {
    useComposerStore.setState({
      activeGroupId: "current-group",
      composerText: "hello",
      destGroupId: "target-group",
    });

    useComposerStore.getState().clearComposer();

    expect(useComposerStore.getState().destGroupId).toBe("current-group");
  });

  it("clears a quoted voice document after a successful send reset", () => {
    useComposerStore.setState({
      activeGroupId: "current-group",
      quotedVoiceDocumentRef: {
        kind: "voice_document_ref",
        group_id: "current-group",
        document_path: "voice/notes.md",
        title: "Notes",
      },
    });

    useComposerStore.getState().clearComposer();

    expect(useComposerStore.getState().quotedVoiceDocumentRef).toBeNull();
  });
});

describe("useComposerStore voice document drafts", () => {
  it("keeps quoted documents scoped to their original group", () => {
    useComposerStore.setState({
      activeGroupId: "group-a",
      composerText: "Review this",
      quotedVoiceDocumentRef: {
        kind: "voice_document_ref",
        group_id: "group-a",
        document_path: "voice/a.md",
        title: "A",
      },
      drafts: {},
    });

    useComposerStore.getState().switchGroup("group-a", "group-b");
    expect(useComposerStore.getState().quotedVoiceDocumentRef).toBeNull();

    useComposerStore.getState().switchGroup("group-b", "group-a");
    expect(useComposerStore.getState().quotedVoiceDocumentRef?.document_path).toBe("voice/a.md");
  });
});
