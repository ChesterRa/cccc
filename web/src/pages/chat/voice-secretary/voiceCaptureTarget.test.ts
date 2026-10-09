import { describe, expect, it } from "vite-plus/test";
import { resolveVoiceCaptureTarget, voicePanelViewForComposerMode } from "./voiceCaptureTarget";

const base = {
  recordingTarget: null,
  panelOpen: false,
  panelView: "document" as const,
  composerMode: "prompt" as const,
  promptAutoRefine: false,
  viewedDocumentPath: "notes/viewed.md",
  defaultDocumentPath: "notes/default.md",
};

describe("voice capture target", () => {
  it("uses the composer mode and the default document while the workspace is closed", () => {
    expect(resolveVoiceCaptureTarget({ ...base, composerMode: "document" })).toEqual({
      mode: "document",
      dispatchTarget: "document",
      documentPath: "notes/default.md",
    });
    expect(resolveVoiceCaptureTarget(base).dispatchTarget).toBe("composer");
  });

  it("records into the viewed document from the open document view, whatever the composer mode", () => {
    expect(resolveVoiceCaptureTarget({ ...base, panelOpen: true, composerMode: "prompt" })).toEqual(
      { mode: "document", documentPath: "notes/viewed.md", dispatchTarget: "document" },
    );
    expect(
      resolveVoiceCaptureTarget({ ...base, panelOpen: true, viewedDocumentPath: "" }).documentPath,
    ).toBe("notes/default.md");
  });

  it("records a question from the open ask view", () => {
    expect(resolveVoiceCaptureTarget({ ...base, panelOpen: true, panelView: "ask" }).mode).toBe(
      "instruction",
    );
  });

  it("keeps the started target while the user browses other views or closes the workspace", () => {
    const recordingTarget = {
      mode: "document" as const,
      documentPath: "notes/viewed.md",
      dispatchTarget: "document" as const,
    };
    for (const browsing of [
      { panelOpen: false, panelView: "document" as const },
      { panelOpen: true, panelView: "ask" as const },
      { panelOpen: true, panelView: "document" as const, viewedDocumentPath: "notes/other.md" },
    ]) {
      expect(resolveVoiceCaptureTarget({ ...base, ...browsing, recordingTarget })).toBe(
        recordingTarget,
      );
    }
  });

  it("opens the workspace on the view matching a document or ask composer mode", () => {
    expect(voicePanelViewForComposerMode("document", "ask")).toBe("document");
    expect(voicePanelViewForComposerMode("instruction", "document")).toBe("ask");
    expect(voicePanelViewForComposerMode("prompt", "ask")).toBe("ask");
  });

  it.each([false, true])(
    "keeps the original Prompt route when another Group has different preferences (auto=%s)",
    (promptAutoRefine) => {
      const recordingTarget = resolveVoiceCaptureTarget({ ...base, promptAutoRefine });
      const next = resolveVoiceCaptureTarget({
        ...base,
        recordingTarget,
        promptAutoRefine: !promptAutoRefine,
        panelOpen: true,
        panelView: "ask",
      });
      expect(next).toBe(recordingTarget);
      expect(next.dispatchTarget).toBe(promptAutoRefine ? "prompt" : "composer");
    },
  );
});
