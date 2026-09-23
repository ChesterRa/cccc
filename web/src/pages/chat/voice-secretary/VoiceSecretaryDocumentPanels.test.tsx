// @vitest-environment happy-dom

import type { TFunction } from "i18next";
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import type { AssistantVoiceDocument } from "../../../types";
import { VoiceSecretaryDocumentListPanel } from "./VoiceSecretaryDocumentListPanel";
import { VoiceSecretaryWorkspacePanel } from "./VoiceSecretaryWorkspacePanel";

const t = ((key: string, options?: Record<string, unknown>) => {
  const overrides: Record<string, string> = {
    voiceSecretaryMarkdownBadge: "TYPE_CHIP",
    voiceSecretaryRepoBackedBadge: "STORAGE_CHIP",
  };
  let value = overrides[key] || String(options?.defaultValue || key);
  for (const [name, replacement] of Object.entries(options || {})) {
    value = value.split(`{{${name}}}`).join(String(replacement));
  }
  return value;
}) as unknown as TFunction;

const documents: AssistantVoiceDocument[] = [
  {
    document_id: "doc-1",
    document_path: "docs/voice/primary.md",
    title: "Primary notes",
    status: "active",
    workspace_path: "docs/voice/primary.md",
  },
  {
    document_id: "doc-2",
    document_path: "docs/voice/follow-up.md",
    title: "Follow-up",
    status: "active",
    workspace_path: "docs/voice/follow-up.md",
  },
];

