import { describe, expect, it } from "vite-plus/test";

import { getVoiceSecretaryWorkspaceVisibility } from "./voiceSecretaryWorkspaceLayout";

describe("voiceSecretaryWorkspaceLayout", () => {
  it.each(["prompt", "instruction"] as const)(
    "reveals a linked document without changing %s capture mode",
    (captureMode) => {
      for (const isSmallScreen of [true, false]) {
        const visibility = getVoiceSecretaryWorkspaceVisibility({
          captureMode,
          isSmallScreen,
          documentRevealed: true,
        });
        expect(visibility.showWorkspace).toBe(true);
        expect(visibility.showRequestPanel).toBe(!isSmallScreen);
        expect(
          getVoiceSecretaryWorkspaceVisibility({ captureMode, isSmallScreen }).showActivityFeed,
        ).toBe(true);
      }
    },
  );
  it.each(["prompt", "instruction"] as const)(
    "keeps desktop %s focused on the current request and results",
    (captureMode) => {
      expect(getVoiceSecretaryWorkspaceVisibility({ captureMode, isSmallScreen: false })).toEqual({
        showDocumentList: false,
        showWorkspace: false,
        showRequestPanel: true,
        showRequestCard: true,
        showActivityFeed: true,
      });
    },
  );
  it("keeps prompt mode focused on request and activity content on small screens", () => {
    expect(
      getVoiceSecretaryWorkspaceVisibility({ captureMode: "prompt", isSmallScreen: true }),
    ).toEqual({
      showDocumentList: false,
      showWorkspace: false,
      showRequestPanel: true,
      showRequestCard: true,
      showActivityFeed: true,
    });
  });

  it("keeps ask mode focused on request and activity content on small screens", () => {
    expect(
      getVoiceSecretaryWorkspaceVisibility({ captureMode: "instruction", isSmallScreen: true }),
    ).toEqual({
      showDocumentList: false,
      showWorkspace: false,
      showRequestPanel: true,
      showRequestCard: true,
      showActivityFeed: true,
    });
  });

  it("keeps document mode focused on the document on small screens", () => {
    expect(
      getVoiceSecretaryWorkspaceVisibility({ captureMode: "document", isSmallScreen: true }),
    ).toEqual({
      showDocumentList: false,
      showWorkspace: true,
      showRequestPanel: false,
      showRequestCard: false,
      showActivityFeed: false,
    });
  });

  it("keeps document tools available beside the document on larger screens", () => {
    expect(
      getVoiceSecretaryWorkspaceVisibility({ captureMode: "document", isSmallScreen: false }),
    ).toEqual({
      showDocumentList: true,
      showWorkspace: true,
      showRequestPanel: true,
      showRequestCard: true,
      showActivityFeed: true,
    });
  });
});
