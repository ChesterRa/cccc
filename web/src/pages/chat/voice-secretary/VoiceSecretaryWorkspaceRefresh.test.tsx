import { createHash } from "node:crypto";
// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { VoiceSecretaryComposerControl } from "../VoiceSecretaryComposerControl";
import { useGroupStore, useModalStore, useUIStore } from "../../../stores";
import { usePendingPageWork } from "../../../stores/usePendingPageWork";
import type { AssistantStateResult, AssistantVoiceDocument, LedgerEvent } from "../../../types";

const mocks = vi.hoisted(() => ({
  workspace: vi.fn(),
  status: vi.fn(),
  save: vi.fn(),
  instruction: vi.fn(),
  document: vi.fn(),
  t: (key: string, values?: { defaultValue?: string }) => values?.defaultValue || key,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.t }) }));
vi.mock("../../../services/api", async (original) => ({
  ...(await original<typeof import("../../../services/api")>()),
  fetchVoiceAssistantWorkspace: mocks.workspace,
  fetchVoiceAssistantStatus: mocks.status,
  saveVoiceAssistantDocument: mocks.save,
  sendVoiceAssistantDocumentInstruction: mocks.instruction,
  fetchVoiceAssistantDocumentContent: mocks.document,
  fetchVoiceAssistantMeetingSession: vi.fn(async () => ({ ok: true, result: {} })),
  fetchLatestVoiceAssistantMeetingSession: vi.fn(async () => ({ ok: true, result: {} })),
}));
vi.mock("./VoiceDocumentLibrary", () => ({
  VoiceDocumentLibrary: ({
    documents,
    onSelectDocument,
  }: {
    documents: AssistantVoiceDocument[];
    onSelectDocument: (document: AssistantVoiceDocument) => void;
  }) =>
    documents.map((document) => (
      <button
        key={document.document_id}
        data-select-document={document.document_path}
        onClick={() => onSelectDocument(document)}
      />
    )),
}));
vi.mock("./SecretaryTasks", () => ({ SecretaryTasks: () => null }));
vi.mock("./VoiceSecretaryWorkspacePanel", () => ({
  VoiceSecretaryWorkspacePanel: ({
    documentDraft,
    activeDocumentWritePath,
    onEditDocumentChange,
    onSaveDocument,
    onLoadLatestDocument,
    onOverwriteDocument,
    documentConflict,
    documentRemoteChanged,
    documentHasUnsavedEdits,
    documentEditing,
    onToggleDocumentEditing,
    footer,
  }: {
    documentDraft: string;
    activeDocumentWritePath: string;
    onEditDocumentChange: (value: string) => void;
    onSaveDocument: () => void;
    onLoadLatestDocument: () => void;
    onOverwriteDocument: () => void;
    documentConflict: string | null;
    documentRemoteChanged: boolean;
    documentHasUnsavedEdits: boolean;
    documentEditing: boolean;
    onToggleDocumentEditing: () => void;
    footer: React.ReactNode;
  }) => (
    <>
      <output
        data-workspace-document={activeDocumentWritePath}
        data-remote-changed={documentRemoteChanged}
        data-dirty={documentHasUnsavedEdits}
        data-editing={documentEditing}
      >
        {documentDraft}
      </output>
      <button onClick={() => onEditDocumentChange("saved edit")} data-edit-document />
      <button onClick={() => onEditDocumentChange("second edit")} data-edit-document-later />
      <button onClick={() => onEditDocumentChange("original")} data-reset-document />
      <button onClick={onSaveDocument} data-save-document />
      <button onClick={onToggleDocumentEditing} data-toggle-editor />
      <button onClick={onLoadLatestDocument} data-load-document />
      <button onClick={onOverwriteDocument} data-overwrite-document />
      <output data-save-conflict>{documentConflict}</output>
      {footer}
    </>
  ),
}));

function workspace(group: string, text: string): AssistantStateResult {
  return {
    group_id: group,
    assistant: {
      assistant_id: "voice_secretary",
      kind: "voice_secretary",
      enabled: true,
      lifecycle: "idle",
      config: { recognition_backend: "browser_asr" },
    },
    documents: [
      {
        document_id: `${group}-document`,
        document_path: `${group}.md`,
        workspace_path: `${group}.md`,
        title: `${group} notes`,
        content: text,
        content_sha256: createHash("sha256").update(text).digest("hex"),
        content_target_id: createHash("sha256").update(`${group}.md`).digest("hex"),
        status: "active",
      },
    ],
  };
}

