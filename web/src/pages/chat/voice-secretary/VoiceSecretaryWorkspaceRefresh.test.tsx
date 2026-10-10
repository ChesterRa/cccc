// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { VoiceSecretaryComposerControl } from "../VoiceSecretaryComposerControl";
import { useGroupStore, useModalStore, useUIStore } from "../../../stores";
import type { AssistantStateResult, AssistantVoiceDocument, LedgerEvent } from "../../../types";

const mocks = vi.hoisted(() => ({
  workspace: vi.fn(),
  status: vi.fn(),
  save: vi.fn(),
  document: vi.fn(),
  t: (key: string, values?: { defaultValue?: string }) => values?.defaultValue || key,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.t }) }));
vi.mock("../../../services/api", async (original) => ({
  ...(await original<typeof import("../../../services/api")>()),
  fetchVoiceAssistantWorkspace: mocks.workspace,
  fetchVoiceAssistantStatus: mocks.status,
  saveVoiceAssistantDocument: mocks.save,
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
  }: {
    documentDraft: string;
    activeDocumentWritePath: string;
    onEditDocumentChange: (value: string) => void;
    onSaveDocument: () => void;
    onLoadLatestDocument: () => void;
  }) => (
    <>
      <output data-workspace-document={activeDocumentWritePath}>{documentDraft}</output>
      <button onClick={() => onEditDocumentChange("saved edit")} data-edit-document />
      <button onClick={onSaveDocument} data-save-document />
      <button onClick={onLoadLatestDocument} data-load-document />
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
    await click("[data-load-document]");
    expect(documentText()).toBe("remote contents");
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

  it("rejects a previous content read after an accepted save to the same selected document", async () => {
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
    expect(mocks.save).toHaveBeenLastCalledWith(
      "A",
      expect.objectContaining({ documentPath: "B.md", content: "saved edit" }),
    );
    await act(async () =>
      finish({ ok: true, result: { document: { ...selected, content: "before save" } } }),
    );
    expect(documentText()).toBe("saved edit");
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
});
