// @vitest-environment happy-dom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type {
  LedgerEvent,
  PresentationMessageRef,
  TaskMessageRef,
  VoiceDocumentMessageRef,
} from "../../types";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: { defaultValue?: string }) => options?.defaultValue || key,
  }),
}));

import { MessageReferenceSections } from "./MessageReferenceSections";

describe("MessageReferenceSections", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("renders complete document paths and preserves interactive reference callbacks", async () => {
    const event: LedgerEvent = { id: "event-1", kind: "message" };
    const presentationRef: PresentationMessageRef = {
      kind: "presentation_ref",
      slot_id: "slot-1",
      label: "Quarterly review",
    };
    const documentPath = `docs/voice-secretary/${"nested/".repeat(20)}meeting-notes.md`;
    const voiceDocumentRef: VoiceDocumentMessageRef = {
      kind: "voice_document_ref",
      group_id: "group-a",
      document_id: "doc-a",
      document_path: documentPath,
      title: "Meeting notes",
    };
    const taskRef: TaskMessageRef = {
      kind: "task_ref",
      task_id: "task-1",
      title: "Publish release",
      status: "active",
    };
    const onOpenPresentationRef = vi.fn();
    const onOpenTaskRef = vi.fn();

    await act(async () => {
      root.render(
        <MessageReferenceSections
          event={event}
          presentationRefs={[presentationRef]}
          voiceDocumentRefs={[voiceDocumentRef]}
          taskRefs={[taskRef]}
          taskById={new Map()}
          sectionClassName="reference-section"
          onOpenPresentationRef={onOpenPresentationRef}
          onOpenTaskRef={onOpenTaskRef}
        />,
      );
    });

    const documentChip = Array.from(host.querySelectorAll("[title]")).find(
      (element) => element.getAttribute("title") === documentPath,
    );
    expect(documentChip?.textContent).toContain("Meeting notes");
    expect(documentChip?.getAttribute("title")).toBe(documentPath);

    const buttons = host.querySelectorAll("button");
    await act(async () => buttons[0]?.click());
    await act(async () => buttons[1]?.click());
    expect(onOpenPresentationRef).toHaveBeenCalledWith(presentationRef, event);
    expect(onOpenTaskRef).toHaveBeenCalledWith(taskRef, event);
  });
});
