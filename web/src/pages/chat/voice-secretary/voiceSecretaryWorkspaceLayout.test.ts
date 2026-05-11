import { describe, expect, it } from "vitest";

import { getVoiceSecretaryWorkspaceVisibility } from "./voiceSecretaryWorkspaceLayout";

describe("voiceSecretaryWorkspaceLayout", () => {
  it("keeps prompt mode focused on request and activity content on small screens", () => {
    expect(getVoiceSecretaryWorkspaceVisibility({ captureMode: "prompt", isSmallScreen: true })).toEqual({
      showDocumentList: false,
      showWorkspace: false,
      showRequestPanel: true,
      showRequestCard: true,
      showActivityFeed: true,
    });
  });

  it("keeps ask mode focused on request and activity content on small screens", () => {
    expect(getVoiceSecretaryWorkspaceVisibility({ captureMode: "instruction", isSmallScreen: true })).toEqual({
      showDocumentList: false,
      showWorkspace: false,
      showRequestPanel: true,
      showRequestCard: true,
      showActivityFeed: true,
    });
  });

  it("keeps document mode focused on the document on small screens", () => {
    expect(getVoiceSecretaryWorkspaceVisibility({ captureMode: "document", isSmallScreen: true })).toEqual({
      showDocumentList: false,
      showWorkspace: true,
      showRequestPanel: false,
      showRequestCard: false,
      showActivityFeed: false,
    });
  });

  it("keeps the full three-panel workspace on larger screens", () => {
    expect(getVoiceSecretaryWorkspaceVisibility({ captureMode: "prompt", isSmallScreen: false })).toEqual({
      showDocumentList: true,
      showWorkspace: true,
      showRequestPanel: true,
      showRequestCard: true,
      showActivityFeed: true,
    });
  });
});