describe("Voice Secretary workspace refresh ownership", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const render = (group = "A", initiallyOpen = true) =>
    act(async () =>
      root.render(
        <VoiceSecretaryComposerControl
          selectedGroupId={group}
          isDark={false}
          busy=""
          initiallyOpen={initiallyOpen}
        />,
      ),
    );
  const documentText = () => document.querySelector("[data-workspace-document]")?.textContent;
  const advance = (ms: number) => act(async () => vi.advanceTimersByTimeAsync(ms));
  const click = (selector: string) =>
    act(async () => document.querySelector<HTMLButtonElement>(selector)!.click());
  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.useFakeTimers();
    vi.clearAllMocks();
    vi.stubGlobal(
      "confirm",
      vi.fn(() => true),
    );
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        disconnect() {}
      },
    );
    useUIStore.setState({ canAccessGlobalSettings: true });
    useGroupStore.setState({ chatByGroup: {} });
    useModalStore.setState((state) => ({ modals: { ...state.modals, settings: false } }));
    mocks.status.mockResolvedValue({ ok: true, result: {} });
    mocks.save.mockImplementation(
      async (group: string, payload: { documentPath: string; content: string }) => ({
        ok: true,
        result: {
          document: {
            ...workspace(group, payload.content).documents![0],
            document_path: payload.documentPath,
            workspace_path: payload.documentPath,
            content_target_id: createHash("sha256").update(payload.documentPath).digest("hex"),
          },
        },
      }),
    );
    mocks.document.mockResolvedValue({ ok: true, result: {} });
    mocks.workspace.mockImplementation((group: string) =>
      Promise.resolve({ ok: true, result: workspace(group, "original") }),
    );
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("keeps saved configuration visible when its owner is unavailable and opens execution details", async () => {
    const value = workspace("A", "original");
    value.assistant!.health = {
      secretary: {
        configured: true,
        ready: false,
        readiness_code: "owner_unavailable",
        readiness_error: "Task fixture-record: invalid JSON at line 2 column 4",
      },
    };
    mocks.workspace.mockResolvedValue({ ok: true, result: value });
    await render();
    const banner = document.querySelector("[data-voice-readiness]");
    expect(banner?.textContent).toContain("settings:voiceSettings.readiness.ownerUnavailable");
    expect(banner?.textContent).toContain("fixture-record");
    expect(banner?.textContent).not.toContain("voiceSecretaryNotConfigured");
    await click("[data-voice-readiness-action]");
    expect(document.querySelector('[data-voice-body-mode="execution"]')).not.toBeNull();
    expect(useModalStore.getState().modals.settings).toBe(false);
    expect(mocks.save).not.toHaveBeenCalled();
    await click("[data-voice-workstage-toggle]");
    expect(documentText()).toBe("original");
  });

  it("routes an invalid saved Profile to Secretary settings without claiming it is unconfigured", async () => {
    const value = workspace("A", "original");
    value.assistant!.health = {
      secretary: {
        configured: true,
        ready: false,
        readiness_code: "invalid_configuration",
        readiness_error: "Profile could not be resolved",
      },
    };
    mocks.workspace.mockResolvedValue({ ok: true, result: value });
    await render();
    expect(document.querySelector("[data-voice-readiness]")?.textContent).toContain(
      "settings:voiceSettings.readiness.invalidConfiguration",
    );
    await click("[data-voice-readiness-action]");
    expect(useModalStore.getState().settingsTarget).toMatchObject({
      scope: "global",
      tab: "voice",
      voiceSection: "secretary",
    });
    expect(mocks.save).not.toHaveBeenCalled();
  });

  it("uses the unconfigured code instead of comparing an English diagnostic", async () => {
    const value = workspace("A", "original");
    value.assistant!.health = {
      secretary: {
        configured: false,
        ready: false,
        readiness_code: "not_configured",
        readiness_error: "Different diagnostic wording",
      },
    };
    mocks.workspace.mockResolvedValue({ ok: true, result: value });
    await render();
    const banner = document.querySelector("[data-voice-readiness]");
    expect(banner?.textContent).toContain("settings:voiceSettings.readiness.notConfigured");
    expect(banner?.textContent).not.toContain("Different diagnostic wording");
  });

  it("continues displaying successful slow refreshes while focus events keep arriving", async () => {
    await render();
    expect(documentText()).toBe("original");
    mocks.workspace.mockClear();
    mocks.workspace.mockImplementation(
      (group: string) =>
        new Promise((resolve) =>
          setTimeout(() => resolve({ ok: true, result: workspace(group, "updated") }), 2500),
        ),
    );
    for (let index = 0; index < 6; index++) {
      await act(async () => window.dispatchEvent(new Event("focus")));
      await advance(1000);
    }
    expect(documentText()).toBe("updated");
    expect(mocks.workspace.mock.calls.length).toBeLessThanOrEqual(3);
  });

  it("does not apply an older Group response after switching Groups", async () => {
    await render();
    let finish!: (value: unknown) => void;
    mocks.workspace.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await act(async () => window.dispatchEvent(new Event("focus")));
    mocks.workspace.mockImplementation((group: string) =>
      Promise.resolve({ ok: true, result: workspace(group, "current B contents") }),
    );
    await render("B");
    // Switching Group closes the previous workspace; the next explicit open shows B.
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(documentText()).toBe("current B contents");
    await act(async () => finish({ ok: true, result: workspace("A", "late A contents") }));
    expect(document.body.textContent).not.toContain("late A contents");
    expect(documentText()).toBe("current B contents");
  });

  it("preserves a dirty draft when its save fails and the older read completes", async () => {
    await render();
    let finish!: (value: unknown) => void;
    mocks.workspace.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await act(async () => window.dispatchEvent(new Event("focus")));
    await click("[data-edit-document]");
    mocks.save.mockResolvedValue({ ok: false, error: { message: "Rejected save" } });
    await click("[data-save-document]");
    await act(async () => finish({ ok: true, result: workspace("A", "remote contents") }));
    expect(documentText()).toBe("saved edit");
    // Explicitly discarding the local draft still reads the accepted remote snapshot.
    mocks.document.mockResolvedValue({
      ok: true,
      result: { document: workspace("A", "remote contents").documents![0] },
    });
    await click("[data-load-document]");
    expect(documentText()).toBe("remote contents");
  });

  it("keeps a conflicting draft and only overwrites the version the user confirmed", async () => {
    const current = createHash("sha256").update("remote").digest("hex");
    mocks.save.mockResolvedValueOnce({
      ok: false,
      error: {
        code: "voice_document_conflict",
        message: "conflict",
        details: { current_sha256: current },
      },
    });
    await render();
    await click("[data-toggle-editor]");
    await click("[data-edit-document]");
    await click("[data-save-document]");
    expect(documentText()).toBe("saved edit");
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("content");
    vi.mocked(window.confirm).mockReturnValueOnce(false);
    await click("[data-overwrite-document]");
    expect(mocks.save).toHaveBeenCalledTimes(1);
    await click("[data-overwrite-document]");
    expect(mocks.save).toHaveBeenCalledTimes(2);
    expect(mocks.save).toHaveBeenLastCalledWith(
      "A",
      expect.objectContaining({
        content: "saved edit",
        expectedContentSha256: current,
        expectedTargetId: workspace("A", "original").documents![0].content_target_id,
      }),
    );
    expect(documentText()).toBe("saved edit");
    expect(document.querySelector("[data-dirty]")?.getAttribute("data-dirty")).toBe("false");
  });

  it("does not discard a draft when a successful save response has no usable version", async () => {
    const saved = workspace("A", "saved edit").documents![0];
    delete saved.content_target_id;
    mocks.save.mockResolvedValueOnce({ ok: true, result: { document: saved } });
    await render();
    await click("[data-toggle-editor]");
    await click("[data-edit-document]");
    await click("[data-save-document]");
    expect(documentText()).toBe("saved edit");
    expect(document.querySelector("[data-dirty]")?.getAttribute("data-dirty")).toBe("true");
    expect(document.querySelector("[data-editing]")?.getAttribute("data-editing")).toBe("true");
  });

  it("keeps the editor mounted when an unchanged workspace refresh has no target token", async () => {
    const summary = workspace("A", "original");
    delete summary.documents![0].content_target_id;
    mocks.workspace.mockResolvedValue({ ok: true, result: summary });
    mocks.document.mockResolvedValue({
      ok: true,
      result: { document: workspace("A", "original").documents![0] },
    });
    await render();
    expect(mocks.document).toHaveBeenCalledTimes(1);
    await click("[data-toggle-editor]");
    await advance(30_000);
    expect(mocks.document).toHaveBeenCalledTimes(1);
    expect(document.querySelector("[data-editing]")?.getAttribute("data-editing")).toBe("true");
  });

  it("preserves the dirty draft when the selected document disappears from a refresh", async () => {
    await render();
    await click("[data-toggle-editor]");
    await click("[data-edit-document]");
    const remote = workspace("A", "remote");
    remote.documents = [];
    mocks.workspace.mockResolvedValue({ ok: true, result: remote });
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(documentText()).toBe("saved edit");
    expect(document.querySelector("[data-dirty]")?.getAttribute("data-dirty")).toBe("true");
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("unavailable");
    await click("[data-save-document]");
    expect(mocks.save).not.toHaveBeenCalled();
  });

  it("does not offer a write to a changed target or a deleted document", async () => {
    mocks.save.mockResolvedValueOnce({
      ok: false,
      error: { code: "voice_document_target_conflict", message: "changed location" },
    });
    await render();
    await click("[data-edit-document]");
    await click("[data-save-document]");
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("target");
    await click("[data-overwrite-document]");
    expect(mocks.save).toHaveBeenCalledTimes(1);
    expect(documentText()).toBe("saved edit");
  });

  it("requires another confirmation when the document changes again during overwrite", async () => {
    const first = createHash("sha256").update("first remote").digest("hex");
    const second = createHash("sha256").update("second remote").digest("hex");
    mocks.save
      .mockResolvedValueOnce({
        ok: false,
        error: {
          code: "voice_document_conflict",
          message: "conflict",
          details: { current_sha256: first },
        },
      })
      .mockResolvedValueOnce({
        ok: false,
        error: {
          code: "voice_document_conflict",
          message: "conflict",
          details: { current_sha256: second },
        },
      });
    await render();
    await click("[data-edit-document]");
    await click("[data-save-document]");
    await click("[data-overwrite-document]");
    expect(mocks.save).toHaveBeenLastCalledWith(
      "A",
      expect.objectContaining({ expectedContentSha256: first }),
    );
    expect(documentText()).toBe("saved edit");
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("content");
    await click("[data-overwrite-document]");
    expect(mocks.save).toHaveBeenLastCalledWith(
      "A",
      expect.objectContaining({ expectedContentSha256: second }),
    );
    expect(window.confirm).toHaveBeenCalledTimes(2);
  });

  it("keeps the draft and cannot overwrite when the document disappeared", async () => {
    mocks.save.mockResolvedValueOnce({
      ok: false,
      error: {
        code: "voice_document_conflict",
        message: "missing",
        details: { current_sha256: null },
      },
    });
    await render();
    await click("[data-edit-document]");
    await click("[data-save-document]");
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("missing");
    await click("[data-overwrite-document]");
    expect(mocks.save).toHaveBeenCalledTimes(1);
    expect(documentText()).toBe("saved edit");
  });

  it("keeps the draft when loading the latest is cancelled or fails", async () => {
    await render();
    await click("[data-edit-document]");
    vi.mocked(window.confirm).mockReturnValueOnce(false);
    await click("[data-load-document]");
    expect(mocks.document).not.toHaveBeenCalled();
    mocks.document.mockResolvedValueOnce({
      ok: false,
      error: { code: "failed", message: "offline" },
    });
    await click("[data-load-document]");
    expect(documentText()).toBe("saved edit");
    expect(document.querySelector("[data-dirty]")?.getAttribute("data-dirty")).toBe("true");
  });

  it("keeps later typing if the confirmed reload returns late", async () => {
    let finish!: (value: unknown) => void;
    mocks.document.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await render();
    await click("[data-edit-document]");
    await click("[data-load-document]");
    await click("[data-edit-document-later]");
    await act(async () =>
      finish({ ok: true, result: { document: workspace("A", "remote").documents![0] } }),
    );
    expect(documentText()).toBe("second edit");
    expect(document.querySelector("[data-dirty]")?.getAttribute("data-dirty")).toBe("true");
  });

  it("coalesces ledger updates into one follow-up and displays the latest snapshot", async () => {
    await render();
    const finishes: Array<(value: unknown) => void> = [];
    mocks.workspace.mockClear().mockImplementation(
      () =>
        new Promise((resolve) => {
          finishes.push(resolve);
        }),
    );
    await act(async () => window.dispatchEvent(new Event("focus")));
    for (let index = 0; index < 4; index++)
      await act(async () =>
        useGroupStore
          .getState()
          .appendEvent(
            { id: `document-${index}`, kind: "assistant.voice.document" } as LedgerEvent,
            "A",
          ),
      );
    expect(mocks.workspace).toHaveBeenCalledTimes(1);
    await act(async () => finishes[0]({ ok: true, result: workspace("A", "first snapshot") }));
    expect(documentText()).toBe("first snapshot");
    expect(mocks.workspace).toHaveBeenCalledTimes(2);
    await act(async () => finishes[1]({ ok: true, result: workspace("A", "latest snapshot") }));
    expect(documentText()).toBe("latest snapshot");
    expect(mocks.workspace).toHaveBeenCalledTimes(2);
  });

  it("rejects the earlier read when returning from settings requests a new snapshot", async () => {
    await render();
    let finish!: (value: unknown) => void;
    mocks.workspace.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await act(async () => window.dispatchEvent(new Event("focus")));
    await act(async () =>
      useModalStore.setState((state) => ({ modals: { ...state.modals, settings: true } })),
    );
    mocks.workspace.mockImplementation((group: string) =>
      Promise.resolve({ ok: true, result: workspace(group, "after settings") }),
    );
    await act(async () =>
      useModalStore.setState((state) => ({ modals: { ...state.modals, settings: false } })),
    );
    await act(async () => finish({ ok: true, result: workspace("A", "before settings") }));
    expect(documentText()).toBe("after settings");
  });

  it("does not load an earlier selected document into the later selection", async () => {
    const contents = workspace("A", "original");
    contents.documents!.push(
      { document_id: "B", document_path: "B.md", title: "B", status: "active", content_chars: 20 },
      { document_id: "C", document_path: "C.md", title: "C", status: "active", content_chars: 20 },
    );
    mocks.workspace.mockResolvedValue({ ok: true, result: contents });
    const finishes: Record<string, (value: unknown) => void> = {};
    mocks.document.mockImplementation(
      (_group: string, path: string) =>
        new Promise((resolve) => {
          finishes[path] = resolve;
        }),
    );
    await render();
    await click('[data-select-document="B.md"]');
    await click('[data-select-document="C.md"]');
    await act(async () =>
      finishes["C.md"]({
        ok: true,
        result: { document: { ...contents.documents![2], content: "C contents" } },
      }),
    );
    expect(documentText()).toBe("C contents");
    await act(async () =>
      finishes["B.md"]({
        ok: true,
        result: { document: { ...contents.documents![1], content: "late B contents" } },
      }),
    );
    expect(documentText()).toBe("C contents");
    expect(
      document.querySelector("[data-workspace-document]")?.getAttribute("data-workspace-document"),
    ).toBe("C.md");
  });

  it("preserves typing made while the selected document's content is loading", async () => {
    const contents = workspace("A", "original");
    const selected = {
      document_id: "B",
      document_path: "B.md",
      title: "B",
      status: "active",
      content_chars: 20,
    };
    contents.documents!.push(selected);
    mocks.workspace.mockResolvedValue({ ok: true, result: contents });
    let finish!: (value: unknown) => void;
    mocks.document.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await render();
    await click('[data-select-document="B.md"]');
    await click("[data-edit-document]");
    await act(async () =>
      finish({ ok: true, result: { document: { ...selected, content: "late contents" } } }),
    );
    expect(documentText()).toBe("saved edit");
  });

  it("does not save a selected document before its content and version have loaded", async () => {
    const contents = workspace("A", "original");
    const selected = {
      document_id: "B",
      document_path: "B.md",
      title: "B",
      status: "active",
      content_chars: 20,
    };
    contents.documents!.push(selected);
    mocks.workspace.mockResolvedValue({ ok: true, result: contents });
    let finish!: (value: unknown) => void;
    mocks.document.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await render();
    await click('[data-select-document="B.md"]');
    await click("[data-edit-document]");
    await click("[data-save-document]");
    expect(mocks.save).not.toHaveBeenCalled();
    await act(async () =>
      finish({ ok: true, result: { document: { ...selected, content: "before save" } } }),
    );
    expect(documentText()).toBe("saved edit");
  });

  it("keeps edits made after a save was sent while its response is pending", async () => {
    let finish!: (value: unknown) => void;
    mocks.save.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await render();
    await click("[data-toggle-editor]");
    await click("[data-edit-document]");
    await click("[data-save-document]");
    expect(mocks.save).toHaveBeenCalledWith(
      "A",
      expect.objectContaining({ content: "saved edit" }),
    );
    await click("[data-edit-document-later]");
    await act(async () =>
      finish({ ok: true, result: { document: workspace("A", "saved edit").documents![0] } }),
    );
    expect(documentText()).toBe("second edit");
    expect(document.querySelector("[data-workspace-document]")?.getAttribute("data-dirty")).toBe(
      "true",
    );
    expect(document.querySelector("[data-workspace-document]")?.getAttribute("data-editing")).toBe(
      "true",
    );
    await click("[data-save-document]");
    expect(mocks.save).toHaveBeenLastCalledWith(
      "A",
      expect.objectContaining({ content: "second edit" }),
    );
    expect(document.querySelector("[data-workspace-document]")?.getAttribute("data-dirty")).toBe(
      "false",
    );
  });

  it("keeps edits made while a document instruction response is pending", async () => {
    let finish!: (value: unknown) => void;
    mocks.instruction.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await render();
    await act(async () => {
      const input = document.querySelector<HTMLTextAreaElement>("[data-voice-instruction-input]")!;
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
        input,
        "Review this",
      );
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await click('[data-voice-input-action="update"]');
    expect(mocks.instruction).toHaveBeenCalled();
    await click("[data-toggle-editor]");
    await click("[data-edit-document-later]");
    await act(async () =>
      finish({
        ok: true,
        result: { document: workspace("A", "original").documents![0], request_id: "r" },
      }),
    );
    expect(documentText()).toBe("second edit");
    expect(document.querySelector("[data-workspace-document]")?.getAttribute("data-dirty")).toBe(
      "true",
    );
    expect(document.querySelector("[data-workspace-document]")?.getAttribute("data-editing")).toBe(
      "true",
    );
  });

  it("does not treat a metadata-only instruction reply as a new document version", async () => {
    const oldMetadata = workspace("A", "original").documents![0];
    delete oldMetadata.content_target_id;
    mocks.instruction.mockResolvedValueOnce({
      ok: true,
      result: { document: oldMetadata, request_id: "metadata-only" },
    });
    await render();
    await act(async () => {
      const input = document.querySelector<HTMLTextAreaElement>("[data-voice-instruction-input]")!;
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
        input,
        "Review this",
      );
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await click('[data-voice-input-action="update"]');
    expect(documentText()).toBe("original");
    expect(
      document.querySelector("[data-remote-changed]")?.getAttribute("data-remote-changed"),
    ).toBe("false");
    await click("[data-edit-document]");
    await click("[data-save-document]");
    expect(mocks.save).toHaveBeenCalledWith(
      "A",
      expect.objectContaining({
        expectedTargetId: workspace("A", "original").documents![0].content_target_id,
      }),
    );
  });

  it("restores a Group's unsaved draft and original write base after switching away", async () => {
    await render("A");
    await click("[data-toggle-editor]");
    await click("[data-edit-document]");
    await render("B");
    mocks.workspace.mockImplementation((group: string) =>
      Promise.resolve({
        ok: true,
        result: workspace(group, group === "A" ? "remote change" : "B content"),
      }),
    );
    await render("A");
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(documentText()).toBe("saved edit");
    expect(document.querySelector("[data-dirty]")?.getAttribute("data-dirty")).toBe("true");
    expect(document.querySelector("[data-editing]")?.getAttribute("data-editing")).toBe("true");
    expect(
      document.querySelector("[data-remote-changed]")?.getAttribute("data-remote-changed"),
    ).toBe("true");
    await click("[data-save-document]");
    expect(mocks.save).toHaveBeenCalledWith(
      "A",
      expect.objectContaining({
        content: "saved edit",
        expectedContentSha256: workspace("A", "original").documents![0].content_sha256,
      }),
    );
  });

  it("keeps separate drafts for two Groups without writing them into browser storage", async () => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    await render("A");
    await click("[data-edit-document]");
    await render("B");
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    await click("[data-edit-document-later]");
    await render("A");
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(documentText()).toBe("saved edit");
    await render("B");
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(documentText()).toBe("second edit");
    for (const storage of [window.localStorage, window.sessionStorage]) {
      for (let index = 0; index < storage.length; index++) {
        const value = storage.getItem(storage.key(index)!) || "";
        expect(value).not.toContain("saved edit");
        expect(value).not.toContain("second edit");
      }
    }
  });

  it("reports a dirty embedded voice document while it is held for another Group", async () => {
    const parent = { postMessage: vi.fn() } as unknown as Window;
    vi.spyOn(window, "parent", "get").mockReturnValue(parent);
    window.history.replaceState(null, "", "/ui/connect");
    usePendingPageWork.setState({ owners: new Set() });
    try {
      await render("A");
      await click("[data-edit-document]");
      expect(usePendingPageWork.getState().owners.size).toBe(1);
      await render("B");
      expect(usePendingPageWork.getState().owners.size).toBe(1);
      await render("A");
      await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
      await click("[data-reset-document]");
      expect(usePendingPageWork.getState().owners.size).toBe(0);
    } finally {
      window.history.replaceState(null, "", "/");
      vi.restoreAllMocks();
    }
  });

  it("keeps an already unavailable draft unavailable after switching Groups and a failed refresh", async () => {
    await render("A");
    await click("[data-toggle-editor]");
    await click("[data-edit-document]");
    const absent = workspace("A", "original");
    absent.documents = [];
    mocks.workspace.mockResolvedValueOnce({ ok: true, result: absent });
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("unavailable");
    await render("B");
    mocks.workspace.mockImplementation((group: string) =>
      group === "A"
        ? Promise.resolve({ ok: false, error: { message: "offline", code: "offline" } })
        : Promise.resolve({ ok: true, result: workspace(group, "original") }),
    );
    await render("A");
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(documentText()).toBe("saved edit");
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("unavailable");
    await click("[data-save-document]");
    expect(mocks.save).not.toHaveBeenCalled();
    mocks.workspace.mockResolvedValue({ ok: true, result: workspace("A", "original") });
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("");
    expect(documentText()).toBe("saved edit");
    await click("[data-save-document]");
    expect(mocks.save).toHaveBeenCalledTimes(1);
  });

  it("keeps a restored draft if its document was archived while another Group was open", async () => {
    await render("A");
    await click("[data-toggle-editor]");
    await click("[data-edit-document]");
    await render("B");
    mocks.workspace.mockImplementation((group: string) => {
      const state = workspace(group, "original");
      if (group === "A") state.documents = [];
      return Promise.resolve({ ok: true, result: state });
    });
    await render("A");
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(documentText()).toBe("saved edit");
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("unavailable");
    await click("[data-save-document]");
    expect(mocks.save).not.toHaveBeenCalled();
  });

  it("restores a content conflict and its conditional overwrite after switching Groups", async () => {
    const remoteSha = createHash("sha256").update("remote edit").digest("hex");
    mocks.save.mockResolvedValueOnce({
      ok: false,
      error: {
        code: "voice_document_conflict",
        message: "changed",
        details: { current_sha256: remoteSha },
      },
    });
    await render("A");
    await click("[data-edit-document]");
    await click("[data-save-document]");
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("content");
    await render("B");
    await render("A");
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(documentText()).toBe("saved edit");
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("content");
    await click("[data-overwrite-document]");
    expect(mocks.save).toHaveBeenCalledTimes(2);
    expect(mocks.save).toHaveBeenLastCalledWith(
      "A",
      expect.objectContaining({
        expectedContentSha256: remoteSha,
        expectedTargetId: workspace("A", "original").documents![0].content_target_id,
      }),
    );
  });

  it("restores a target conflict without allowing overwrite after switching Groups", async () => {
    mocks.save.mockResolvedValueOnce({
      ok: false,
      error: { code: "voice_document_target_conflict", message: "changed location" },
    });
    await render("A");
    await click("[data-edit-document]");
    await click("[data-save-document]");
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("target");
    await render("B");
    await render("A");
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(documentText()).toBe("saved edit");
    expect(document.querySelector("[data-save-conflict]")?.textContent).toBe("target");
    await click("[data-overwrite-document]");
    expect(mocks.save).toHaveBeenCalledTimes(1);
  });

  it("does not apply an old save response to a restored and edited draft", async () => {
    let finish!: (value: unknown) => void;
    mocks.save.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await render("A");
    await click("[data-edit-document]");
    await click("[data-save-document]");
    await render("B");
    mocks.workspace.mockImplementation((group: string) =>
      Promise.resolve({
        ok: true,
        result: workspace(group, group === "A" ? "newer remote" : "B content"),
      }),
    );
    await render("A");
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(documentText()).toBe("saved edit");
    await click("[data-edit-document-later]");
    await act(async () =>
      finish({ ok: true, result: { document: workspace("A", "saved edit").documents![0] } }),
    );
    expect(documentText()).toBe("second edit");
    expect(document.querySelector("[data-dirty]")?.getAttribute("data-dirty")).toBe("true");
  });

  it("does not apply an old instruction response after switching away and reloading a newer version", async () => {
    let finish!: (value: unknown) => void;
    mocks.instruction.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await render("A");
    await act(async () => {
      const input = document.querySelector<HTMLTextAreaElement>("[data-voice-instruction-input]")!;
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
        input,
        "Review this",
      );
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await click('[data-voice-input-action="update"]');
    await render("B");
    mocks.workspace.mockImplementation((group: string) =>
      Promise.resolve({
        ok: true,
        result: workspace(group, group === "A" ? "newer remote" : "B content"),
      }),
    );
    await render("A");
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(documentText()).toBe("newer remote");
    // Keep the follow-up read pending so it cannot mask an incorrect late reply.
    mocks.workspace.mockImplementation(() => new Promise(() => {}));
    await act(async () =>
      finish({
        ok: true,
        result: { document: workspace("A", "old result").documents![0], request_id: "r" },
      }),
    );
    expect(documentText()).toBe("newer remote");
  });

  it("keeps an accepted document save when a previous workspace read returns late", async () => {
    await render();
    let finish!: (value: unknown) => void;
    mocks.workspace.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await act(async () => window.dispatchEvent(new Event("focus")));
    await click("[data-edit-document]");
    await click("[data-save-document]");
    expect(mocks.save).toHaveBeenCalledWith(
      "A",
      expect.objectContaining({ content: "saved edit" }),
    );
    expect(documentText()).toBe("saved edit");
    await act(async () => finish({ ok: true, result: workspace("A", "before the save") }));
    expect(documentText()).toBe("saved edit");
    await click("[data-edit-document]");
    await click("[data-load-document]");
    expect(documentText()).toBe("saved edit");
  });

  it("keeps processing ledger-triggered refreshes while the workspace is hidden by settings", async () => {
    await render();
    mocks.workspace.mockImplementation((group: string) =>
      Promise.resolve({ ok: true, result: workspace(group, "updated in settings") }),
    );
    await act(async () => {
      useModalStore.setState((state) => ({ modals: { ...state.modals, settings: true } }));
      useGroupStore
        .getState()
        .appendEvent(
          { id: "document-update", kind: "assistant.voice.document" } as LedgerEvent,
          "A",
        );
    });
    await act(async () =>
      useModalStore.setState((state) => ({ modals: { ...state.modals, settings: false } })),
    );
    expect(documentText()).toBe("updated in settings");
  });

  it("defers active document body fetch until the workspace is opened", async () => {
    const metadataOnly = workspace("A", "");
    delete metadataOnly.documents![0].content;
    metadataOnly.documents![0].content_chars = 12;
    mocks.workspace.mockResolvedValue({ ok: true, result: metadataOnly });
    mocks.document.mockResolvedValue({
      ok: true,
      result: { document: { ...metadataOnly.documents![0], content: "loaded on open" } },
    });

    await render("A", false);
    expect(mocks.workspace).toHaveBeenCalledTimes(1);
    expect(mocks.document).not.toHaveBeenCalled();

    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(mocks.document).toHaveBeenCalledWith("A", "A.md");
    expect(documentText()).toBe("loaded on open");
  });

  it.each(["unchanged", "changed", "empty"])(
    "does not infer a body conflict from a closed metadata refresh (%s body on reopen)",
    async (change) => {
      await render();
      await click("[data-edit-document]");
      await click("[data-voice-sheet-close]");

      const metadata = workspace("A", "");
      delete metadata.documents![0].content;
      metadata.documents![0].content_chars = 8;
      mocks.workspace.mockResolvedValue({ ok: true, result: metadata });
      mocks.document
        .mockClear()
        .mockResolvedValue({
          ok: true,
          result: {
            document: {
              ...metadata.documents![0],
              content:
                change === "unchanged" ? "original" : change === "empty" ? "" : "remote edit",
            },
          },
        });

      await act(async () => window.dispatchEvent(new Event("focus")));
      expect(mocks.document).not.toHaveBeenCalled();
      // Keep the reopen read pending so we can also inspect the metadata-only state.
      let finish!: (value: unknown) => void;
      mocks.document.mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      );
      await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
      expect(documentText()).toBe("saved edit");
      expect(
        document.querySelector("[data-remote-changed]")?.getAttribute("data-remote-changed"),
      ).toBe("false");
      await act(async () =>
        finish({
          ok: true,
          result: {
            document: {
              ...metadata.documents![0],
              content:
                change === "unchanged" ? "original" : change === "empty" ? "" : "remote edit",
            },
          },
        }),
      );
      expect(documentText()).toBe("saved edit");
      expect(
        document.querySelector("[data-remote-changed]")?.getAttribute("data-remote-changed"),
      ).toBe(String(change !== "unchanged"));
    },
  );
});
