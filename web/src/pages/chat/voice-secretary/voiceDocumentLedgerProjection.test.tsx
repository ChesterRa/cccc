// @vitest-environment happy-dom

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../../../types";
import {
  type ClearVoiceDocumentReferences,
  useVoiceDocumentArchiveEventProjection,
} from "./voiceDocumentReferenceLifecycle";

function ProjectionProbe({
  groupId,
  events,
  clearReferences,
}: {
  groupId: string;
  events: LedgerEvent[];
  clearReferences: ClearVoiceDocumentReferences;
}) {
  useVoiceDocumentArchiveEventProjection({ groupId, events, clearReferences });
  return null;
}

describe("voice document archive event projection", () => {
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

  it("retains each group cursor and projects every archive in a merged tail", async () => {
    const clearReferences = vi.fn();
    const baseline = [
      { id: "a-1", kind: "assistant.voice.document", data: { action: "saved" } },
    ] as LedgerEvent[];

    await act(async () => {
      root.render(
        <ProjectionProbe groupId="group-a" events={baseline} clearReferences={clearReferences} />,
      );
    });
    await act(async () => {
      root.render(
        <ProjectionProbe
          groupId="group-b"
          events={[{ id: "b-1", kind: "assistant.voice.session" }]}
          clearReferences={clearReferences}
        />,
      );
    });
    expect(clearReferences).not.toHaveBeenCalled();

    const pythonArchive = {
      id: "a-2",
      kind: "assistant.voice.document",
      group_id: "group-a",
      data: {
        action: "archive",
        document_id: "doc-a",
        document_path: "docs/voice-secretary/archive/a.md",
        archived_from_document_path: "docs/voice-secretary/a.md",
      },
    } as LedgerEvent;
    const rustArchive = {
      id: "a-3",
      kind: "assistant.voice.document",
      group_id: "group-a",
      data: {
        action: "archived",
        document: {
          document_id: "doc-b",
          document_path: "docs/voice-secretary/b.md",
          status: "archived",
        },
      },
    } as LedgerEvent;

    await act(async () => {
      root.render(
        <ProjectionProbe
          groupId="group-a"
          events={[
            ...baseline,
            pythonArchive,
            rustArchive,
            { id: "a-4", kind: "assistant.voice.session" },
          ]}
          clearReferences={clearReferences}
        />,
      );
    });

    expect(clearReferences).toHaveBeenCalledTimes(2);
    expect(clearReferences).toHaveBeenNthCalledWith(
      1,
      "group-a",
      expect.objectContaining({ document_id: "doc-a", document_path: "docs/voice-secretary/a.md" }),
    );
    expect(clearReferences).toHaveBeenNthCalledWith(
      2,
      "group-a",
      expect.objectContaining({ document_id: "doc-b", document_path: "docs/voice-secretary/b.md" }),
    );
  });
});