describe("Voice Secretary document panels", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("shows live original text only for the viewed document and replaces it after stop", async () => {
    const livePreview = {
      id: "voice-stream-test",
      mode: "document" as const,
      phase: "interim" as const,
      documentPath: "docs/voice/primary.md",
      text: "第一句原文\n正在说的第二句",
      updatedAt: 100,
    };
    await act(async () =>
      root.render(workspacePanel({ view: "transcript", recording: true, livePreview })),
    );
    expect(host.querySelector("[data-voice-live-transcript]")?.textContent).toContain(
      livePreview.text,
    );
    await act(async () =>
      root.render(
        workspacePanel({
          view: "transcript",
          recording: true,
          livePreview: { ...livePreview, text: "继续识别" },
        }),
      ),
    );
    expect(host.querySelector("[data-voice-live-transcript]")?.textContent).toContain("继续识别");
    expect(host.textContent).not.toContain("正在说的第二句");
    await act(async () =>
      root.render(
        workspacePanel({
          view: "transcript",
          recording: true,
          livePreview: { ...livePreview, documentPath: "other.md" },
        }),
      ),
    );
    expect(host.querySelector("[data-voice-live-transcript]")).toBeNull();
    await act(async () =>
      root.render(
        workspacePanel({
          view: "transcript",
          recording: false,
          livePreview,
          transcriptItems: [
            { ...livePreview, id: "saved", phase: "final", text: "最终原文", createdAt: 100 },
          ],
        }),
      ),
    );
    expect(host.querySelector("[data-voice-live-transcript]")).toBeNull();
    expect(host.textContent).toContain("最终原文");
  });

  it("glows the whole workspace while recording and sweeps a beam while analyzing", async () => {
    const glow = () => host.querySelector("[data-voice-workspace-glow]");
    await act(async () => root.render(workspacePanel({ recording: false })));
    expect(glow()?.querySelector("[data-voice-document-panel]")).toBeTruthy();
    expect(glow()?.hasAttribute("data-active")).toBe(false);
    await act(async () => root.render(workspacePanel({ recording: true })));
    expect(glow()?.hasAttribute("data-active")).toBe(true);
    expect(glow()?.hasAttribute("data-processing")).toBe(false);
    await act(async () =>
      root.render(
        workspacePanel({
          recording: false,
          view: "transcript",
          transcriptItems: [
            {
              id: "final",
              mode: "document",
              phase: "final",
              text: "",
              updatedAt: 1,
              createdAt: 1,
              processingPhase: "separating_speakers",
            },
          ],
        }),
      ),
    );
    expect(glow()?.hasAttribute("data-processing")).toBe(true);
  });

  it("marks the default document with a badge and sets a default from the row menu", async () => {
    const onSelectDocument = vi.fn();
    const onSetCaptureTargetDocument = vi.fn();

    await act(async () => {
      root.render(
        <VoiceSecretaryDocumentListPanel
          actionBusy=""
          activeDocumentPath="docs/voice/primary.md"
          captureTargetDocumentPath="docs/voice/primary.md"
          creatingDocument={false}
          documents={documents}
          isDark={false}
          newDocumentTitleDraft=""
          t={t}
          documentKey={(document) => document.document_id}
          documentPath={(document) => document.document_path || ""}
          onCancelCreateDocument={vi.fn()}
          onCreateDocument={vi.fn()}
          onNewDocumentTitleChange={vi.fn()}
          onSelectDocument={onSelectDocument}
          onSetCaptureTargetDocument={onSetCaptureTargetDocument}
          onStartCreateDocument={vi.fn()}
        />,
      );
    });

    expect(host.textContent).not.toContain("docs/voice/primary.md");
    const badges = host.querySelectorAll("[data-voice-document-default]");
    expect(badges).toHaveLength(1);
    expect(badges[0]?.closest('[role="button"]')?.textContent).toContain("Primary notes");
    expect(host.querySelector("[data-voice-document-target]")).toBeNull();

    const triggers = host.querySelectorAll<HTMLButtonElement>('button[aria-haspopup="menu"]');
    expect(triggers).toHaveLength(2);
    const menuItem = async (trigger: HTMLButtonElement, label: string) => {
      await act(async () => trigger.click());
      return [...document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')].find(
        (item) => item.textContent === label,
      );
    };

    const defaultItem = await menuItem(triggers[0]!, "Use by default");
    expect(defaultItem?.disabled).toBe(true);
    await act(async () => triggers[0]!.click());

    const availableItem = await menuItem(triggers[1]!, "Use by default");
    expect(availableItem?.disabled).toBe(false);
    await act(async () => availableItem?.click());
    expect(onSetCaptureTargetDocument).toHaveBeenCalledWith(documents[1]);
    expect(onSelectDocument).not.toHaveBeenCalled();
    expect(document.querySelector('[role="menu"]')).toBeNull();

    const documentRows = host.querySelectorAll<HTMLElement>('[role="button"]');
    expect(documentRows[0]?.tabIndex).toBe(0);
    await act(async () => {
      documentRows[0]?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }),
      );
    });
    expect(onSelectDocument).toHaveBeenCalledWith(documents[0]);
    await act(async () => {
      documentRows[1]?.dispatchEvent(
        new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true }),
      );
    });
    expect(onSelectDocument).toHaveBeenCalledWith(documents[1]);
  });

  it("keeps behavioral status and repo metadata while removing duplicate type chips", async () => {
    await act(async () => {
      root.render(workspacePanel());
    });

    expect(host.textContent).not.toContain("TYPE_CHIP");
    expect(host.textContent).not.toContain("STORAGE_CHIP");
    expect(host.textContent).toContain("Default document");
    expect(host.textContent).toContain("Repo markdown");
    expect(host.textContent).toContain("docs/voice/primary.md");

    const lightQuoteAction = Array.from(host.querySelectorAll("button")).find((button) =>
      button.textContent?.includes("Quote in chat"),
    );
    expect(lightQuoteAction?.querySelector(".lucide-message-square-quote")).toBeTruthy();

    await act(async () => {
      root.render(
        workspacePanel({
          activeDocumentPath: "",
          activeDocumentWritePath: "",
          captureTargetDocumentPath: "",
        }),
      );
    });
    expect(host.textContent).toContain("Waiting for transcript");
    expect(host.textContent).toContain("Auto-create on transcript");
  });
});

function workspacePanel(
  overrides: Partial<ComponentProps<typeof VoiceSecretaryWorkspacePanel>> = {},
) {
  return (
    <VoiceSecretaryWorkspacePanel
      activeDocumentPath="docs/voice/primary.md"
      activeDocumentWritePath="docs/voice/primary.md"
      actionBusy=""
      captureTargetDocumentPath="docs/voice/primary.md"
      documentDisplayTitle="Primary notes"
      documentDraft="# Notes"
      documentEditing={false}
      documentHasUnsavedEdits={false}
      documentLoading={false}
      documentRemoteChanged={false}
      isDark={false}
      recording={false}
      recordingAudioLevel={() => 0}
      t={t}
      transcriptItems={[]}
      view="document"
      onChangeView={vi.fn()}
      onArchiveDocument={vi.fn()}
      onClearTranscript={vi.fn()}
      onDownloadDocument={vi.fn()}
      onEditDocumentChange={vi.fn()}
      onLoadLatestDocument={vi.fn()}
      onQuoteDocument={vi.fn()}
      onSaveDocument={vi.fn()}
      onToggleDocumentEditing={vi.fn()}
      formatTime={(value) => String(value)}
      formatFullTime={(value) => String(value)}
      normalizeTranscriptText={(value) => value}
      {...overrides}
    />
  );
}
