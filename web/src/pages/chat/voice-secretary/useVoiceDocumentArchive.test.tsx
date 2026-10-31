// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import type { TFunction } from "i18next";
import { useVoiceDocumentArchive } from "./useVoiceDocumentArchive";
import { deleteVoiceAssistantDocument } from "../../../services/api/voiceDocumentDelete";
vi.mock("../../../services/api/voiceDocumentDelete", () => ({
  deleteVoiceAssistantDocument: vi.fn(),
}));
const target = {
  document_id: "target",
  document_path: "voice/target.md",
  title: "目标文档",
  status: "active",
};
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it.each(["success", "failure", "cancel"])(
  "deletion %s preserves the document unless the API succeeds",
  async (outcome) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.stubGlobal("confirm", vi.fn().mockReturnValue(outcome !== "cancel"));
    vi.mocked(deleteVoiceAssistantDocument)
      .mockReset()
      .mockResolvedValue(
        outcome === "failure"
          ? { ok: false, error: { code: "io_error", message: "delete failed" } }
          : { ok: true, result: { group_id: "group", document: { ...target, status: "deleted" } } },
      );
    const clearReferences = vi.fn(),
      setDocuments = vi.fn(),
      setViewedDocumentPath = vi.fn(),
      showError = vi.fn(),
      refreshAssistant = vi.fn().mockResolvedValue(undefined);
    const captureTargetDocumentPathRef = { current: target.document_path };
    let remove: ReturnType<typeof useVoiceDocumentArchive>;
    function Harness() {
      remove = useVoiceDocumentArchive({
        selectedGroupId: "group",
        activeDocumentWritePath: target.document_path,
        viewedDocumentPath: target.document_path,
        documents: [target],
        archivedDocumentPathsRef: { current: new Set() },
        captureTargetDocumentPathRef,
        isCurrentGroup: () => true,
        getDraftSnapshot: () => ({
          path: target.document_path,
          session: 1,
          content: "",
          dirty: false,
        }),
        preserveUnavailableDraft: vi.fn(),
        loadDocumentDraft: vi.fn(),
        clearReferences,
        refreshAssistant,
        setActionBusy: vi.fn(),
        setDocuments,
        setViewedDocumentPath,
        setDocumentEditing: vi.fn(),
        setCaptureTargetDocumentPath: vi.fn(),
        showError,
        showNotice: vi.fn(),
        t: ((_key: string, options: { defaultValue: string }) => options.defaultValue) as TFunction,
      });
      return null;
    }
    const host = document.createElement("div");
    const root = createRoot(host);
    try {
      await act(async () => root.render(<Harness />));
      await act(async () => {
        await remove(target, true);
      });
      if (outcome === "cancel") expect(deleteVoiceAssistantDocument).not.toHaveBeenCalled();
      else
        expect(deleteVoiceAssistantDocument).toHaveBeenCalledWith("group", target.document_path, {
          by: "user",
        });
      if (outcome === "success") {
        expect(clearReferences).toHaveBeenCalledWith("group", target);
        expect(
          setDocuments.mock.calls[0][0]([target, { ...target, document_path: "other.md" }]),
        ).toEqual([expect.objectContaining({ document_path: "other.md" })]);
        expect(setViewedDocumentPath).toHaveBeenCalledWith("");
        expect(captureTargetDocumentPathRef.current).toBe("");
        expect(refreshAssistant).toHaveBeenCalled();
      } else {
        expect(setDocuments).not.toHaveBeenCalled();
        expect(clearReferences).not.toHaveBeenCalled();
        expect(captureTargetDocumentPathRef.current).toBe(target.document_path);
        if (outcome === "failure") expect(showError).toHaveBeenCalledWith("delete failed");
      }
    } finally {
      await act(async () => root.unmount());
    }
  },
);

it.each(["new draft", "new session"])(
  "does not discard %s when an archive response arrives late",
  async (kind) => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    const confirm = vi.fn().mockReturnValue(true);
    vi.stubGlobal("confirm", confirm);
    let finish!: (value: Awaited<ReturnType<typeof deleteVoiceAssistantDocument>>) => void;
    vi.mocked(deleteVoiceAssistantDocument).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    let snapshot = { path: target.document_path, session: 1, content: "first draft", dirty: true };
    const preserve = vi.fn(),
      load = vi.fn(),
      setPath = vi.fn();
    let remove: ReturnType<typeof useVoiceDocumentArchive>;
    function Harness() {
      remove = useVoiceDocumentArchive({
        selectedGroupId: "group",
        activeDocumentWritePath: target.document_path,
        viewedDocumentPath: target.document_path,
        documents: [target],
        archivedDocumentPathsRef: { current: new Set() },
        captureTargetDocumentPathRef: { current: "" },
        isCurrentGroup: () => true,
        getDraftSnapshot: () => snapshot,
        preserveUnavailableDraft: preserve,
        loadDocumentDraft: load,
        clearReferences: vi.fn(),
        refreshAssistant: vi.fn().mockResolvedValue(undefined),
        setActionBusy: vi.fn(),
        setDocuments: vi.fn(),
        setViewedDocumentPath: setPath,
        setDocumentEditing: vi.fn(),
        setCaptureTargetDocumentPath: vi.fn(),
        showError: vi.fn(),
        showNotice: vi.fn(),
        t: ((_key: string, options: { defaultValue: string }) => options.defaultValue) as TFunction,
      });
      return null;
    }
    const host = document.createElement("div");
    const root = createRoot(host);
    try {
      await act(async () => root.render(<Harness />));
      let pending!: Promise<void>;
      await act(async () => {
        pending = remove(target, true);
      });
      expect(confirm.mock.calls[0][0]).toContain("Unsaved edits");
      snapshot =
        kind === "new draft"
          ? { ...snapshot, content: "later draft" }
          : { ...snapshot, session: 2, path: "voice/other.md", content: "other draft" };
      await act(async () => {
        finish({
          ok: true,
          result: { group_id: "group", document: { ...target, status: "deleted" } },
        });
        await pending;
      });
      expect(load).not.toHaveBeenCalled();
      expect(setPath).not.toHaveBeenCalled();
      if (kind === "new draft") expect(preserve).toHaveBeenCalledWith(target, target.document_path);
      else expect(preserve).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
    }
  },
);
