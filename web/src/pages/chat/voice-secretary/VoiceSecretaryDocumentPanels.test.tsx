// @vitest-environment happy-dom

import type { TFunction } from "i18next";
import { act, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import type { AssistantVoiceDocument } from "../../../types";
import { VoiceSecretaryDocumentListPanel } from "./VoiceSecretaryDocumentListPanel";
import { VoiceSecretaryDocumentTargetButton } from "./VoiceSecretaryDocumentTargetButton";
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

  it("uses one container and one state glyph without losing interaction", async () => {
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
          documentsCountLabel="2 docs"
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

    const selectedAction = host.querySelector<HTMLButtonElement>('button[aria-pressed="true"]');
    const availableAction = host.querySelector<HTMLButtonElement>('button[aria-pressed="false"]');
    expect(selectedAction?.disabled).toBe(false);
    expect(selectedAction?.dataset.state).toBe("default");
    expect(availableAction?.dataset.state).toBe("available");
    expect(selectedAction?.querySelectorAll("svg")).toHaveLength(1);
    expect(availableAction?.querySelectorAll("svg")).toHaveLength(1);
    expect(selectedAction?.querySelector(".lucide-file-check")).toBeTruthy();
    expect(availableAction?.querySelector(".lucide-file-text")).toBeTruthy();
    expect(selectedAction?.querySelector("[data-document-surface]")).toBeNull();
    expect(selectedAction?.querySelector("[data-default-indicator]")).toBeNull();
    expect(selectedAction?.className).toContain("bg-[rgb(35,36,37)]");
    expect(selectedAction?.getAttribute("aria-label")).toContain("Primary notes");
    expect(selectedAction?.getAttribute("title")).toBeNull();
    expect(availableAction?.getAttribute("aria-label")).toContain("Follow-up");
    expect(availableAction?.getAttribute("title")).toBeNull();
    expect(selectedAction?.className).toContain("focus-visible:ring-2");

    await act(async () => selectedAction?.click());
    expect(onSetCaptureTargetDocument).not.toHaveBeenCalled();

    await act(async () => availableAction?.click());
    expect(onSetCaptureTargetDocument).toHaveBeenCalledWith(documents[1]);
    expect(onSelectDocument).not.toHaveBeenCalled();

    const documentRows = host.querySelectorAll<HTMLElement>('[role="button"]');
    expect(documentRows[0]?.tabIndex).toBe(0);
    expect(documentRows[0]?.className).toContain("focus-visible:ring-2");
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

  it("keeps the single-glyph control legible in dark and disabled states", async () => {
    const onActivate = vi.fn();
    await act(async () => {
      root.render(
        <VoiceSecretaryDocumentTargetButton
          disabled={false}
          isDark
          label="Primary notes is the default document"
          selected
          onActivate={onActivate}
        />,
      );
    });

    const darkDefault = host.querySelector<HTMLButtonElement>("[data-voice-document-target]");
    expect(darkDefault?.className).toContain("bg-white/[0.13]");
    expect(darkDefault?.getAttribute("aria-label")).toBe("Primary notes is the default document");
    expect(darkDefault?.getAttribute("title")).toBeNull();
    expect(darkDefault?.querySelectorAll("svg")).toHaveLength(1);
    expect(darkDefault?.querySelector(".lucide-file-check")).toBeTruthy();

    await act(async () => {
      root.render(
        <VoiceSecretaryDocumentTargetButton
          disabled
          isDark={false}
          label="Document has no repository path"
          selected={false}
          onActivate={onActivate}
        />,
      );
    });
    const disabled = host.querySelector<HTMLButtonElement>("[data-voice-document-target]");
    expect(disabled?.disabled).toBe(true);
    expect(disabled?.dataset.state).toBe("available");
    expect(disabled?.className).toContain("disabled:opacity-40");
    await act(async () => disabled?.click());
    expect(onActivate).not.toHaveBeenCalled();
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
    const lightArchiveAction = Array.from(host.querySelectorAll("button")).find((button) =>
      button.textContent?.includes("Archive"),
    );
    expect(lightQuoteAction?.className).toBe(lightArchiveAction?.className);
    expect(lightQuoteAction?.className).toContain("border-black/10");
    expect(lightQuoteAction?.className).not.toContain("violet");
    expect(lightQuoteAction?.querySelector(".lucide-message-square-quote")).toBeTruthy();

    await act(async () => {
      root.render(workspacePanel({ isDark: true }));
    });
    const darkQuoteAction = Array.from(host.querySelectorAll("button")).find((button) =>
      button.textContent?.includes("Quote in chat"),
    );
    expect(darkQuoteAction?.className).toContain("border-white/10");
    expect(darkQuoteAction?.className).not.toContain("violet");

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
      recordingAudioLevels={[]}
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
